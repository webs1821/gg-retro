use super::contact_book::ContactBook;
use gg_protocol::consts::{GGStatus, version::GG_VERSION_60};
use gg_protocol::packets::ContactStatus;
use gg_protocol::{GGLogin60, GGNumber};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::instrument;

#[derive(Debug, Clone, Default)]
pub struct UserPresence {
  pub uin: GGNumber,
  pub status: GGStatus,
  /// Status description (optional, max 70 characters).
  pub description: Option<String>,
  /// Return time (optional).
  pub time: Option<u32>,
}

impl UserPresence {
  pub fn offline(uin: GGNumber) -> Self {
    Self {
      uin,
      status: GGStatus::NotAvail,
      ..Default::default()
    }
  }

  pub fn available(uin: GGNumber) -> Self {
    Self {
      uin,
      status: GGStatus::Avail,
      ..Default::default()
    }
  }
}

impl From<GGLogin60> for UserPresence {
  fn from(value: GGLogin60) -> Self {
    Self {
      uin: value.uin,
      status: value.status,
      time: value.time,
      description: value.description,
    }
  }
}

impl From<UserPresence> for ContactStatus {
  fn from(mut presence: UserPresence) -> Self {
    if matches!(
      presence.status.base(),
      GGStatus::Invisible | GGStatus::InvisibleDescr
    ) {
      presence = UserPresence::offline(presence.uin);
    }
    ContactStatus {
      uin: presence.uin,
      flags: 0, // 0x00 = no special flags (0x40 would be voice support)
      status: presence.status.base().bits() as u8,
      remote_ip: Ipv4Addr::new(0, 0, 0, 0), // No direct connection info
      remote_port: 0,
      version: GG_VERSION_60 as u8,
      image_size: 0, // No image support
      description: presence.description,
      time: presence.time,
    }
  }
}

pub type PresenceChangeStream = ReceiverStream<GGNumber>;

pub struct PresenceHub {
  state: RwLock<HashMap<GGNumber, UserPresence>>,
  books: RwLock<HashMap<GGNumber, ContactBook>>,
  observers: RwLock<HashMap<GGNumber, HashSet<GGNumber>>>,
  sessions: RwLock<HashMap<GGNumber, mpsc::Sender<GGNumber>>>,
}

impl PresenceHub {
  pub fn new() -> Self {
    Self {
      state: RwLock::new(HashMap::new()),
      books: RwLock::new(HashMap::new()),
      observers: RwLock::new(HashMap::new()),
      sessions: RwLock::new(HashMap::new()),
    }
  }

  pub fn online(&self) -> usize {
    self
      .state
      .read()
      .values()
      .filter(|presence| {
        !presence.status.friends_only()
          && matches!(
            presence.status.base(),
            GGStatus::Avail | GGStatus::AvailDescr | GGStatus::Busy | GGStatus::BusyDescr
          )
      })
      .count()
  }

  #[instrument(skip_all)]
  pub fn find(&self, uin: &GGNumber) -> UserPresence {
    self
      .state
      .read()
      .get(uin)
      .cloned()
      .unwrap_or_else(|| UserPresence::offline(*uin))
  }

  #[instrument(skip_all)]
  pub fn register(&self, uin: GGNumber) -> PresenceChangeStream {
    let mut sessions = self.sessions.write();
    let mut state = self.state.write();
    state.insert(uin, UserPresence::offline(uin));
    let (tx, rx) = mpsc::channel::<GGNumber>(1024);
    sessions.remove(&uin);
    sessions.insert(uin, tx);

    ReceiverStream::new(rx)
  }

  #[instrument(skip_all)]
  pub fn subscribe(&self, uin: GGNumber, watched: &[GGNumber]) {
    let mut observers = self.observers.write();

    for &watched_uid in watched {
      observers.entry(watched_uid).or_default().insert(uin);
    }
  }

  #[instrument(skip_all)]
  pub fn unsubscribe(&self, uin: GGNumber, watched: &[GGNumber]) {
    let mut observers = self.observers.write();

    for &watched_uid in watched {
      if let Some(set) = observers.get_mut(&watched_uid) {
        set.remove(&uin);
      }
    }
  }

  #[instrument(skip_all)]
  pub fn notify(&self, presence: UserPresence) {
    let observers = self.observers.read();
    let sessions = self.sessions.read();
    let uin = presence.uin;
    self.state.write().insert(uin, presence);

    if let Some(watchers) = observers.get(&uin) {
      for watcher in watchers {
        if let Some(channel) = sessions.get(watcher) {
          let _ = channel.try_send(uin);
        }
      }
    }
  }

  #[instrument(skip_all)]
  pub fn refresh(&self, uin: GGNumber) {
    self.notify(self.find(&uin))
  }

  pub fn set_book(&self, uin: GGNumber, book: &ContactBook) {
    self.unsubscribe_all(uin);
    self.subscribe(uin, &book.watched());
    self.books.write().insert(uin, book.clone());
    self.refresh(uin);
  }

  pub fn find_for(&self, uin: GGNumber, viewer: GGNumber) -> UserPresence {
    let presence = self.find(&uin);
    let books = self.books.read();
    let book = books.get(&uin);
    if book.is_some_and(|b| b.is_hidden_from(viewer))
      || (presence.status.friends_only() && !book.is_some_and(|b| b.is_friend(viewer)))
    {
      UserPresence::offline(uin)
    } else {
      presence
    }
  }

  fn unsubscribe_all(&self, uin: GGNumber) {
    self.observers.write().retain(|_, watchers| {
      watchers.remove(&uin);
      !watchers.is_empty()
    });
  }

  pub fn unregister(&self, uin: GGNumber, _watched: &[GGNumber]) {
    self.notify(UserPresence::offline(uin));
    self.sessions.write().remove(&uin);
    self.unsubscribe_all(uin);
    self.books.write().remove(&uin);
    self.state.write().remove(&uin);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;
  use tokio_stream::StreamExt;

  fn presence(uin: GGNumber) -> UserPresence {
    UserPresence {
      uin,
      status: GGStatus::Avail,
      description: None,
      time: None,
    }
  }

  async fn recv(stream: &mut PresenceChangeStream) -> Option<GGNumber> {
    tokio::time::timeout(Duration::from_millis(100), stream.next())
      .await
      .ok()
      .flatten()
  }

  #[tokio::test]
  async fn test_subscriber_receives_presence_updates() {
    let hub = PresenceHub::new();

    let mut rx = hub.register(1000);
    hub.subscribe(1000, &[5000]);

    hub.notify(presence(5000));

    assert_eq!(recv(&mut rx).await, Some(5000));
  }

  #[tokio::test]
  async fn test_multiple_subscribers_receive_same_update() {
    let hub = PresenceHub::new();

    let mut rx1 = hub.register(1000);
    let mut rx2 = hub.register(2000);
    hub.subscribe(1000, &[5000]);
    hub.subscribe(2000, &[5000]);

    hub.notify(presence(5000));

    assert_eq!(recv(&mut rx1).await, Some(5000));
    assert_eq!(recv(&mut rx2).await, Some(5000));
  }

  #[tokio::test]
  async fn test_unsubscribed_user_does_not_receive_updates() {
    let hub = PresenceHub::new();

    let mut rx1 = hub.register(1000);
    let mut rx2 = hub.register(2000);
    hub.subscribe(1000, &[5000]);
    hub.subscribe(2000, &[6000]); // watches different user

    hub.notify(presence(5000));

    assert!(recv(&mut rx1).await.is_some());
    assert!(recv(&mut rx2).await.is_none());
  }

  #[tokio::test]
  async fn test_unregister_stops_receiving_updates() {
    let hub = PresenceHub::new();

    let mut rx = hub.register(1000);
    hub.subscribe(1000, &[5000]);

    hub.notify(presence(5000));
    assert!(recv(&mut rx).await.is_some());

    hub.unregister(1000, &[5000]);

    hub.notify(presence(5000));
    assert!(recv(&mut rx).await.is_none());
  }

  #[tokio::test]
  async fn test_reregister_receives_updates_on_new_channel() {
    let hub = PresenceHub::new();

    let mut old_rx = hub.register(1000);
    hub.subscribe(1000, &[5000]);

    let mut new_rx = hub.register(1000); // re-register
    hub.subscribe(1000, &[5000]);

    hub.notify(presence(5000));

    assert!(recv(&mut old_rx).await.is_none()); // old channel closed
    assert_eq!(recv(&mut new_rx).await, Some(5000));
  }

  #[tokio::test]
  async fn test_bidirectional_watching() {
    let hub = PresenceHub::new();

    let mut rx1 = hub.register(1000);
    let mut rx2 = hub.register(2000);
    hub.subscribe(1000, &[2000]);
    hub.subscribe(2000, &[1000]);

    hub.notify(presence(1000));
    hub.notify(presence(2000));

    assert_eq!(recv(&mut rx1).await, Some(2000));
    assert_eq!(recv(&mut rx2).await, Some(1000));
  }

  #[tokio::test]
  async fn test_unsubscribe_stops_updates() {
    let hub = PresenceHub::new();

    let mut rx = hub.register(1000);
    hub.subscribe(1000, &[5000, 6000]);

    hub.notify(presence(5000));
    assert_eq!(recv(&mut rx).await, Some(5000));

    hub.unsubscribe(1000, &[5000]);

    hub.notify(presence(5000));
    hub.notify(presence(6000));

    assert_eq!(recv(&mut rx).await, Some(6000));
    assert!(recv(&mut rx).await.is_none());
  }
}
