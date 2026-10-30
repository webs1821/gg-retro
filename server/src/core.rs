use crate::{
  messenger::{MessageDispatcher, PresenceHub},
  models::DatabasePool,
  security::{Admission, PasswordVault},
};
use axum::{http, http::Uri};
use figment::{
  Figment,
  providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::{net::IpAddr, sync::Arc, time::Duration};
use zeroize::Zeroizing;

#[derive(Deserialize, Serialize, Debug)]
pub struct ServerConfig {
  bind: String,
  http_port: u16,
  gg_port: u16,
  db: String,
  hostname: String,
  client_downloads_dir: String,
  pub advertised_ip: Option<IpAddr>,
  pub advertised_port: u16,
  public_scheme: String,
  password_key_file: String,
  registration_invite_file: String,
  migrate_plaintext_passwords: bool,
}
impl Default for ServerConfig {
  fn default() -> Self {
    Self {
      bind: "127.0.0.1".into(),
      http_port: 8080,
      gg_port: 8074,
      db: "./gg.db".into(),
      hostname: "gg-retro.local".into(),
      client_downloads_dir: String::new(),
      advertised_ip: None,
      advertised_port: 8074,
      public_scheme: "https".into(),
      password_key_file: "./secrets/password.key".into(),
      registration_invite_file: "./secrets/registration-invite.txt".into(),
      migrate_plaintext_passwords: false,
    }
  }
}
impl ServerConfig {
  pub fn api_bind(&self) -> String {
    format!("{}:{}", self.bind, self.http_port)
  }
  pub fn gg_bind(&self) -> String {
    format!("{}:{}", self.bind, self.gg_port)
  }
  pub fn hostname(&self) -> &str {
    &self.hostname
  }
}
pub struct AppState {
  db_pool: DatabasePool,
  pub host_ip: IpAddr,
  presence_hub: PresenceHub,
  message_dispatcher: MessageDispatcher,
  config: ServerConfig,
  vault: PasswordVault,
  pub admission: Arc<Admission>,
  pub images: crate::messenger::images::ImageService,
  pub client_downloads: Option<crate::downloads::ClientDownloads>,
}
impl AppState {
  pub fn db_pool(&self) -> &DatabasePool {
    &self.db_pool
  }
  pub fn config(&self) -> &ServerConfig {
    &self.config
  }
  pub fn presence_hub(&self) -> &PresenceHub {
    &self.presence_hub
  }
  pub fn message_dispatcher(&self) -> &MessageDispatcher {
    &self.message_dispatcher
  }
  pub fn vault(&self) -> &PasswordVault {
    &self.vault
  }
  pub fn host_uri(&self, path: &str) -> Result<Uri, http::Error> {
    Uri::builder()
      .scheme(self.config.public_scheme.as_str())
      .authority(self.config.hostname())
      .path_and_query(path)
      .build()
  }
}
pub type SharedAppState = Arc<AppState>;
pub async fn create_app_state() -> Result<SharedAppState, Box<dyn std::error::Error>> {
  let config: ServerConfig = Figment::new()
    .merge(Toml::file("/etc/gg-retro/config.toml"))
    .merge(Toml::file("config.toml"))
    .merge(Env::prefixed("GG_"))
    .join(Serialized::defaults(ServerConfig::default()))
    .extract()?;
  let client_downloads = crate::downloads::load(&config.client_downloads_dir, &config.hostname)?;
  let key = Zeroizing::new(std::fs::read(&config.password_key_file)?);
  let vault = PasswordVault::new(&key)?;
  if !matches!(config.public_scheme.as_str(), "http" | "https") {
    return Err("invalid public scheme".into());
  }
  let db_options = SqliteConnectOptions::new()
    .filename(&config.db)
    .create_if_missing(true)
    .journal_mode(SqliteJournalMode::Wal)
    .synchronous(SqliteSynchronous::Full)
    .busy_timeout(Duration::from_secs(5))
    .foreign_keys(true);
  let db_pool = SqlitePoolOptions::new()
    .max_connections(4)
    .connect_with(db_options)
    .await?;
  sqlx::migrate!("./migrations").run(&db_pool).await?;
  let mut tx = db_pool.begin().await?;
  let credentials: Vec<(u32, String)> = sqlx::query_as("SELECT uin, password FROM users")
    .fetch_all(&mut *tx)
    .await?;
  for (uin, password) in credentials {
    let password = Zeroizing::new(password);
    if password.starts_with("ggv1:") {
      vault.open(&password, uin)?;
    } else if config.migrate_plaintext_passwords {
      sqlx::query("UPDATE users SET password = ? WHERE uin = ?")
        .bind(vault.seal(&password, uin)?)
        .bind(uin)
        .execute(&mut *tx)
        .await?;
    } else {
      return Err("Legacy plaintext credentials found: back up the database/key and explicitly enable migrate_plaintext_passwords once".into());
    }
  }
  tx.commit().await?;
  let invites = Zeroizing::new(std::fs::read_to_string(&config.registration_invite_file)?);
  crate::models::registration::sync_invites(&db_pool, &invites).await?;
  drop(invites);
  if config.migrate_plaintext_passwords {
    sqlx::query("VACUUM").execute(&db_pool).await?;
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
      .execute(&db_pool)
      .await?;
  }
  let host_ip = match config.advertised_ip {
    Some(ip) => ip,
    None => local_ip_address::local_ip()?,
  };
  let message_dispatcher = MessageDispatcher::new(&db_pool);
  Ok(Arc::new(AppState {
    images: crate::messenger::images::ImageService::new(),
    db_pool,
    client_downloads,
    host_ip,
    presence_hub: PresenceHub::new(),
    message_dispatcher,
    config,
    vault,
    admission: Arc::new(Admission::default()),
  }))
}
