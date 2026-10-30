use crate::models::{DatabasePool, MessageRepository, RepositoryError, UserRepository};
use gg_protocol::{
  GGNumber,
  consts::{AckStatus, GGMessageClass},
  packets::{GGRecvMessage, GGSendMessage},
};
use parking_lot::Mutex as MapLock;
use std::{
  collections::HashMap,
  sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
  },
};
use tokio::sync::{Mutex, OwnedMutexGuard, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

pub type MessagesStream = ReceiverStream<()>;
struct Registration {
  id: u64,
  notify: mpsc::Sender<()>,
  stop: CancellationToken,
  delivery: Arc<Mutex<()>>,
}
pub struct MessageDispatcher {
  sessions: MapLock<HashMap<GGNumber, Registration>>,
  next_id: AtomicU64,
  db_pool: DatabasePool,
}
impl MessageDispatcher {
  pub fn new(db_pool: &DatabasePool) -> Self {
    Self {
      sessions: MapLock::new(HashMap::new()),
      next_id: AtomicU64::new(1),
      db_pool: db_pool.clone(),
    }
  }
  /// Cancel the old reader and serialize delivery/cleanup for this account.
  /// No registry lock is held across an await or socket write.
  pub async fn register(
    &self,
    uin: GGNumber,
  ) -> (u64, MessagesStream, CancellationToken, OwnedMutexGuard<()>) {
    let id = self.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel(1);
    let stop = CancellationToken::new();
    let delivery = {
      let mut sessions = self.sessions.lock();
      let delivery = if let Some(old) = sessions.get(&uin) {
        old.stop.cancel();
        old.delivery.clone()
      } else {
        Arc::new(Mutex::new(()))
      };
      sessions.insert(
        uin,
        Registration {
          id,
          notify: tx,
          stop: stop.clone(),
          delivery: delivery.clone(),
        },
      );
      delivery
    };
    let guard = delivery.lock_owned().await;
    (id, ReceiverStream::new(rx), stop, guard)
  }
  pub fn unregister(&self, uin: GGNumber, id: u64) {
    let mut sessions = self.sessions.lock();
    if sessions.get(&uin).is_some_and(|s| s.id == id) {
      sessions.remove(&uin);
    }
  }
  pub async fn dispatch(
    &self,
    sender: GGNumber,
    incoming: GGSendMessage,
  ) -> Result<AckStatus, RepositoryError> {
    let users = UserRepository::new(&self.db_pool);
    if incoming.message.len() > 8192 || !users.exists(incoming.recipient).await? {
      return Ok(AckStatus::NotDelivered);
    }
    let online = self.sessions.lock().contains_key(&incoming.recipient);
    let class = GGMessageClass::try_from((incoming.class.bits() & !1) | u32::from(!online))
      .unwrap_or_default();
    let message = GGRecvMessage {
      sender,
      seq: incoming.seq,
      class,
      message: incoming.message,
      formatting: incoming.formatting,
      time: std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as u32,
    };
    match MessageRepository::new(&self.db_pool)
      .store(incoming.recipient, &message)
      .await
    {
      Err(RepositoryError::MailboxFull) => return Ok(AckStatus::MboxFull),
      Err(error) => return Err(error),
      Ok(_) => {}
    }
    // The durable database is authoritative; a coalesced/lost wake-up is recovered by polling.
    let sessions = self.sessions.lock();
    if let Some(session) = sessions.get(&incoming.recipient) {
      let _ = session.notify.try_send(());
    }
    // GG6 has no recipient acknowledgement: promise durable acceptance, never receipt/read.
    Ok(AckStatus::Queued)
  }
}
