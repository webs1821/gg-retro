//! Private image offers contain identifiers only, never image contents or filenames.
use super::{DatabasePool, RepositoryError};
use gg_protocol::packets::{ImageId, RichTextFormat};
use sqlx::{Sqlite, Transaction};

pub const MAX_IMAGES_PER_MESSAGE: usize = 4;
pub const MAX_STORED_IMAGES: i64 = 4096;
pub const MAX_USER_STORED_IMAGES: i64 = 512;

pub fn descriptors(formatting: Option<&[RichTextFormat]>) -> Vec<ImageId> {
  let mut ids = Vec::new();
  for id in formatting
    .unwrap_or_default()
    .iter()
    .filter_map(|f| f.image)
  {
    if !ids.contains(&id) {
      ids.push(id);
      // Keep one extra descriptor to reject the whole message; bound duplicate checks.
      if ids.len() > MAX_IMAGES_PER_MESSAGE {
        break;
      }
    }
  }
  ids
}

/// Store access grants in the same transaction as the text message.
pub async fn reserve(
  tx: &mut Transaction<'_, Sqlite>,
  message: i64,
  sender: u32,
  recipient: u32,
  ids: &[ImageId],
) -> Result<(), RepositoryError> {
  if ids.is_empty() {
    return Ok(());
  }
  if ids.len() > MAX_IMAGES_PER_MESSAGE || ids.iter().any(|id| !id.valid()) {
    return Err(RepositoryError::MailboxFull);
  }
  let (total,sent,received):(i64,i64,i64)=sqlx::query_as("SELECT count(*),count(CASE WHEN m.sender_uin=? THEN 1 END),count(CASE WHEN m.recipient_uin=? THEN 1 END) FROM message_images i JOIN messages m ON m.id=i.message_id")
    .bind(sender).bind(recipient).fetch_one(&mut **tx).await?;
  let count = ids.len() as i64;
  if total + count > MAX_STORED_IMAGES
    || sent + count > MAX_USER_STORED_IMAGES
    || received + count > MAX_USER_STORED_IMAGES
  {
    return Err(RepositoryError::MailboxFull);
  }
  for id in ids {
    sqlx::query("INSERT INTO message_images(message_id,size,crc32) VALUES (?,?,?)")
      .bind(message)
      .bind(id.size)
      .bind(id.crc32)
      .execute(&mut **tx)
      .await?;
  }
  Ok(())
}

pub async fn offered(
  pool: &DatabasePool,
  owner: u32,
  recipient: u32,
  id: ImageId,
) -> Result<bool, RepositoryError> {
  Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM message_images i JOIN messages m ON m.id=i.message_id WHERE m.sender_uin=? AND m.recipient_uin=? AND i.size=? AND i.crc32=?)")
    .bind(owner).bind(recipient).bind(id.size).bind(id.crc32).fetch_one(pool).await?)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::MessageRepository;
  use gg_protocol::{consts::GGMessageClass, packets::GGRecvMessage};
  async fn db() -> DatabasePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
      .max_connections(1)
      .connect("sqlite::memory:")
      .await
      .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
  }
  fn message(size: u32, crc32: u32) -> GGRecvMessage {
    GGRecvMessage {
      sender: 1,
      seq: 1,
      time: 0,
      class: GGMessageClass::Chat,
      message: "\u{a0}".into(),
      formatting: Some(vec![RichTextFormat {
        image: Some(ImageId { size, crc32 }),
        ..Default::default()
      }]),
    }
  }
  async fn fixture(pool: &DatabasePool) -> i64 {
    sqlx::query_scalar("INSERT INTO messages(recipient_uin,sender_uin,seq,time,class,message) VALUES (9,9,1,0,8,'fixture') RETURNING id").fetch_one(pool).await.unwrap()
  }
  #[tokio::test]
  async fn tiny_images_cannot_exhaust_metadata_storage() {
    let pool = db().await;
    let id = fixture(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    for crc in 0..MAX_STORED_IMAGES {
      sqlx::query("INSERT INTO message_images(message_id,size,crc32) VALUES (?,1,?)")
        .bind(id)
        .bind(crc)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    assert!(matches!(
      MessageRepository::new(&pool).store(2, &message(1, 3)).await,
      Err(RepositoryError::MailboxFull)
    ));
    assert_eq!(
      sqlx::query_scalar::<_, i64>("SELECT count(*) FROM messages")
        .fetch_one(&pool)
        .await
        .unwrap(),
      1
    );
  }
  #[tokio::test]
  async fn tiny_image_quota_is_also_per_sender_and_recipient() {
    let pool = db().await;
    let id = fixture(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    for crc in 0..MAX_USER_STORED_IMAGES {
      sqlx::query("INSERT INTO message_images(message_id,size,crc32) VALUES (?,1,?)")
        .bind(id)
        .bind(crc)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    let repo = MessageRepository::new(&pool);
    assert!(matches!(
      repo.store(9, &message(1, 5)).await,
      Err(RepositoryError::MailboxFull)
    ));
    let mut sent = message(1, 6);
    sent.sender = 9;
    assert!(matches!(
      repo.store(2, &sent).await,
      Err(RepositoryError::MailboxFull)
    ));
    repo.store(2, &message(1, 7)).await.unwrap();
    assert_eq!(
      sqlx::query_scalar::<_, i64>("SELECT count(*) FROM messages")
        .fetch_one(&pool)
        .await
        .unwrap(),
      2
    );
  }
  #[tokio::test]
  async fn grants_are_private_and_expire_with_the_message() {
    let pool = db().await;
    let repo = MessageRepository::new(&pool);
    let record = repo.store(2, &message(9, 0xcbf43926)).await.unwrap();
    let id = ImageId {
      size: 9,
      crc32: 0xcbf43926,
    };
    assert!(offered(&pool, 1, 2, id).await.unwrap());
    assert!(!offered(&pool, 1, 3, id).await.unwrap());
    assert!(!offered(&pool, 3, 2, id).await.unwrap());
    sqlx::query("UPDATE messages SET delivered_at=datetime('now','-2 hours') WHERE id=?")
      .bind(record.id)
      .execute(&pool)
      .await
      .unwrap();
    assert_eq!(repo.cleanup_old_delivered(60).await.unwrap(), 1);
    assert!(!offered(&pool, 1, 2, id).await.unwrap());
  }
  #[tokio::test]
  async fn migration_removes_experimental_cache_but_preserves_grants() {
    let pool = db().await;
    sqlx::raw_sql("DROP TABLE message_images; DROP INDEX messages_image_participants;")
      .execute(&pool)
      .await
      .unwrap();
    sqlx::raw_sql(include_str!("../../migrations/20261030020000_images.sql"))
      .execute(&pool)
      .await
      .unwrap();
    let parent = fixture(&pool).await;
    sqlx::query(
      "INSERT INTO message_images(message_id,size,crc32,filename,data) VALUES (?,9,3421780262,?,?)",
    )
    .bind(parent)
    .bind(b"old.png".as_slice())
    .bind(b"123456789".as_slice())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
      "../../migrations/20261030030000_classicImages.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    assert!(
      offered(
        &pool,
        9,
        9,
        ImageId {
          size: 9,
          crc32: 0xcbf43926
        }
      )
      .await
      .unwrap()
    );
    assert!(
      sqlx::query("SELECT data,filename FROM message_images")
        .fetch_all(&pool)
        .await
        .is_err()
    );
    assert_eq!(
      sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await
        .unwrap(),
      "ok"
    );
  }
}
