use super::images::{Begin, ImageEvent, ImagePayload};
use super::{MessagesStream, PresenceChangeStream, UserPresence, contact_book::ContactBook};
use crate::models::images;
use crate::{
  core::SharedAppState,
  models::{MessageRepository, UserRepository},
};
use futures::{SinkExt, StreamExt};
use gg_protocol::{
  GGCodec, GGError, GGNumber, GGPacket,
  packets::{ContactEntry, GGSendMessageAck},
};
use gg_protocol::{
  consts::{AckStatus, GGMessageClass},
  packets::{GGRecvImage, GGSendImage, ImageId, ImageTransfer, MAX_IMAGE_OPTION_BYTES},
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
  image_events: Option<tokio_stream::wrappers::ReceiverStream<ImageEvent>>,
  image_size: u8,
  logoff: bool,
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
      image_events: None,
      image_size: 0,
      logoff: false,
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
        self.image_size = login.image_size;
        self.initial_presence = Some(login.into());
        return Ok(());
      }
    }
    self.send(GGPacket::LoginFailed).await?;
    Err(UserSessionError::Authentication)
  }
  pub async fn sync(&mut self) -> Result<(), UserSessionError> {
    let uin = self.uin.ok_or(UserSessionError::Authentication)?;
    let (id, messages, image_events, replaced, guard) =
      self.app_state.message_dispatcher().register(uin).await;
    self.registration = Some(id);
    self.messages = Some(messages);
    self.image_events = Some(image_events);
    self.replaced = replaced;
    self.delivery_guard = Some(guard);
    if self.replaced.is_cancelled() {
      return Err(UserSessionError::Closed);
    }
    crate::models::offline_presence::clear(self.app_state.db_pool(), uin).await?;
    self.presence_changes = Some(self.app_state.presence_hub().register(uin));
    // Do not publish until the complete contact/privacy list has arrived.
    self.send(GGPacket::LoginOk).await
  }
  async fn publish_initial(&mut self) -> Result<(), UserSessionError> {
    if let Some(presence) = self.initial_presence.take() {
      self.set_presence(presence).await?;
    }
    Ok(())
  }
  async fn set_presence(&mut self, mut presence: UserPresence) -> Result<(), UserSessionError> {
    use gg_protocol::consts::GGStatus;
    let offline = matches!(
      presence.status.base(),
      GGStatus::NotAvail | GGStatus::NotAvailDescr
    );
    if !offline && !self.contacts_ready {
      self.initial_presence = Some(presence);
      return Ok(());
    }
    if offline {
      self.logoff = true;
      presence.image_size = 0;
      // Never expose a description without the account's complete privacy list.
      if self.contacts_ready
        && presence.status.base() == GGStatus::NotAvailDescr
        && presence.description.as_ref().is_some_and(|d| !d.is_empty())
      {
        crate::models::offline_presence::save(self.app_state.db_pool(), &presence, &self.contacts)
          .await?;
      } else {
        crate::models::offline_presence::clear(self.app_state.db_pool(), presence.uin).await?;
        presence = UserPresence::offline(presence.uin);
      }
    }
    self.app_state.presence_hub().notify(presence);
    Ok(())
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
        } else {
          sqlx::query("DELETE FROM message_images WHERE message_id=?")
            .bind(id)
            .execute(self.app_state.db_pool())
            .await?;
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
    self.publish_initial().await?;
    if self.logoff {
      return Ok(());
    }
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
    let mut image_requests = 0u32;
    loop {
      tokio::select! {
          biased;
          _ = self.shutdown.cancelled() => break,
          _ = self.replaced.cancelled() => break,
          _ = poll.tick() => {
              if last_packet.elapsed() > Duration::from_secs(300) { return Err(UserSessionError::Timeout); }
              self.deliver_pending().await?;
              for (target,event) in self.app_state.images.expire() { self.app_state.message_dispatcher().image_event(target,event); }
          }
          Some(()) = self.messages.as_mut().unwrap().next() => self.deliver_pending().await?,
          Some(event) = self.image_events.as_mut().unwrap().next(), if self.contacts_ready => self.image_event(event).await?,
          Some(other) = self.presence_changes.as_mut().unwrap().next() => {
              let status = self.app_state.presence_hub().find_for(other, uin);
              self.send(GGPacket::Status60(status.into())).await?;
          }
          packet = self.protocol.next() => {
              let packet = packet.ok_or(UserSessionError::Closed)??;
              last_packet = Instant::now();
              if rate_window.elapsed() >= Duration::from_secs(60) { rate_window = Instant::now(); packet_count = 0; message_count = 0; image_requests = 0; }
              packet_count += 1;
              if packet_count > 600 { return Err(UserSessionError::Limit); }
              match packet {
                  GGPacket::Ping => self.send(GGPacket::Pong).await?,
                  GGPacket::SendImage(message) => {
                      if matches!(message.transfer,ImageTransfer::Request(_)) {
                          image_requests += 1;
                          if image_requests > 20 { return Err(UserSessionError::Limit); }
                      }
                      self.image_message(message).await?;
                  }
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
                      self.contacts_ready = true; self.publish_initial().await?;
                      if !self.logoff { self.deliver_pending().await?; }
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
                      self.set_presence(UserPresence { uin, image_size: self.image_size, status: status.status, description: status.description, time: status.time }).await?;
                  }
                  GGPacket::UserListRequest { kind, data } => self.user_list(kind, data).await?,
                  GGPacket::Disconnect => break,
                  _ => {} // Unknown extensions contain no decoded user text in logs.
              }
              if self.logoff { break; }
          }
      }
    }
    // GG_DISCONNECTING means a forced logout (e.g. another login), not maintenance.
    // Classic clients can stop reconnecting after it. A restart gets an ordinary FIN.
    if self.replaced.is_cancelled() && !self.shutdown.is_cancelled() {
      let _ = self.send(GGPacket::Disconnect).await;
    }
    Ok(())
  }
  pub async fn cleanup(&mut self) {
    // Flush then half-close, so queued data/FIN is not replaced by an abrupt reset.
    let _ = tokio::time::timeout(Duration::from_secs(1), self.protocol.close()).await;
    if let (Some(uin), Some(id)) = (self.uin, self.registration.take()) {
      for (target, event) in self.app_state.images.disconnect(uin, id) {
        self
          .app_state
          .message_dispatcher()
          .image_event(target, event);
      }
      // Still holding the account delivery guard: successor cannot publish presence yet.
      self.app_state.presence_hub().unregister(uin, &[]);
      self.app_state.message_dispatcher().unregister(uin, id);
    }
    self.delivery_guard.take();
  }

  fn images_blocked(&self, owner: u32, recipient: u32) -> bool {
    self.app_state.presence_hub().is_blocked(owner, recipient)
      || self.app_state.presence_hub().is_blocked(recipient, owner)
  }
  async fn start_image_fetch(
    &self,
    owner: u32,
    recipient: u32,
    id: ImageId,
    waiter: u64,
  ) -> Result<bool, UserSessionError> {
    if self.images_blocked(owner, recipient)
      || !images::offered(self.app_state.db_pool(), owner, recipient, id).await?
    {
      return Ok(false);
    }
    let Some(owner_session) = self.app_state.message_dispatcher().session_id(owner) else {
      return Ok(false);
    };
    match self
      .app_state
      .images
      .begin(owner, recipient, id, owner_session, waiter)
    {
      Begin::Existing => Ok(true),
      Begin::Full => Ok(false),
      Begin::New => {
        if self.app_state.message_dispatcher().image_event(
          owner,
          ImageEvent::Request {
            recipient,
            id,
            owner_session,
          },
        ) {
          Ok(true)
        } else {
          self
            .app_state
            .images
            .cancel(owner, recipient, id, owner_session);
          Ok(false)
        }
      }
    }
  }
  async fn send_image(
    &mut self,
    owner: u32,
    transfer: ImageTransfer,
  ) -> Result<(), UserSessionError> {
    self
      .send(GGPacket::RecvImage(GGRecvImage {
        sender: owner,
        seq: 0,
        time: std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)
          .unwrap_or_default()
          .as_secs() as u32,
        class: GGMessageClass::Msg,
        transfer,
      }))
      .await
  }
  async fn image_ack(
    &mut self,
    message: &GGSendImage,
    status: AckStatus,
  ) -> Result<(), UserSessionError> {
    if message.class.bits() & GGMessageClass::Ack.bits() == 0 {
      self
        .send(GGPacket::SendMessageAck(GGSendMessageAck {
          recipient: message.recipient,
          seq: message.seq,
          status,
        }))
        .await?;
    }
    Ok(())
  }
  async fn image_message(&mut self, message: GGSendImage) -> Result<(), UserSessionError> {
    let uin = self.uin.ok_or(UserSessionError::Authentication)?;
    let session = self.registration.ok_or(UserSessionError::Authentication)?;
    let id = message.transfer.id();
    if !self.contacts_ready || self.images_blocked(uin, message.recipient) {
      if !matches!(message.transfer, ImageTransfer::Request(_)) {
        if let Some((target, event)) =
          self
            .app_state
            .images
            .cancel(uin, message.recipient, id, session)
        {
          self
            .app_state
            .message_dispatcher()
            .image_event(target, event);
        }
      }
      self.image_ack(&message, AckStatus::Blocked).await?;
      if matches!(message.transfer, ImageTransfer::Request(_)) {
        self
          .send_image(message.recipient, ImageTransfer::unavailable(id))
          .await?;
      }
      return Ok(());
    }
    if matches!(message.transfer, ImageTransfer::Request(_)) {
      let accepted = id.size <= u32::from(self.image_size) * 1024
        && self
          .start_image_fetch(message.recipient, uin, id, session)
          .await?;
      self
        .image_ack(
          &message,
          if accepted {
            AckStatus::Delivered
          } else {
            AckStatus::NotDelivered
          },
        )
        .await?;
      if !accepted {
        self
          .send_image(message.recipient, ImageTransfer::unavailable(id))
          .await?;
      }
    } else {
      let (status, notification) =
        self
          .app_state
          .images
          .receive(uin, message.recipient, session, message.transfer.clone());
      if let Some((recipient, event)) = notification {
        if !self
          .app_state
          .message_dispatcher()
          .image_event(recipient, event)
        {
          self.app_state.images.cancel(uin, recipient, id, session);
        }
      }
      self.image_ack(&message, status).await?;
    }
    Ok(())
  }
  async fn image_event(&mut self, event: ImageEvent) -> Result<(), UserSessionError> {
    match event {
      ImageEvent::Request {
        recipient,
        id,
        owner_session,
      } => {
        let owner = self.uin.ok_or(UserSessionError::Authentication)?;
        if !self
          .app_state
          .images
          .expecting(owner, recipient, id, owner_session)
        {
          return Ok(());
        }
        if self.registration == Some(owner_session) && !self.images_blocked(owner, recipient) {
          self
            .send_image(recipient, ImageTransfer::Request(id))
            .await?;
        } else if let Some((target, event)) =
          self
            .app_state
            .images
            .cancel(owner, recipient, id, owner_session)
        {
          self
            .app_state
            .message_dispatcher()
            .image_event(target, event);
        }
      }
      ImageEvent::Ready {
        owner,
        id,
        request_session,
      } => {
        let recipient = self.uin.ok_or(UserSessionError::Authentication)?;
        if self.registration == Some(request_session) {
          let payload = self
            .app_state
            .images
            .take(owner, recipient, id, request_session);
          if !self.images_blocked(owner, recipient)
            && images::offered(self.app_state.db_pool(), owner, recipient, id).await?
          {
            if let Some(payload) = payload {
              self.send_image_payload(owner, id, payload).await?;
              return Ok(());
            }
          }
          self
            .send_image(owner, ImageTransfer::unavailable(id))
            .await?;
        }
      }
      ImageEvent::Unavailable {
        owner,
        id,
        request_session,
      } => {
        if self.registration == Some(request_session) {
          self
            .send_image(owner, ImageTransfer::unavailable(id))
            .await?;
        }
      }
    }
    Ok(())
  }
  async fn send_image_payload(
    &mut self,
    owner: u32,
    id: ImageId,
    image: ImagePayload,
  ) -> Result<(), UserSessionError> {
    if id.size > u32::from(self.image_size) * 1024
      || image.data.len() != id.size as usize
      || crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(&image.data) != id.crc32
    {
      return self.send_image(owner, ImageTransfer::unavailable(id)).await;
    }
    let start = Instant::now();
    let mut offset = 0;
    while offset < image.data.len() {
      if self.replaced.is_cancelled() || self.shutdown.is_cancelled() {
        return Err(UserSessionError::Closed);
      }
      if start.elapsed() > Duration::from_secs(30) {
        return Err(UserSessionError::Timeout);
      }
      let first = offset == 0;
      let capacity = MAX_IMAGE_OPTION_BYTES - 9 - if first { image.filename.len() + 1 } else { 0 };
      let end = (offset + capacity).min(image.data.len());
      self
        .send_image(
          owner,
          ImageTransfer::Reply {
            id,
            first,
            filename: if first {
              Some(image.filename.clone())
            } else {
              None
            },
            data: image.data[offset..end].to_vec(),
          },
        )
        .await?;
      offset = end;
    }
    Ok(())
  }
}
