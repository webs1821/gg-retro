//! An offline description and its visibility rules are saved in one atomic row.
use crate::{
  messenger::{PresenceHub, UserPresence, contact_book::ContactBook},
  models::DatabasePool,
};
use gg_protocol::{
  consts::GGStatus,
  packets::{ContactEntry, ContactType},
};

pub async fn clear(pool: &DatabasePool, uin: u32) -> Result<(), sqlx::Error> {
  sqlx::query("DELETE FROM offline_presence WHERE uin=?")
    .bind(uin)
    .execute(pool)
    .await?;
  Ok(())
}
pub async fn save(
  pool: &DatabasePool,
  presence: &UserPresence,
  book: &ContactBook,
) -> Result<(), sqlx::Error> {
  let mut privacy = Vec::new();
  for entry in book.entries() {
    privacy.extend_from_slice(&entry.uin.to_le_bytes());
    privacy.push(entry.user_type.bits());
  }
  sqlx::query("INSERT INTO offline_presence(uin,status,description,return_time,privacy) VALUES (?,?,?,?,?) ON CONFLICT(uin) DO UPDATE SET status=excluded.status,description=excluded.description,return_time=excluded.return_time,privacy=excluded.privacy")
    .bind(presence.uin).bind(presence.status.bits()).bind(&presence.description).bind(presence.time).bind(privacy).execute(pool).await?;
  Ok(())
}
pub async fn restore(pool: &DatabasePool, hub: &PresenceHub) -> Result<(), sqlx::Error> {
  let rows: Vec<(u32, u32, String, Option<u32>, Vec<u8>)> =
    sqlx::query_as("SELECT uin,status,description,return_time,privacy FROM offline_presence")
      .fetch_all(pool)
      .await?;
  for (uin, status, description, time, privacy) in rows {
    let mut entries = Vec::new();
    for entry in privacy.chunks_exact(5) {
      entries.push(ContactEntry {
        uin: u32::from_le_bytes(entry[..4].try_into().unwrap()),
        user_type: ContactType::try_from(entry[4])
          .map_err(|_| sqlx::Error::Decode("Invalid offline privacy flags".into()))?,
      });
    }
    let mut book = ContactBook::new();
    book.set(&entries);
    let status = GGStatus::try_from(status)
      .map_err(|_| sqlx::Error::Decode("Invalid offline status".into()))?;
    hub.restore_offline(
      UserPresence {
        uin,
        status,
        description: Some(description),
        time,
        image_size: 0,
      },
      book,
    );
  }
  Ok(())
}
