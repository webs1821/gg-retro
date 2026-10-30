use super::{MessagesStream, PresenceChangeStream, UserPresence, contact_book::ContactBook};
use crate::{
  core::SharedAppState,
  models::{MessageRepository, UserRepository},
};
use futures::{SinkExt, StreamExt};
use gg_protocol::{
  GGCodec, GGError, GGNumber, GGPacket,
  packets::{ContactEntry, GGSendMessageAck},
};
use rand::Rng;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::{net::TcpStream, sync::OwnedMutexGuard};
use tokio_util::{codec::Framed, sync::CancellationToken};

pub struct UserSessionController {
  seed: u32,
  uin: Option<GGNumber>,
  initial_presence: Option<UserPresence>,
  contacts: ContactBook,
  protocol: Framed<TcpStream, GGCodec>,
  shutdown: CancellationToken,
  replaced: CancellationToken,
  app_state: SharedAppState,
  presence_changes: Option<PresenceChangeStream>,
  messages: Option<MessagesStream>,
  registration: Option<u64>,
  delivery_guard: Option<OwnedMutexGuard<()>>,
  contacts_ready: bool,
}
#[derive(Error, Debug)]
pub enum UserSessionError {
  #[error("Connection closed or replaced")]
  Closed,
  #[error("Authentication failed")]
  Authentication,
  #[error("Connection timed out")]
  Timeout,
  #[error("Protocol limit exceeded")]
  Limit,
  #[error("GG protocol error: {0}")]
  Protocol(#[from] GGError),
  #[error("Repository error: {0}")]
  Repository(#[from] crate::models::RepositoryError),
  #[error("Database error")]
  Database(#[from] sqlx::Error),
}
impl UserSessionController {
  pub fn new(stream: TcpStream, shutdown: CancellationToken, app_state: SharedAppState) -> Self {
    Self {
      seed: rand::rng().random(),
      uin: None,
      initial_presence: None,
      contacts: ContactBook::new(),
      protocol: Framed::new(stream, GGCodec::server()),
      shutdown,
      replaced: CancellationToken::new(),
      app_state,
      presence_changes: None,
      messages: None,
      registration: None,
      delivery_guard: None,
      contacts_ready: false,
    }
  }
  async fn send(&mut self, packet: GGPacket) -> Result<(), UserSessionError> {
    tokio::time::timeout(Duration::from_secs(5), self.protocol.send(packet))
      .await
      .map_err(|_| UserSessionError::Timeout)??;
    Ok(())
  }
  pub async fn establish_session(&mut self) -> Result<(), UserSessionError> {
    self.send(GGPacket::Welcome { seed: self.seed }).await?;
    let packet = tokio::select! {
        _ = self.shutdown.cancelled() => return Err(UserSessionError::Closed),
        packet = tokio::time::timeout(Duration::from_secs(20), self.protocol.next()) => packet.map_err(|_| UserSessionError::Timeout)?,
    };
    let Some(Ok(GGPacket::Login60(login))) = packet else {
      return Err(UserSessionError::Authentication);
    };
    if login
      .description
      .as_ref()
      .is_some_and(|d| d.chars().count() > 70)
    {
      return Err(UserSessionError::Limit);
    }
    let user = UserRepository::new(self.app_state.db_pool())
      .find_by_uin(login.uin)
      .await?;
    if let Some(user) = user {
      use subtle::ConstantTimeEq;
      let password = self.app_state.vault().open(&user.password, user.uin)?;
      if bool::from(gg_protocol::gg_login_hash(&password, self.seed).ct_eq(&login.hash)) {
        self.uin = Some(login.uin);
        self.initial_presence = Some(login.into());
        return Ok(());
      }
    }
    self.send(GGPacket::LoginFailed).await?;
    Err(UserSessionError::Authentication)
  }
  pub async fn sync(&mut self) -> Result<(), UserSessionError> {
    let uin = self.uin.ok_or(UserSessionError::Authentication)?;
    let (id, messages, replaced, guard) = self.app_state.message_dispatcher().register(uin).await;
    self.registration = Some(id);
    self.messages = Some(messages);
    self.replaced = replaced;
    self.delivery_guard = Some(guard);
    if self.replaced.is_cancelled() {
      return Err(UserSessionError::Closed);
    }
    self.presence_changes = Some(self.app_state.presence_hub().register(uin));
    self.app_state.presence_hub().notify(
      self
        .initial_presence
        .take()
        .unwrap_or_else(|| UserPresence::available(uin)),
    );
    self.send(GGPacket::LoginOk).await
  }
  async fn deliver_pending(&mut self) -> Result<(), UserSessionError> {
    if !self.contacts_ready {
      return Ok(());
    }
    let uin = self.uin.ok_or(UserSessionError::Authentication)?;
    let messages = MessageRepository::new(self.app_state.db_pool());
    if let Some(batch) = messages.find_pending(uin).await? {
      for message in batch {
        if self.replaced.is_cancelled() || self.shutdown.is_cancelled() {
          return Err(UserSessionError::Closed);
        }
        let id = message.id;
        if !self.contacts.is_blocked(message.sender_uin) {
          self.send(GGPacket::RecvMessage(message.into())).await?;
        }
        // Each successful socket write is recorded separately; GG6 has no receipt ACK.
        messages.mark_single_delivered(id).await?;
      }
    }
    Ok(())
  }
  async fn sync_contacts(&mut self, contacts: &[ContactEntry]) -> Result<(), UserSessionError> {
    if contacts.len() > 1000 {
      return Err(UserSessionError::Limit);
    }
    let uin = self.uin.ok_or(UserSessionError::Authentication)?;
    self.contacts.set(contacts);
    self.app_state.presence_hub().set_book(uin, &self.contacts);
    self.contacts_ready = true;
    let statuses = self
      .contacts
      .watched()
      .into_iter()
      .map(|other| self.app_state.presence_hub().find_for(other, uin).into())
      .collect();
    self.send(GGPacket::NotifyReply60(statuses)).await?;
    self.deliver_pending().await
  }
  async fn user_list(&mut self, kind: u8, data: Vec<u8>) -> Result<(), UserSessionError> {
    let uin = self.uin.ok_or(UserSessionError::Authentication)?;
    match kind {
      0 | 1 => {
        if data.len() > 2047 {
          return Err(UserSessionError::Limit);
        }
        // GG6 acknowledges every chunk; there is no final-chunk flag on upload.
        if kind == 0 {
          sqlx::query("INSERT INTO contact_lists(uin,data) VALUES (?,?) ON CONFLICT(uin) DO UPDATE SET data=excluded.data, updated_at=CURRENT_TIMESTAMP")
                        .bind(uin).bind(&data).execute(self.app_state.db_pool()).await?;
        } else {
          let result = sqlx::query("UPDATE contact_lists SET data=CAST(data || ? AS BLOB), updated_at=CURRENT_TIMESTAMP WHERE uin=? AND length(data)+? <= 262144")
                        .bind(&data).bind(uin).bind(data.len() as i64).execute(self.app_state.db_pool()).await?;
          if result.rows_affected() != 1 {
            return Err(UserSessionError::Limit);
          }
        }
        self
          .send(GGPacket::UserListReply {
            kind: if kind == 0 { 0 } else { 2 },
            data: vec![],
          })
          .await?;
      }
      2 => {
        let stored: Option<Vec<u8>> =
          sqlx::query_scalar("SELECT data FROM contact_lists WHERE uin=?")
            .bind(uin)
            .fetch_optional(self.app_state.db_pool())
            .await?;
        let stored = stored.unwrap_or_default();
        if stored.is_empty() {
          self
            .send(GGPacket::UserListReply {
              kind: 6,
              data: vec![],
            })
            .await?;
        } else {
          let total = stored.len().div_ceil(2047);
          for (index, chunk) in stored.chunks(2047).enumerate() {
            self
              .send(GGPacket::UserListReply {
                kind: if index + 1 == total { 6 } else { 4 },
                data: chunk.to_vec(),
              })
              .await?;
          }
        }
      }
      _ => return Err(UserSessionError::Limit),
    }
    Ok(())
  }
  pub async fn run(&mut self) -> Result<(), UserSessionError> {
    let uin = self.uin.ok_or(UserSessionError::Authentication)?;
    let mut contacts = Vec::new();
    let mut poll = tokio::time::interval(Duration::from_secs(2));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_packet = Instant::now();
    let mut rate_window = Instant::now();
    let mut packet_count = 0u32;
    let mut message_count = 0u32;
    loop {
      tokio::select! {
          _ = self.shutdown.cancelled() => break,
          _ = self.replaced.cancelled() => break,
          _ = poll.tick() => {
              if last_packet.elapsed() > Duration::from_secs(300) { return Err(UserSessionError::Timeout); }
              self.deliver_pending().await?;
          }
          Some(()) = self.messages.as_mut().unwrap().next() => self.deliver_pending().await?,
          Some(other) = self.presence_changes.as_mut().unwrap().next() => {
              let status = self.app_state.presence_hub().find_for(other, uin);
              self.send(GGPacket::Status60(status.into())).await?;
          }
          packet = self.protocol.next() => {
              let packet = packet.ok_or(UserSessionError::Closed)??;
              last_packet = Instant::now();
              if rate_window.elapsed() >= Duration::from_secs(60) { rate_window = Instant::now(); packet_count = 0; message_count = 0; }
              packet_count += 1;
              if packet_count > 600 { return Err(UserSessionError::Limit); }
              match packet {
                  GGPacket::Ping => self.send(GGPacket::Pong).await?,
                  GGPacket::SendMessage(message) => {
                      message_count += 1;
                      if message_count > 300 { return Err(UserSessionError::Limit); }
                      let recipient = message.recipient; let seq = message.seq;
                      let status = self.app_state.message_dispatcher().dispatch(uin, message).await?;
                      self.send(GGPacket::SendMessageAck(GGSendMessageAck { recipient, seq, status })).await?;
                  }
                  GGPacket::ListEmpty => {
                      contacts.clear(); self.contacts.set(&[]);
                      self.app_state.presence_hub().set_book(uin, &self.contacts);
                      self.contacts_ready = true; self.deliver_pending().await?;
                  }
                  GGPacket::NotifyFirst(batch) => {
                      if contacts.len() + batch.len() > 1000 { return Err(UserSessionError::Limit); }
                      contacts.extend(batch);
                  }
                  GGPacket::NotifyLast(batch) => {
                      if contacts.len() + batch.len() > 1000 { return Err(UserSessionError::Limit); }
                      contacts.extend(batch); self.sync_contacts(&contacts).await?; contacts.clear();
                  }
                  GGPacket::AddNotify(entry) => {
                      self.contacts.add(&entry); let entries = self.contacts.entries(); self.sync_contacts(&entries).await?;
                  }
                  GGPacket::RemoveNotify(entry) => {
                      self.contacts.remove(&entry); let entries = self.contacts.entries(); self.sync_contacts(&entries).await?;
                  }
                  GGPacket::NewStatus(status) => {
                      if status.description.as_ref().is_some_and(|d| d.chars().count() > 70) { return Err(UserSessionError::Limit); }
                      self.app_state.presence_hub().notify(UserPresence { uin, status: status.status, description: status.description, time: status.time });
                  }
                  GGPacket::UserListRequest { kind, data } => self.user_list(kind, data).await?,
                  GGPacket::Disconnect => break,
                  _ => {} // Unknown extensions contain no decoded user text in logs.
              }
          }
      }
    }
    let _ = self.send(GGPacket::Disconnect).await;
    Ok(())
  }
  pub async fn cleanup(&mut self) {
    if let (Some(uin), Some(id)) = (self.uin, self.registration.take()) {
      // Still holding the account delivery guard: successor cannot publish presence yet.
      self.app_state.presence_hub().unregister(uin, &[]);
      self.app_state.message_dispatcher().unregister(uin, id);
    }
    self.delivery_guard.take();
  }
}
