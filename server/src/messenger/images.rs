//! Bounded, session-bound relay. Image bytes exist only for a requested live transfer.
use gg_protocol::{
  consts::AckStatus,
  packets::{ImageId, ImageTransfer},
};
use parking_lot::Mutex;
use std::{
  collections::HashMap,
  time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub enum ImageEvent {
  Request {
    recipient: u32,
    id: ImageId,
    owner_session: u64,
  },
  Ready {
    owner: u32,
    id: ImageId,
    request_session: u64,
  },
  Unavailable {
    owner: u32,
    id: ImageId,
    request_session: u64,
  },
}
type Key = (u32, u32, ImageId);
pub type ImageNotification = (u32, ImageEvent);
pub struct ImagePayload {
  pub filename: Vec<u8>,
  pub data: Vec<u8>,
}
struct Upload {
  session: u64,
  waiter: u64,
  started: Instant,
  filename: Option<Vec<u8>>,
  data: Vec<u8>,
  ready: bool,
}
#[derive(PartialEq, Eq)]
pub enum Begin {
  New,
  Existing,
  Full,
}
pub struct ImageService {
  uploads: Mutex<HashMap<Key, Upload>>,
}
impl ImageService {
  pub fn new() -> Self {
    Self {
      uploads: Mutex::new(HashMap::new()),
    }
  }
  pub fn begin(
    &self,
    owner: u32,
    recipient: u32,
    id: ImageId,
    owner_session: u64,
    waiter: u64,
  ) -> Begin {
    let mut uploads = self.uploads.lock();
    let key = (owner, recipient, id);
    if let Some(upload) = uploads.get(&key) {
      if upload.session == owner_session && upload.waiter == waiter {
        return Begin::Existing;
      }
      uploads.remove(&key);
    }
    if uploads.len() >= 64
      || uploads.keys().filter(|k| k.0 == owner).count() >= 4
      || uploads.keys().filter(|k| k.1 == recipient).count() >= 8
    {
      return Begin::Full;
    }
    uploads.insert(
      key,
      Upload {
        session: owner_session,
        waiter,
        started: Instant::now(),
        filename: None,
        data: vec![],
        ready: false,
      },
    );
    Begin::New
  }
  fn unavailable(key: Key, upload: Upload) -> ImageNotification {
    (
      key.1,
      ImageEvent::Unavailable {
        owner: key.0,
        id: key.2,
        request_session: upload.waiter,
      },
    )
  }
  pub fn cancel(
    &self,
    owner: u32,
    recipient: u32,
    id: ImageId,
    session: u64,
  ) -> Option<ImageNotification> {
    let mut uploads = self.uploads.lock();
    let key = (owner, recipient, id);
    if !uploads.get(&key).is_some_and(|u| u.session == session) {
      return None;
    }
    Some(Self::unavailable(key, uploads.remove(&key).unwrap()))
  }
  pub fn expecting(&self, owner: u32, recipient: u32, id: ImageId, session: u64) -> bool {
    self
      .uploads
      .lock()
      .get(&(owner, recipient, id))
      .is_some_and(|u| u.session == session && !u.ready)
  }
  pub fn expire(&self) -> Vec<ImageNotification> {
    let mut uploads = self.uploads.lock();
    let keys: Vec<_> = uploads
      .iter()
      .filter(|(_, u)| u.started.elapsed() >= Duration::from_secs(60))
      .map(|(key, _)| *key)
      .collect();
    keys
      .into_iter()
      .map(|key| Self::unavailable(key, uploads.remove(&key).unwrap()))
      .collect()
  }
  pub fn disconnect(&self, uin: u32, session: u64) -> Vec<ImageNotification> {
    let mut uploads = self.uploads.lock();
    let keys: Vec<_> = uploads
      .iter()
      .filter(|(k, u)| (k.0 == uin && u.session == session) || (k.1 == uin && u.waiter == session))
      .map(|(key, _)| *key)
      .collect();
    keys
      .into_iter()
      .filter_map(|key| {
        let upload = uploads.remove(&key).unwrap();
        if key.1 == uin && upload.waiter == session {
          None
        } else {
          Some(Self::unavailable(key, upload))
        }
      })
      .collect()
  }
  /// Take once, only for the requesting session. Ready payloads share the bounded transfer budget.
  pub fn take(
    &self,
    owner: u32,
    recipient: u32,
    id: ImageId,
    request_session: u64,
  ) -> Option<ImagePayload> {
    let mut uploads = self.uploads.lock();
    let key = (owner, recipient, id);
    if !uploads
      .get(&key)
      .is_some_and(|u| u.waiter == request_session && u.ready)
    {
      return None;
    }
    let upload = uploads.remove(&key).unwrap();
    if upload.started.elapsed() >= Duration::from_secs(60) {
      return None;
    }
    Some(ImagePayload {
      filename: upload.filename.unwrap(),
      data: upload.data,
    })
  }
  pub fn receive(
    &self,
    owner: u32,
    recipient: u32,
    session: u64,
    transfer: ImageTransfer,
  ) -> (AckStatus, Option<ImageNotification>) {
    let ImageTransfer::Reply {
      id,
      first,
      filename,
      data,
    } = transfer
    else {
      return (AckStatus::NotDelivered, None);
    };
    let key = (owner, recipient, id);
    let mut uploads = self.uploads.lock();
    let Some(upload) = uploads.get_mut(&key) else {
      return (AckStatus::NotDelivered, None);
    };
    if upload.session != session || upload.ready {
      return (AckStatus::NotDelivered, None);
    }
    let invalid = upload.started.elapsed() >= Duration::from_secs(60)
      || (first && (filename.is_none() || upload.filename.is_some()))
      || (!first && upload.filename.is_none())
      || upload.data.len() + data.len() > id.size as usize;
    if invalid {
      return (
        AckStatus::NotDelivered,
        Some(Self::unavailable(key, uploads.remove(&key).unwrap())),
      );
    }
    if first {
      upload.filename = filename.map(|name| safe_filename(&name));
      upload.data = Vec::with_capacity(id.size as usize);
    }
    upload.data.extend_from_slice(&data);
    if upload.data.len() < id.size as usize {
      return (AckStatus::Delivered, None);
    }
    if crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(&upload.data) != id.crc32 {
      return (
        AckStatus::NotDelivered,
        Some(Self::unavailable(key, uploads.remove(&key).unwrap())),
      );
    }
    upload.ready = true;
    (
      AckStatus::Delivered,
      Some((
        recipient,
        ImageEvent::Ready {
          owner,
          id,
          request_session: upload.waiter,
        },
      )),
    )
  }
}
fn safe_filename(name: &[u8]) -> Vec<u8> {
  let name = name
    .rsplit(|c| *c == b'/' || *c == b'\\')
    .next()
    .unwrap_or_default();
  let stem = name
    .split(|b| *b == b'.')
    .next()
    .unwrap_or_default()
    .to_ascii_uppercase();
  let device = [b"CON".as_slice(), b"PRN", b"AUX", b"NUL"].contains(&stem.as_slice())
    || (stem.len() == 4
      && (stem.starts_with(b"COM") || stem.starts_with(b"LPT"))
      && (b'1'..=b'9').contains(&stem[3]));
  if name.is_empty()
    || name.len() > 255
    || name == b"."
    || name == b".."
    || device
    || matches!(name.last(), Some(b'.' | b' '))
    || name
      .iter()
      .any(|b| *b < 32 || *b == 127 || b"<>:\"|?*".contains(b))
  {
    b"image.bin".to_vec()
  } else {
    name.to_vec()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  fn id(n: u32) -> ImageId {
    ImageId { size: 9, crc32: n }
  }
  fn complete(s: &ImageService) {
    let (ack, event) = s.receive(
      1,
      2,
      10,
      ImageTransfer::Reply {
        id: id(0xcbf43926),
        first: true,
        filename: Some(b"image.png".to_vec()),
        data: b"123456789".to_vec(),
      },
    );
    assert_eq!(ack, AckStatus::Delivered);
    assert!(matches!(
      event,
      Some((
        2,
        ImageEvent::Ready {
          request_session: 20,
          ..
        }
      ))
    ));
  }
  #[test]
  fn transfer_limits_are_global_and_per_participant() {
    let s = ImageService::new();
    for n in 0..4 {
      assert!(s.begin(1, 2, id(n), 10, 20) == Begin::New);
    }
    assert!(s.begin(1, 3, id(5), 10, 30) == Begin::Full);
    for n in 0..4 {
      assert!(s.begin(3, 2, id(n), 30, 20) == Begin::New);
    }
    assert!(s.begin(4, 2, id(9), 40, 20) == Begin::Full);
    for n in 0..56 {
      assert!(s.begin(100 + n, 1000 + n, id(n), n as u64, 1000 + n as u64) == Begin::New);
    }
    assert!(s.begin(999, 999, id(1), 999, 999) == Begin::Full);
  }
  #[test]
  fn completed_payload_is_private_single_use_and_not_a_cache() {
    let s = ImageService::new();
    let id = id(0xcbf43926);
    s.begin(1, 2, id, 10, 20);
    complete(&s);
    assert!(s.take(1, 3, id, 20).is_none());
    assert!(s.take(1, 2, id, 21).is_none());
    assert_eq!(s.take(1, 2, id, 20).unwrap().data, b"123456789");
    assert!(s.take(1, 2, id, 20).is_none());
    assert!(s.uploads.lock().is_empty());
  }
  #[test]
  fn ready_images_still_expire_and_hold_a_transfer_slot() {
    let s = ImageService::new();
    let id = id(0xcbf43926);
    s.begin(1, 2, id, 10, 20);
    complete(&s);
    for n in 0..3 {
      s.begin(1, 2, super::tests::id(n), 10, 20);
    }
    assert!(s.begin(1, 2, super::tests::id(4), 10, 20) == Begin::Full);
    s.uploads.lock().get_mut(&(1, 2, id)).unwrap().started =
      Instant::now() - Duration::from_secs(61);
    assert_eq!(s.expire().len(), 1);
    assert!(s.take(1, 2, id, 20).is_none());
  }
  #[test]
  fn disconnect_discards_buffers_and_old_sessions_cannot_cancel_new_transfers() {
    let s = ImageService::new();
    let id = id(0xcbf43926);
    s.begin(1, 2, id, 10, 20);
    complete(&s);
    assert_eq!(s.disconnect(1, 10).len(), 1);
    assert!(s.take(1, 2, id, 20).is_none());
    s.begin(1, 2, id, 10, 20);
    complete(&s);
    assert!(s.disconnect(2, 20).is_empty());
    assert!(s.uploads.lock().is_empty());
    s.begin(1, 2, id, 11, 21);
    assert!(s.disconnect(1, 10).is_empty());
    assert!(s.disconnect(2, 20).is_empty());
    assert!(s.expecting(1, 2, id, 11));
    let (ack, _) = s.receive(
      1,
      2,
      10,
      ImageTransfer::Reply {
        id,
        first: true,
        filename: Some(b"x.png".to_vec()),
        data: vec![1],
      },
    );
    assert_eq!(ack, AckStatus::NotDelivered);
    assert!(s.uploads.lock()[&(1, 2, id)].data.is_empty());
  }
  #[test]
  fn duplicate_start_and_cumulative_overflow_cancel_assembly() {
    let s = ImageService::new();
    let id = ImageId { size: 2, crc32: 1 };
    for duplicate in [true, false] {
      s.begin(1, 2, id, 10, 20);
      let (ack, _) = s.receive(
        1,
        2,
        10,
        ImageTransfer::Reply {
          id,
          first: true,
          filename: Some(b"image.png".to_vec()),
          data: vec![1],
        },
      );
      assert_eq!(ack, AckStatus::Delivered);
      let (ack, event) = s.receive(
        1,
        2,
        10,
        ImageTransfer::Reply {
          id,
          first: duplicate,
          filename: duplicate.then(|| b"image.png".to_vec()),
          data: vec![2, 3],
        },
      );
      assert_eq!(ack, AckStatus::NotDelivered);
      assert!(event.is_some());
      assert!(s.uploads.lock().is_empty());
    }
  }
  #[test]
  fn unsafe_names_never_reach_a_windows_client() {
    for name in [
      b"CON.gif".as_slice(),
      b"a:secret.png",
      b"..",
      b"bad\x01.jpg",
      b"NUL",
      b"LPT1.bmp",
      b"COM9.png",
      b"trailing.",
    ] {
      assert_eq!(safe_filename(name), b"image.bin");
    }
    assert_eq!(safe_filename(b"C:\\private\\photo.png"), b"photo.png");
    assert_eq!(safe_filename(b"/home/private/photo.gif"), b"photo.gif");
  }
}
