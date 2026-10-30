//! Offline message model and repository.

use crate::models::RepositoryError;
use gg_protocol::GGNumber;
use gg_protocol::consts::GGMessageClass;
use gg_protocol::packets::{GGRecvMessage, RichTextFormats};
use sqlx::{FromRow, Pool, Sqlite};
use tracing::instrument;

pub type QueuedMessageId = i64;

/// Offline message record from database.
#[derive(Debug, Clone, FromRow)]
pub struct QueuedMessage {
  /// Database ID.
  pub id: QueuedMessageId,
  /// Recipient user number.
  pub recipient_uin: u32,
  /// Sender user number.
  pub sender_uin: u32,
  /// Chat group ID.
  pub seq: u32,
  /// Unix timestamp when message was sent.
  pub time: u32,
  /// Message class (stored as integer).
  pub class: u32,
  /// Message content.
  pub message: String,
  /// Rich text formatting (raw protocol bytes, NULL if none).
  pub formatting: Option<Vec<u8>>,
  /// When the message was stored.
  pub created_at: Option<String>,
  /// When the message was delivered (NULL if pending).
  pub delivered_at: Option<String>,
}

impl Into<GGRecvMessage> for QueuedMessage {
  fn into(self) -> GGRecvMessage {
    // Deserialize formatting from raw bytes using TryFrom
    let formatting = self
      .formatting
      .as_deref()
      .and_then(|data| RichTextFormats::try_from(data).ok())
      .map(|f| f.0)
      .filter(|f| !f.is_empty());

    GGRecvMessage {
      sender: self.sender_uin,
      seq: self.seq,
      time: self.time,
      class: GGMessageClass::try_from(self.class).unwrap_or_default(),
      message: self.message,
      formatting,
    }
  }
}

/// Repository for offline message database operations.
#[derive(Clone)]
pub struct MessageRepository {
  pool: Pool<Sqlite>,
}

impl std::fmt::Debug for MessageRepository {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("MessageRepository").finish()
  }
}

impl MessageRepository {
  /// Create a new repository with the given database pool.
  pub fn new(pool: &Pool<Sqlite>) -> Self {
    Self { pool: pool.clone() }
  }

  /// Store an offline message for a recipient.
  #[instrument(skip(self, msg))]
  pub async fn store(
    &self,
    recipient: GGNumber,
    msg: &GGRecvMessage,
  ) -> Result<QueuedMessage, RepositoryError> {
    // Serialize formatting to raw protocol bytes using Into
    let formatting_bytes: Option<Vec<u8>> = msg
      .formatting
      .as_ref()
      .filter(|f| !f.is_empty())
      .map(|f| Vec::<u8>::from(RichTextFormats::from(f.as_slice())));

    let mut tx = self.pool.begin().await?;
    let result = sqlx::query_as::<_, QueuedMessage>(
      "INSERT INTO messages (recipient_uin, sender_uin, seq, time, class, message, formatting) \
       SELECT ?, ?, ?, ?, ?, ?, ? WHERE (SELECT count(*) FROM messages WHERE recipient_uin = ? AND delivered_at IS NULL) < 1000 AND (SELECT count(*) FROM messages) < 50000 RETURNING *"
    )
      .bind(recipient)
      .bind(msg.sender)
      .bind(msg.seq)
      .bind(msg.time)
      .bind(msg.class.bits())
      .bind(&msg.message)
      .bind(&formatting_bytes)
      .bind(recipient)
      .fetch_optional(&mut *tx)
      .await?
      .ok_or(RepositoryError::MailboxFull)?;
    let ids = super::images::descriptors(msg.formatting.as_deref());
    super::images::reserve(&mut tx, result.id, msg.sender, recipient, &ids).await?;
    tx.commit().await?;

    tracing::debug!(
      id = result.id,
      recipient = recipient,
      sender = msg.sender,
      has_formatting = formatting_bytes.is_some(),
      "Offline message stored"
    );
    Ok(result)
  }

  /// Find all pending (undelivered) messages for a recipient.
  #[instrument(skip(self))]
  pub async fn find_pending(
    &self,
    recipient: GGNumber,
  ) -> Result<Option<Vec<QueuedMessage>>, RepositoryError> {
    let messages = sqlx::query_as::<_, QueuedMessage>(
      "SELECT * FROM messages \
       WHERE recipient_uin = ? AND delivered_at IS NULL \
       ORDER BY id ASC \
       LIMIT 100",
    )
    .bind(recipient)
    .fetch_all(&self.pool)
    .await?;

    tracing::debug!(
      count = messages.len(),
      recipient = recipient,
      "Found pending messages"
    );

    if messages.len() > 0 {
      Ok(Some(messages))
    } else {
      Ok(None)
    }
  }

  /// Find a single pending message for a recipient (oldest first).
  #[instrument(skip(self))]
  pub async fn find_one_pending(
    &self,
    msg_id: QueuedMessageId,
  ) -> Result<Option<QueuedMessage>, RepositoryError> {
    let message = sqlx::query_as::<_, QueuedMessage>(
      "SELECT * FROM messages \
       WHERE id = ? AND delivered_at IS NULL \
       ORDER BY id ASC \
       LIMIT 1",
    )
    .bind(msg_id)
    .fetch_optional(&self.pool)
    .await?;

    Ok(message)
  }

  /// Mark messages as delivered by setting delivered_at timestamp.
  #[instrument(skip(self))]
  pub async fn mark_delivered(&self, ids: &[QueuedMessageId]) -> Result<(), RepositoryError> {
    if ids.is_empty() {
      return Ok(());
    }

    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");

    let query = format!(
      "UPDATE messages SET delivered_at = CURRENT_TIMESTAMP \
       WHERE id IN ({}) AND delivered_at IS NULL",
      placeholders
    );

    let mut query_builder = sqlx::query(&query);
    for &id in ids {
      query_builder = query_builder.bind(id);
    }

    let result = query_builder.execute(&self.pool).await?;

    tracing::debug!(
      count = result.rows_affected(),
      "Messages marked as delivered"
    );
    Ok(())
  }

  /// Mark a single message as delivered by ID.
  #[instrument(skip(self))]
  pub async fn mark_single_delivered(&self, id: QueuedMessageId) -> Result<(), RepositoryError> {
    let result = sqlx::query(
      "UPDATE messages SET delivered_at = CURRENT_TIMESTAMP \
       WHERE id = ? AND delivered_at IS NULL",
    )
    .bind(id)
    .execute(&self.pool)
    .await?;

    tracing::debug!(
      id = id,
      rows_affected = result.rows_affected(),
      "Message marked as delivered"
    );

    Ok(())
  }

  /// Delete messages that were delivered more than `minutes` ago.
  /// Returns the number of deleted messages.
  #[instrument(skip(self))]
  pub async fn cleanup_old_delivered(&self, minutes: i64) -> Result<u64, RepositoryError> {
    let result = sqlx::query(
      "DELETE FROM messages \
       WHERE delivered_at IS NOT NULL \
       AND datetime(delivered_at, '+' || ? || ' minutes') < datetime('now')",
    )
    .bind(minutes)
    .execute(&self.pool)
    .await?;

    let deleted = result.rows_affected();
    if deleted > 0 {
      tracing::debug!(count = deleted, "Old delivered messages cleaned up");
    }
    Ok(deleted)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use sqlx::sqlite::SqlitePoolOptions;

  async fn setup_test_db() -> Result<Pool<Sqlite>, RepositoryError> {
    let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await?;

    sqlx::migrate!("./migrations")
      .run(&pool)
      .await
      .expect("migrations failed");

    Ok(pool)
  }

  fn create_test_message(sender: u32, message: &str) -> GGRecvMessage {
    GGRecvMessage {
      sender,
      seq: 1,
      time: 1234567890,
      class: GGMessageClass::Chat,
      message: message.to_string(),
      formatting: None,
    }
  }

  #[tokio::test]
  async fn test_store_and_find_pending() {
    let pool = setup_test_db().await.unwrap();
    let repo = MessageRepository::new(&pool);

    let msg = create_test_message(12345, "Hello offline!");
    let stored = repo.store(67890, &msg).await.unwrap();

    assert_eq!(stored.sender_uin, 12345);
    assert_eq!(stored.recipient_uin, 67890);
    assert!(stored.delivered_at.is_none());

    let pending = repo.find_pending(67890).await.unwrap().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message, "Hello offline!");
  }

  #[tokio::test]
  async fn test_mark_delivered() {
    let pool = setup_test_db().await.unwrap();
    let repo = MessageRepository::new(&pool);

    let msg = create_test_message(12345, "Test message");
    let stored = repo.store(67890, &msg).await.unwrap();

    repo.mark_delivered(&[stored.id]).await.unwrap();

    let pending = repo.find_pending(67890).await.unwrap();
    assert!(pending.is_none() || pending.unwrap().is_empty());
  }

  #[tokio::test]
  async fn test_into_recv_message() {
    let pool = setup_test_db().await.unwrap();
    let repo = MessageRepository::new(&pool);

    let msg = create_test_message(12345, "Queued message");
    let stored = repo.store(67890, &msg).await.unwrap();

    let recv_msg: GGRecvMessage = stored.into();
    assert_eq!(recv_msg.sender, 12345);
    assert_eq!(recv_msg.class, GGMessageClass::Chat);
    assert_eq!(recv_msg.message, "Queued message");
  }

  #[tokio::test]
  async fn test_find_pending_empty() {
    let pool = setup_test_db().await.unwrap();
    let repo = MessageRepository::new(&pool);

    let pending = repo.find_pending(99999).await.unwrap();
    assert!(pending.is_none() || pending.unwrap().is_empty());
  }

  #[tokio::test]
  async fn test_mark_delivered_empty_ids() {
    let pool = setup_test_db().await.unwrap();
    let repo = MessageRepository::new(&pool);

    // Should not error on empty slice
    repo.mark_delivered(&[]).await.unwrap();
  }

  #[tokio::test]
  async fn test_mark_single_delivered() {
    let pool = setup_test_db().await.unwrap();
    let repo = MessageRepository::new(&pool);

    let msg = create_test_message(12345, "Test single message");
    let stored = repo.store(67890, &msg).await.unwrap();

    repo.mark_single_delivered(stored.id).await.unwrap();

    let pending = repo.find_pending(67890).await.unwrap();
    assert!(pending.is_none());
  }

  #[tokio::test]
  async fn test_find_one_pending() {
    let pool = setup_test_db().await.unwrap();
    let repo = MessageRepository::new(&pool);

    // Store a message
    let msg1 = create_test_message(111, "First");
    let stored1 = repo.store(67890, &msg1).await.unwrap();

    // find_one_pending should find by message id
    let one = repo.find_one_pending(stored1.id).await.unwrap();
    assert!(one.is_some());
    assert_eq!(one.unwrap().id, stored1.id);
  }

  #[tokio::test]
  async fn test_find_one_pending_empty() {
    let pool = setup_test_db().await.unwrap();
    let repo = MessageRepository::new(&pool);

    let one = repo.find_one_pending(99999).await.unwrap();
    assert!(one.is_none());
  }
}
