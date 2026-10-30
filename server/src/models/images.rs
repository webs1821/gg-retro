//! Access is scoped to the sender and recipient of an accepted message.
use super::{DatabasePool, RepositoryError};
use gg_protocol::packets::{ImageId, RichTextFormat};
use sqlx::{Sqlite, Transaction};

pub const MAX_IMAGES_PER_MESSAGE: usize = 4;
pub const MAX_RESERVED_BYTES: i64 = 64 * 1024 * 1024;
pub const MAX_USER_RESERVED_BYTES: i64 = 16 * 1024 * 1024;
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
    }
  }
  ids
}

/// Called inside the message INSERT transaction; rejected attachments roll back the text too.
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
  let wanted: i64 = ids.iter().map(|id| i64::from(id.size)).sum();
  let (count, total, sent, received, sent_count, received_count): (i64,i64,i64,i64,i64,i64) = sqlx::query_as(
    "SELECT count(*),coalesce(sum(i.size),0),coalesce(sum(CASE WHEN m.sender_uin=? THEN i.size ELSE 0 END),0),coalesce(sum(CASE WHEN m.recipient_uin=? THEN i.size ELSE 0 END),0),count(CASE WHEN m.sender_uin=? THEN 1 END),count(CASE WHEN m.recipient_uin=? THEN 1 END) FROM message_images i JOIN messages m ON m.id=i.message_id")
    .bind(sender).bind(recipient).bind(sender).bind(recipient).fetch_one(&mut **tx).await?;
  if count + ids.len() as i64 > MAX_STORED_IMAGES
    || total + wanted > MAX_RESERVED_BYTES
    || sent + wanted > MAX_USER_RESERVED_BYTES
    || received + wanted > MAX_USER_RESERVED_BYTES
    || sent_count + ids.len() as i64 > MAX_USER_STORED_IMAGES
    || received_count + ids.len() as i64 > MAX_USER_STORED_IMAGES
  {
    return Err(RepositoryError::MailboxFull);
  }
  for id in ids {
    let cached: Option<(Vec<u8>,Vec<u8>)> = sqlx::query_as("SELECT i.filename,i.data FROM message_images i JOIN messages m ON m.id=i.message_id WHERE m.sender_uin=? AND m.recipient_uin=? AND i.size=? AND i.crc32=? AND i.data IS NOT NULL LIMIT 1")
      .bind(sender).bind(recipient).bind(id.size).bind(id.crc32).fetch_optional(&mut **tx).await?;
    let (filename, data) = cached
      .map(|(name, data)| (Some(name), Some(data)))
      .unwrap_or_default();
    sqlx::query(
      "INSERT INTO message_images(message_id,size,crc32,filename,data) VALUES (?,?,?,?,?)",
    )
    .bind(message)
    .bind(id.size)
    .bind(id.crc32)
    .bind(filename)
    .bind(data)
    .execute(&mut **tx)
    .await?;
  }
  Ok(())
}

pub struct CachedImage {
  pub filename: Vec<u8>,
  pub data: Vec<u8>,
}
pub enum OfferedImage {
  Missing,
  Pending,
  Ready(CachedImage),
}
pub async fn lookup(
  pool: &DatabasePool,
  owner: u32,
  recipient: u32,
  id: ImageId,
) -> Result<OfferedImage, RepositoryError> {
  let row: Option<(Option<Vec<u8>>,Option<Vec<u8>>)> = sqlx::query_as(
    "SELECT i.filename,i.data FROM message_images i JOIN messages m ON m.id=i.message_id WHERE m.sender_uin=? AND m.recipient_uin=? AND i.size=? AND i.crc32=? ORDER BY i.data IS NULL,m.id DESC LIMIT 1")
    .bind(owner).bind(recipient).bind(id.size).bind(id.crc32).fetch_optional(pool).await?;
  Ok(match row {
    None => OfferedImage::Missing,
    Some((Some(filename), Some(data))) => OfferedImage::Ready(CachedImage { filename, data }),
    _ => OfferedImage::Pending,
  })
}
pub async fn store(
  pool: &DatabasePool,
  owner: u32,
  recipient: u32,
  id: ImageId,
  filename: &[u8],
  data: &[u8],
) -> Result<bool, RepositoryError> {
  let result=sqlx::query("UPDATE message_images SET filename=?,data=? WHERE size=? AND crc32=? AND data IS NULL AND message_id IN (SELECT id FROM messages WHERE sender_uin=? AND recipient_uin=?)")
    .bind(filename).bind(data).bind(id.size).bind(id.crc32).bind(owner).bind(recipient).execute(pool).await?;
  if result.rows_affected() > 0 {
    return Ok(true);
  }
  // A previous source session can finish its write while a replacement starts the same image.
  // An existing complete copy is success too; a removed message still revokes access.
  Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM message_images i JOIN messages m ON m.id=i.message_id WHERE m.sender_uin=? AND m.recipient_uin=? AND i.size=? AND i.crc32=? AND i.data IS NOT NULL)")
    .bind(owner).bind(recipient).bind(id.size).bind(id.crc32).fetch_one(pool).await?)
}
/// Reconsider unfinished offers after the sender logs in again, without loading any blobs.
pub async fn pending(
  pool: &DatabasePool,
  owner: u32,
) -> Result<Vec<(u32, ImageId)>, RepositoryError> {
  let rows:Vec<(u32,u32,u32)>=sqlx::query_as("SELECT DISTINCT m.recipient_uin,i.size,i.crc32 FROM message_images i JOIN messages m ON m.id=i.message_id WHERE m.sender_uin=? AND i.data IS NULL AND i.last_attempt<unixepoch()-300 ORDER BY i.last_attempt,m.id LIMIT 32")
    .bind(owner).fetch_all(pool).await?;
  Ok(
    rows
      .into_iter()
      .map(|(recipient, size, crc32)| (recipient, ImageId { size, crc32 }))
      .collect(),
  )
}

pub async fn attempted(
  pool: &DatabasePool,
  owner: u32,
  recipient: u32,
  id: ImageId,
) -> Result<(), RepositoryError> {
  sqlx::query("UPDATE message_images SET last_attempt=unixepoch() WHERE size=? AND crc32=? AND message_id IN (SELECT id FROM messages WHERE sender_uin=? AND recipient_uin=?)")
    .bind(id.size).bind(id.crc32).bind(owner).bind(recipient).execute(pool).await?;
  Ok(())
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
  async fn global_byte_reservation_is_bounded_and_rejection_rolls_back_text() {
    let pool = db().await;
    let id = fixture(&pool).await;
    let mut remaining = MAX_RESERVED_BYTES - 1000;
    let mut crc = 0;
    let mut tx = pool.begin().await.unwrap();
    while remaining > 0 {
      let size = remaining.min(261120);
      crc += 1;
      sqlx::query("INSERT INTO message_images(message_id,size,crc32) VALUES (?,?,?)")
        .bind(id)
        .bind(size)
        .bind(crc)
        .execute(&mut *tx)
        .await
        .unwrap();
      remaining -= size;
    }
    tx.commit().await.unwrap();
    let repo = MessageRepository::new(&pool);
    repo.store(2, &message(1000, 2)).await.unwrap();
    assert!(matches!(
      repo.store(2, &message(1, 3)).await,
      Err(RepositoryError::MailboxFull)
    ));
    assert_eq!(
      sqlx::query_scalar::<_, i64>("SELECT count(*) FROM messages")
        .fetch_one(&pool)
        .await
        .unwrap(),
      2
    );
    assert_eq!(
      sqlx::query_scalar::<_, i64>("SELECT sum(size) FROM message_images")
        .fetch_one(&pool)
        .await
        .unwrap(),
      MAX_RESERVED_BYTES
    );
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
  async fn retention_removes_image_and_revokes_access_with_the_message() {
    let pool = db().await;
    let repo = MessageRepository::new(&pool);
    let record = repo.store(2, &message(9, 0xcbf43926)).await.unwrap();
    assert!(
      store(
        &pool,
        1,
        2,
        ImageId {
          size: 9,
          crc32: 0xcbf43926
        },
        b"image.png",
        b"123456789"
      )
      .await
      .unwrap()
    );
    assert!(matches!(
      lookup(
        &pool,
        1,
        2,
        ImageId {
          size: 9,
          crc32: 0xcbf43926
        }
      )
      .await
      .unwrap(),
      OfferedImage::Ready(_)
    ));
    // Idempotent completion after a session takeover must not report a cached image as missing.
    let id = ImageId {
      size: 9,
      crc32: 0xcbf43926,
    };
    assert!(
      store(&pool, 1, 2, id, b"image.png", b"123456789")
        .await
        .unwrap()
    );
    assert!(
      !store(&pool, 1, 3, id, b"image.png", b"123456789")
        .await
        .unwrap()
    );
    sqlx::query("UPDATE messages SET delivered_at=datetime('now','-2 hours') WHERE id=?")
      .bind(record.id)
      .execute(&pool)
      .await
      .unwrap();
    assert_eq!(repo.cleanup_old_delivered(60).await.unwrap(), 1);
    assert_eq!(
      sqlx::query_scalar::<_, i64>("SELECT count(*) FROM message_images")
        .fetch_one(&pool)
        .await
        .unwrap(),
      0
    );
    assert!(matches!(
      lookup(
        &pool,
        1,
        2,
        ImageId {
          size: 9,
          crc32: 0xcbf43926
        }
      )
      .await
      .unwrap(),
      OfferedImage::Missing
    ));
  }
}
