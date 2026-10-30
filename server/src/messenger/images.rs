//! Bounded, session-bound image assembly. Only verified complete images reach SQLite/receivers.
use crate::models::{DatabasePool, RepositoryError, images};
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
struct Upload {
  session: u64,
  waiter: Option<u64>,
  started: Instant,
  filename: Option<Vec<u8>>,
  data: Vec<u8>,
  completing: bool,
}
#[derive(PartialEq, Eq)]
pub enum Begin {
  New,
  Existing,
  Full,
}
pub struct ImageService {
  uploads: Mutex<HashMap<Key, Upload>>,
  pool: DatabasePool,
}
impl ImageService {
  pub fn new(pool: &DatabasePool) -> Self {
    Self {
      uploads: Mutex::new(HashMap::new()),
      pool: pool.clone(),
    }
  }
  pub fn begin(
    &self,
    owner: u32,
    recipient: u32,
    id: ImageId,
    owner_session: u64,
    waiter: Option<u64>,
  ) -> Begin {
    let mut uploads = self.uploads.lock();
    let key = (owner, recipient, id);
    if let Some(upload) = uploads.get_mut(&key) {
      if upload.session == owner_session {
        if waiter.is_some() {
          upload.waiter = waiter;
        }
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
        completing: false,
      },
    );
    Begin::New
  }
  fn notification(key: Key, upload: Upload, ready: bool) -> Option<ImageNotification> {
    upload.waiter.map(|request_session| {
      (
        key.1,
        if ready {
          ImageEvent::Ready {
            owner: key.0,
            id: key.2,
            request_session,
          }
        } else {
          ImageEvent::Unavailable {
            owner: key.0,
            id: key.2,
            request_session,
          }
        },
      )
    })
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
    Self::notification(key, uploads.remove(&key).unwrap(), false)
  }
  pub fn expecting(&self, owner: u32, recipient: u32, id: ImageId, session: u64) -> bool {
    self
      .uploads
      .lock()
      .get(&(owner, recipient, id))
      .is_some_and(|u| u.session == session && !u.completing)
  }
  pub fn expire(&self) -> Vec<ImageNotification> {
    let mut uploads = self.uploads.lock();
    let keys: Vec<_> = uploads
      .iter()
      .filter(|(_, u)| u.started.elapsed() >= Duration::from_secs(60) && !u.completing)
      .map(|(k, _)| *k)
      .collect();
    keys
      .into_iter()
      .filter_map(|key| Self::notification(key, uploads.remove(&key).unwrap(), false))
      .collect()
  }
  pub fn disconnect(&self, uin: u32, session: u64) -> Vec<ImageNotification> {
    let mut uploads = self.uploads.lock();
    let keys: Vec<_> = uploads
      .iter()
      .filter(|(k, u)| k.0 == uin && u.session == session)
      .map(|(k, _)| *k)
      .collect();
    let notifications = keys
      .into_iter()
      .filter_map(|key| Self::notification(key, uploads.remove(&key).unwrap(), false))
      .collect();
    for (key, upload) in uploads.iter_mut() {
      if key.1 == uin && upload.waiter == Some(session) {
        upload.waiter = None;
      }
    }
    notifications
  }
  pub async fn receive(
    &self,
    owner: u32,
    recipient: u32,
    session: u64,
    transfer: ImageTransfer,
  ) -> Result<(AckStatus, Option<ImageNotification>), RepositoryError> {
    let ImageTransfer::Reply {
      id,
      first,
      filename,
      data,
    } = transfer
    else {
      return Ok((AckStatus::NotDelivered, None));
    };
    let key = (owner, recipient, id);
    let complete = {
      let mut uploads = self.uploads.lock();
      let Some(upload) = uploads.get_mut(&key) else {
        return Ok((AckStatus::NotDelivered, None));
      };
      if upload.session != session || upload.completing {
        return Ok((AckStatus::NotDelivered, None));
      }
      let invalid = upload.started.elapsed() >= Duration::from_secs(60)
        || (first && (filename.is_none() || upload.filename.is_some()))
        || (!first && upload.filename.is_none())
        || upload.data.len() + data.len() > id.size as usize;
      if invalid {
        return Ok((
          AckStatus::NotDelivered,
          Self::notification(key, uploads.remove(&key).unwrap(), false),
        ));
      }
      if first {
        upload.filename = filename.map(|name| safe_filename(&name));
      }
      upload.data.extend_from_slice(&data);
      if upload.data.len() < id.size as usize {
        return Ok((AckStatus::Delivered, None));
      }
      if crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(&upload.data) != id.crc32 {
        return Ok((
          AckStatus::NotDelivered,
          Self::notification(key, uploads.remove(&key).unwrap(), false),
        ));
      }
      upload.completing = true;
      (
        upload.filename.clone().unwrap(),
        std::mem::take(&mut upload.data),
      )
    };
    // On a database error keep the waiter until the session's cleanup cancels this upload.
    let saved = images::store(&self.pool, owner, recipient, id, &complete.0, &complete.1).await?;
    let notification = {
      let mut uploads = self.uploads.lock();
      if uploads
        .get(&key)
        .is_some_and(|u| u.session == session && u.completing)
      {
        Self::notification(key, uploads.remove(&key).unwrap(), saved)
      } else {
        None
      }
    };
    Ok((
      if saved {
        AckStatus::Delivered
      } else {
        AckStatus::NotDelivered
      },
      notification,
    ))
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
  fn service() -> ImageService {
    ImageService::new(
      &sqlx::sqlite::SqlitePoolOptions::new()
        .connect_lazy("sqlite::memory:")
        .unwrap(),
    )
  }
  fn id(n: u32) -> ImageId {
    ImageId {
      size: 100,
      crc32: n,
    }
  }
  #[tokio::test]
  async fn transfer_limits_are_global_and_per_participant() {
    let s = service();
    for n in 0..4 {
      assert!(s.begin(1, 2, id(n), 10, None) == Begin::New);
    }
    assert!(s.begin(1, 3, id(5), 10, None) == Begin::Full);
    for n in 0..4 {
      assert!(s.begin(3, 2, id(n), 30, None) == Begin::New);
    }
    assert!(s.begin(4, 2, id(9), 40, None) == Begin::Full);
    for n in 0..56 {
      assert!(s.begin(100 + n, 1000 + n, id(n), n as u64, None) == Begin::New);
    }
    assert!(s.begin(999, 999, id(1), 999, None) == Begin::Full);
  }
  #[tokio::test]
  async fn partial_upload_expires_and_notifies_only_requesting_session() {
    let s = service();
    s.begin(1, 2, id(1), 10, None);
    assert!(s.begin(1, 2, id(1), 10, Some(20)) == Begin::Existing);
    s.uploads.lock().get_mut(&(1, 2, id(1))).unwrap().started =
      Instant::now() - Duration::from_secs(61);
    let events = s.expire();
    assert_eq!(events.len(), 1);
    assert!(matches!(
      events[0],
      (
        2,
        ImageEvent::Unavailable {
          request_session: 20,
          ..
        }
      )
    ));
    assert!(s.uploads.lock().is_empty());
  }
  #[tokio::test]
  async fn replaced_source_cannot_append_or_cancel_new_transfer() {
    let s = service();
    s.begin(1, 2, id(1), 10, Some(20));
    s.begin(1, 2, id(1), 11, Some(21));
    assert!(s.disconnect(1, 10).is_empty());
    let (ack, notification) = s
      .receive(
        1,
        2,
        10,
        ImageTransfer::Reply {
          id: id(1),
          first: true,
          filename: Some(b"image.gif".to_vec()),
          data: vec![1],
        },
      )
      .await
      .unwrap();
    assert_eq!(ack, AckStatus::NotDelivered);
    assert!(notification.is_none());
    assert_eq!(s.uploads.lock()[&(1, 2, id(1))].session, 11);
    assert!(s.uploads.lock()[&(1, 2, id(1))].data.is_empty());
  }
  #[tokio::test]
  async fn receiver_relogin_drops_old_waiter_but_keeps_prefetch() {
    let s = service();
    s.begin(1, 2, id(1), 10, Some(20));
    assert!(s.disconnect(2, 20).is_empty());
    assert_eq!(s.uploads.lock()[&(1, 2, id(1))].waiter, None);
    assert!(s.begin(1, 2, id(1), 10, Some(21)) == Begin::Existing);
    assert_eq!(s.uploads.lock()[&(1, 2, id(1))].waiter, Some(21));
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
  #[tokio::test]
  async fn failed_database_write_keeps_waiter_for_session_cleanup() {
    let s = service(); // No schema: a valid complete upload will fail at the database write.
    let id = ImageId {
      size: 9,
      crc32: 0xcbf43926,
    }; // Standard IEEE CRC32 check vector.
    s.begin(1, 2, id, 10, Some(20));
    assert!(
      s.receive(
        1,
        2,
        10,
        ImageTransfer::Reply {
          id,
          first: true,
          filename: Some(b"image.png".to_vec()),
          data: b"123456789".to_vec()
        }
      )
      .await
      .is_err()
    );
    let events = s.disconnect(1, 10);
    assert!(matches!(
      events.as_slice(),
      [(
        2,
        ImageEvent::Unavailable {
          request_session: 20,
          ..
        }
      )]
    ));
    assert!(s.uploads.lock().is_empty());
  }
  #[tokio::test]
  async fn duplicate_start_and_cumulative_overflow_cancel_assembly() {
    let s = service();
    let id = ImageId { size: 2, crc32: 1 };
    for duplicate in [true, false] {
      s.begin(1, 2, id, 10, Some(20));
      let (ack, _) = s
        .receive(
          1,
          2,
          10,
          ImageTransfer::Reply {
            id,
            first: true,
            filename: Some(b"image.png".to_vec()),
            data: vec![1],
          },
        )
        .await
        .unwrap();
      assert_eq!(ack, AckStatus::Delivered);
      let (ack, event) = s
        .receive(
          1,
          2,
          10,
          ImageTransfer::Reply {
            id,
            first: duplicate,
            filename: duplicate.then(|| b"image.png".to_vec()),
            data: vec![2, 3],
          },
        )
        .await
        .unwrap();
      assert_eq!(ack, AckStatus::NotDelivered);
      assert!(matches!(
        event,
        Some((
          2,
          ImageEvent::Unavailable {
            request_session: 20,
            ..
          }
        ))
      ));
      assert!(s.uploads.lock().is_empty());
    }
  }
}
