use crate::codec_helpers::{
  decode_contact_entries, decode_contact_statuses, decode_cp1250, decode_richtext_formatting,
  encode_contact_status_no_size, encode_contact_status_with_size, encode_cp1250,
  encode_richtext_formatting,
};
use crate::consts::{AckStatus, GGMessageClass, GGStatus, packet_type};
use crate::error::GGError;
use crate::packets::{
  GGLogin60, GGPacket, GGRecvImage, GGRecvMessage, GGSendImage, GGSendMessage, GGSendMessageAck,
  ImageTransfer, NewStatus,
};
use crate::packets::{MAX_CONFERENCE_RECIPIENTS, decode_conference, encode_conference};
use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

/// Codec mode for handling direction-dependent packet types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodecMode {
  /// Server mode: decodes packets sent by client (C → S).
  #[default]
  Server,
  /// Client mode: decodes packets sent by server (S → C).
  Client,
}

#[derive(Debug, Default)]
pub struct GGCodec {
  mode: CodecMode,
}

impl GGCodec {
  /// Create a codec for server-side usage (decodes C → S packets).
  pub fn server() -> Self {
    Self {
      mode: CodecMode::Server,
    }
  }

  /// Create a codec for client-side usage (decodes S → C packets).
  pub fn client() -> Self {
    Self {
      mode: CodecMode::Client,
    }
  }
}

impl Encoder<GGPacket> for GGCodec {
  type Error = GGError;

  fn encode(&mut self, item: GGPacket, dst: &mut BytesMut) -> Result<(), Self::Error> {
    match item {
      GGPacket::DirectoryRequest(packet) | GGPacket::DirectoryReply(packet) => {
        let reply = self.mode == CodecMode::Server;
        let payload = packet.encode(reply)?;
        dst.put_u32_le(if reply { 0x0e } else { 0x14 });
        dst.put_u32_le(payload.len() as u32);
        dst.extend_from_slice(&payload);
      }
      GGPacket::SendImage(image) => {
        let option = image.transfer.encode()?;
        dst.put_u32_le(packet_type::GG_SEND_MSG);
        dst.put_u32_le((13 + option.len()) as u32);
        dst.put_u32_le(image.recipient);
        dst.put_u32_le(image.seq);
        dst.put_u32_le(image.class.bits());
        dst.put_u8(0);
        dst.extend_from_slice(&option);
      }
      GGPacket::RecvImage(image) => {
        let option = image.transfer.encode()?;
        dst.put_u32_le(packet_type::GG_RECV_MSG);
        dst.put_u32_le((17 + option.len()) as u32);
        dst.put_u32_le(image.sender);
        dst.put_u32_le(image.seq);
        dst.put_u32_le(image.time);
        dst.put_u32_le(image.class.bits());
        dst.put_u8(0);
        dst.extend_from_slice(&option);
      }
      GGPacket::AddNotify(entry) => {
        dst.put_u32_le(0x0d);
        dst.put_u32_le(5);
        dst.put_u32_le(entry.uin);
        dst.put_u8(entry.user_type.bits());
      }
      GGPacket::RemoveNotify(entry) => {
        dst.put_u32_le(0x0e);
        dst.put_u32_le(5);
        dst.put_u32_le(entry.uin);
        dst.put_u8(entry.user_type.bits());
      }
      GGPacket::UserListRequest { kind, data } | GGPacket::UserListReply { kind, data } => {
        dst.put_u32_le(if self.mode == CodecMode::Client {
          0x16
        } else {
          0x10
        });
        dst.put_u32_le((data.len() + 1) as u32);
        dst.put_u8(kind);
        dst.extend_from_slice(&data);
      }
      GGPacket::Unknown { .. } => {
        return Err(GGError::InvalidFrame("cannot encode unknown packet"));
      }
      GGPacket::Welcome { seed } => {
        dst.put_u32_le(packet_type::GG_WELCOME);
        dst.put_u32_le(4);
        dst.put_u32_le(seed);
      }

      GGPacket::Login60(login) => {
        let mut payload = BytesMut::new();

        // Fixed fields (always present) - 31 bytes total
        payload.put_u32_le(login.uin); // 4 bytes
        payload.put_u32_le(login.hash); // 4 bytes
        payload.put_u32_le(login.status.bits()); // 4 bytes
        payload.put_u32_le(login.version); // 4 bytes
        payload.put_u8(login.unknown1); // 1 byte (0x00)
        payload.put_slice(&login.local_ip.octets()); // 4 bytes
        payload.put_u16_le(login.local_port); // 2 bytes
        payload.put_slice(&login.external_ip.octets()); // 4 bytes
        payload.put_u16_le(login.external_port); // 2 bytes
        payload.put_u8(login.image_size); // 1 byte
        payload.put_u8(login.unknown2); // 1 byte (0xbe)

        // Optional fields (description + time)
        if let Some(ref desc) = login.description {
          let desc_bytes = encode_cp1250(desc);
          payload.put_slice(&desc_bytes);
          payload.put_u8(0); // null terminator
          if let Some(time) = login.time {
            payload.put_u32_le(time);
          }
        }

        dst.put_u32_le(packet_type::GG_LOGIN60);
        dst.put_u32_le(payload.len() as u32);
        dst.put_slice(&payload);
      }

      GGPacket::LoginOk => {
        dst.put_u32_le(packet_type::GG_LOGIN_OK);
        dst.put_u32_le(0); // Zero length - empty packet
      }

      GGPacket::LoginFailed => {
        dst.put_u32_le(packet_type::GG_LOGIN_FAILED);
        dst.put_u32_le(0); // Zero length - empty packet
      }

      GGPacket::Ping => {
        dst.put_u32_le(packet_type::GG_PING);
        dst.put_u32_le(0);
      }

      GGPacket::ListEmpty => {
        dst.put_u32_le(packet_type::GG_LIST_EMPTY);
        dst.put_u32_le(0);
      }

      GGPacket::NotifyFirst(entries) => {
        let length = entries.len() * 5; // 4 bytes uin + 1 byte type
        dst.put_u32_le(packet_type::GG_NOTIFY_FIRST);
        dst.put_u32_le(length as u32);
        for entry in entries {
          dst.put_u32_le(entry.uin);
          dst.put_u8(entry.user_type.bits());
        }
      }

      GGPacket::NotifyLast(entries) => {
        let length = entries.len() * 5; // 4 bytes uin + 1 byte type
        dst.put_u32_le(packet_type::GG_NOTIFY_LAST);
        dst.put_u32_le(length as u32);
        for entry in entries {
          dst.put_u32_le(entry.uin);
          dst.put_u8(entry.user_type.bits());
        }
      }

      GGPacket::Pong => {
        dst.put_u32_le(packet_type::GG_PONG);
        dst.put_u32_le(0);
      }

      GGPacket::Disconnect => {
        dst.put_u32_le(packet_type::GG_DISCONNECTING);
        dst.put_u32_le(0);
      }

      GGPacket::NotifyReply60(statuses) => {
        let mut payload = BytesMut::new();
        for status in &statuses {
          encode_contact_status_with_size(&mut payload, status);
        }
        dst.put_u32_le(packet_type::GG_NOTIFY_REPLY60);
        dst.put_u32_le(payload.len() as u32);
        dst.put_slice(&payload);
      }

      GGPacket::Status60(status) => {
        let mut payload = BytesMut::new();
        encode_contact_status_no_size(&mut payload, &status);
        dst.put_u32_le(packet_type::GG_STATUS60);
        dst.put_u32_le(payload.len() as u32);
        dst.put_slice(&payload);
      }

      GGPacket::NewStatus(new_status) => {
        let mut payload = BytesMut::new();
        payload.put_u32_le(new_status.status.bits());

        // Optional description (null-terminated) + optional time
        if let Some(ref desc) = new_status.description {
          let desc_bytes = encode_cp1250(desc);
          payload.put_slice(&desc_bytes);
          payload.put_u8(0); // null terminator
          if let Some(time) = new_status.time {
            payload.put_u32_le(time);
          }
        }

        dst.put_u32_le(packet_type::GG_NEW_STATUS);
        dst.put_u32_le(payload.len() as u32);
        dst.put_slice(&payload);
      }

      GGPacket::SendMessage(msg) => {
        if msg.recipients.contains(&msg.recipient) {
          return Err(GGError::InvalidFrame("conference includes endpoint"));
        }
        let conference = encode_conference(&msg.recipients)?;
        let mut payload = BytesMut::new();
        payload.put_u32_le(msg.recipient);
        payload.put_u32_le(msg.seq);
        payload.put_u32_le(msg.class.bits());
        let msg_bytes = encode_cp1250(&msg.message);
        payload.put_slice(&msg_bytes);
        payload.put_u8(0); // null terminator

        payload.extend_from_slice(&conference);
        // Conference attributes precede rich text in GG6.
        if let Some(ref formats) = msg.formatting {
          if !formats.is_empty() {
            let richtext_data = encode_richtext_formatting(formats);
            payload.put_slice(&richtext_data);
          }
        }

        dst.put_u32_le(packet_type::GG_SEND_MSG);
        dst.put_u32_le(payload.len() as u32);
        dst.put_slice(&payload);
      }

      GGPacket::RecvMessage(msg) => {
        if msg.recipients.contains(&msg.sender) {
          return Err(GGError::InvalidFrame("conference includes endpoint"));
        }
        let conference = encode_conference(&msg.recipients)?;
        let mut payload = BytesMut::new();
        payload.put_u32_le(msg.sender);
        payload.put_u32_le(msg.seq);
        payload.put_u32_le(msg.time);
        payload.put_u32_le(msg.class.bits());
        let msg_bytes = encode_cp1250(&msg.message);
        payload.put_slice(&msg_bytes);
        payload.put_u8(0); // null terminator

        payload.extend_from_slice(&conference);
        // Conference attributes precede rich text in GG6.
        if let Some(ref formats) = msg.formatting {
          if !formats.is_empty() {
            let richtext_data = encode_richtext_formatting(formats);
            payload.put_slice(&richtext_data);
          }
        }

        dst.put_u32_le(packet_type::GG_RECV_MSG);
        dst.put_u32_le(payload.len() as u32);
        dst.put_slice(&payload);
      }

      GGPacket::SendMessageAck(ack) => {
        dst.put_u32_le(packet_type::GG_SEND_MSG_ACK);
        dst.put_u32_le(12); // Fixed size: 3 * 4 bytes
        dst.put_u32_le(ack.status as u32);
        dst.put_u32_le(ack.recipient);
        dst.put_u32_le(ack.seq);
      }
    }
    Ok(())
  }
}

impl Decoder for GGCodec {
  type Item = GGPacket;
  type Error = GGError;

  fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
    if src.len() < 8 {
      return Ok(None);
    }

    // Peek at the packet type and length without consuming
    let packet_type = u32::from_le_bytes([src[0], src[1], src[2], src[3]]);
    let length = u32::from_le_bytes([src[4], src[5], src[6], src[7]]) as usize;

    if length > 65_536 {
      return Err(GGError::InvalidFrame("payload exceeds 64 KiB"));
    }
    let min = match (self.mode, packet_type) {
      (_, 1) => 4,
      (_, 0x15) => 31,
      (_, 2) => 4,
      (CodecMode::Server, 0x14) | (CodecMode::Client, 0x0e) => 5,
      (CodecMode::Server, 0x0b) => 13,
      (CodecMode::Server, 0x0d | 0x0e) => 5,
      (CodecMode::Server, 0x16) | (CodecMode::Client, 0x10) => 1,
      (_, 0x0a) => 17,
      (_, 5) => 12,
      (CodecMode::Client, 0x0f) => 14,
      _ => 0,
    };
    if length < min {
      return Err(GGError::InvalidFrame("truncated fixed fields"));
    }
    if (self.mode == CodecMode::Server && packet_type == 0x14 && length > 4096)
      || (self.mode == CodecMode::Client && packet_type == 0x0e && length > 32768)
    {
      return Err(GGError::InvalidFrame("public directory payload limit"));
    }
    if self.mode == CodecMode::Server
      && matches!(packet_type, 0x0d | 0x0e | 0x0f | 0x10)
      && (!length.is_multiple_of(5) || (matches!(packet_type, 0x0d | 0x0e) && length != 5))
    {
      return Err(GGError::InvalidFrame("invalid contact entry length"));
    }
    // Check if we have enough data for the full packet
    if src.len() < 8 + length {
      return Ok(None);
    }

    // Now consume the header
    src.advance(8);
    // Parse an isolated frame: a malformed extension can never consume the next header.
    let mut frame = src.split_to(length);
    let src = &mut frame;
    match packet_type {
      0x14 if self.mode == CodecMode::Server => Ok(Some(GGPacket::DirectoryRequest(
        crate::packets::DirectoryPacket::decode(src, false)?,
      ))),
      0x0e if self.mode == CodecMode::Client => Ok(Some(GGPacket::DirectoryReply(
        crate::packets::DirectoryPacket::decode(src, true)?,
      ))),
      0x0d | 0x0e if self.mode == CodecMode::Server => {
        let entry = decode_contact_entries(src, 5).remove(0);
        Ok(Some(if packet_type == 0x0d {
          GGPacket::AddNotify(entry)
        } else {
          GGPacket::RemoveNotify(entry)
        }))
      }
      0x16 if self.mode == CodecMode::Server => {
        let kind = src.get_u8();
        Ok(Some(GGPacket::UserListRequest {
          kind,
          data: src.to_vec(),
        }))
      }
      0x10 if self.mode == CodecMode::Client => {
        let kind = src.get_u8();
        Ok(Some(GGPacket::UserListReply {
          kind,
          data: src.to_vec(),
        }))
      }
      0x0f if self.mode == CodecMode::Client => {
        // GG_STATUS60 uses a trailing description without the length byte.
        let mut status_frame = src.split_to(14);
        let mut statuses = decode_contact_statuses(&mut status_frame, 14);
        let mut status = statuses.remove(0);
        if !src.is_empty() {
          let end = src.iter().position(|b| *b == 0).unwrap_or(src.len());
          status.description = Some(decode_cp1250(&src[..end]));
          if src.len() >= end + 5 {
            status.time = Some(u32::from_le_bytes(
              src[end + 1..end + 5].try_into().unwrap(),
            ));
          }
        }
        Ok(Some(GGPacket::Status60(status)))
      }
      packet_type::GG_WELCOME => {
        let seed = src.get_u32_le();
        return Ok(Some(GGPacket::Welcome { seed }));
      }

      packet_type::GG_LOGIN60 => {
        let uin = src.get_u32_le();
        let hash = src.get_u32_le();
        let status = src.get_u32_le();
        let version = src.get_u32_le();
        let unknown1 = src.get_u8();

        let local_ip = std::net::Ipv4Addr::new(src[0], src[1], src[2], src[3]);
        src.advance(4);
        let local_port = src.get_u16_le();

        let external_ip = std::net::Ipv4Addr::new(src[0], src[1], src[2], src[3]);
        src.advance(4);
        let external_port = src.get_u16_le();

        let image_size = src.get_u8();
        let unknown2 = src.get_u8();

        // Calculate remaining bytes for optional description
        let fixed_size: usize = 4 + 4 + 4 + 4 + 1 + 4 + 2 + 4 + 2 + 1 + 1; // 31 bytes
        let remaining = length.saturating_sub(fixed_size);

        let (description, time) = if remaining > 0 {
          // Find null terminator
          let desc_end = src
            .iter()
            .take(remaining)
            .position(|&b| b == 0)
            .unwrap_or(remaining);
          let desc = decode_cp1250(&src[..desc_end]);
          src.advance(desc_end);

          // Skip null terminator if present
          if desc_end < remaining {
            src.advance(1);
          }

          // Check if time field is present (4 bytes remaining after null)
          let bytes_read = desc_end + 1;
          let time = if remaining > bytes_read && remaining - bytes_read >= 4 {
            Some(src.get_u32_le())
          } else {
            None
          };

          (Some(desc), time)
        } else {
          (None, None)
        };

        return Ok(Some(GGPacket::Login60(GGLogin60 {
          uin,
          hash,
          status: status
            .try_into()
            .map_err(|_| GGError::InvalidFrame("unsupported status"))?,
          version,
          unknown1,
          local_ip,
          local_port,
          external_ip,
          external_port,
          image_size,
          unknown2,
          description,
          time,
        })));
      }

      packet_type::GG_LOGIN_OK => {
        if length > 0 {
          src.advance(length); // Skip any unexpected content
        }
        return Ok(Some(GGPacket::LoginOk));
      }

      packet_type::GG_PING => {
        if length > 0 {
          src.advance(length); // Skip any unexpected content
        }
        return Ok(Some(GGPacket::Ping));
      }

      packet_type::GG_PONG => {
        if length > 0 {
          src.advance(length); // Skip any unexpected content
        }
        return Ok(Some(GGPacket::Pong));
      }

      packet_type::GG_DISCONNECTING => {
        // 0x000b is used for both GG_DISCONNECTING (S→C) and GG_SEND_MSG (C→S)
        // Disambiguate based on codec mode
        match self.mode {
          CodecMode::Server => {
            // Server receives GG_SEND_MSG from client
            let recipient = src.get_u32_le();
            let seq = src.get_u32_le();
            let class_raw = src.get_u32_le();
            let class = GGMessageClass::try_from(class_raw).unwrap_or_default();

            let (message, formatting, recipients, image) = decode_message_body(src)?;
            if let Some(transfer) = image {
              return Ok(Some(GGPacket::SendImage(GGSendImage {
                recipient,
                seq,
                class,
                transfer,
              })));
            }

            if recipients.contains(&recipient) {
              return Err(GGError::InvalidFrame("conference includes endpoint"));
            }
            return Ok(Some(GGPacket::SendMessage(GGSendMessage {
              recipients,
              recipient,
              seq,
              class,
              message,
              formatting,
            })));
          }
          CodecMode::Client => {
            // Client receives GG_DISCONNECTING from server
            if length > 0 {
              src.advance(length);
            }
            return Ok(Some(GGPacket::Disconnect));
          }
        }
      }

      packet_type::GG_LIST_EMPTY => {
        if length > 0 {
          src.advance(length); // Skip any unexpected content
        }
        return Ok(Some(GGPacket::ListEmpty));
      }

      packet_type::GG_NOTIFY_FIRST => {
        let entries = decode_contact_entries(src, length);
        return Ok(Some(GGPacket::NotifyFirst(entries)));
      }

      packet_type::GG_NOTIFY_LAST => {
        let entries = decode_contact_entries(src, length);
        return Ok(Some(GGPacket::NotifyLast(entries)));
      }

      packet_type::GG_LOGIN_FAILED => {
        if length > 0 {
          src.advance(length); // Skip any content
        }
        return Ok(Some(GGPacket::LoginFailed));
      }

      packet_type::GG_NOTIFY_REPLY60 => {
        let statuses = decode_contact_statuses(src, length);
        return Ok(Some(GGPacket::NotifyReply60(statuses)));
      }

      packet_type::GG_NEW_STATUS => {
        let status_raw = src.get_u32_le();
        let status = GGStatus::try_from(status_raw)
          .map_err(|_| GGError::InvalidFrame("unsupported status"))?;

        // Parse optional description and time
        let remaining = length.saturating_sub(4);
        let (description, time) = if remaining > 0 {
          // Find null terminator
          let desc_end = src
            .iter()
            .take(remaining)
            .position(|&b| b == 0)
            .unwrap_or(remaining);
          let desc = if desc_end > 0 {
            Some(decode_cp1250(&src[..desc_end]))
          } else {
            None
          };
          src.advance(desc_end);

          // Skip null terminator if present
          if desc_end < remaining {
            src.advance(1);
          }

          // Check if time field is present (4 bytes remaining after null)
          let bytes_read = desc_end + 1;
          let time = if remaining > bytes_read && remaining - bytes_read >= 4 {
            Some(src.get_u32_le())
          } else {
            None
          };

          (desc, time)
        } else {
          (None, None)
        };

        return Ok(Some(GGPacket::NewStatus(NewStatus {
          status,
          description,
          time,
        })));
      }

      packet_type::GG_RECV_MSG => {
        let sender = src.get_u32_le();
        let seq = src.get_u32_le();
        let time = src.get_u32_le();
        let class_raw = src.get_u32_le();
        let class = GGMessageClass::try_from(class_raw).unwrap_or_default();

        let (message, formatting, recipients, image) = decode_message_body(src)?;
        if let Some(transfer) = image {
          return Ok(Some(GGPacket::RecvImage(GGRecvImage {
            sender,
            seq,
            time,
            class,
            transfer,
          })));
        }

        if recipients.contains(&sender) {
          return Err(GGError::InvalidFrame("conference includes endpoint"));
        }
        return Ok(Some(GGPacket::RecvMessage(GGRecvMessage {
          recipients,
          sender,
          seq,
          time,
          class,
          message,
          formatting,
        })));
      }

      packet_type::GG_SEND_MSG_ACK => {
        let status_raw = src.get_u32_le();
        let status = AckStatus::try_from(status_raw)
          .map_err(|_| GGError::UnsupportedPacketType(status_raw))?;
        let recipient = src.get_u32_le();
        let seq = src.get_u32_le();

        return Ok(Some(GGPacket::SendMessageAck(GGSendMessageAck {
          status,
          recipient,
          seq,
        })));
      }

      _ => Ok(Some(GGPacket::Unknown { packet_type })),
    }
  }
}

fn decode_message_body(
  src: &mut BytesMut,
) -> Result<
  (
    String,
    Option<Vec<crate::packets::RichTextFormat>>,
    Vec<u32>,
    Option<ImageTransfer>,
  ),
  GGError,
> {
  let end = src
    .iter()
    .position(|b| *b == 0)
    .ok_or(GGError::InvalidFrame("unterminated message"))?;
  let message = decode_cp1250(&src[..end]);
  src.advance(end + 1);
  if matches!(src.first(), Some(4..=6)) {
    if !message.is_empty() {
      return Err(GGError::InvalidFrame("image transfer must have empty text"));
    }
    return Ok((message, None, vec![], Some(ImageTransfer::decode(src)?)));
  }
  let mut recipients = vec![];
  let mut formatting = None;
  let mut seen_conference = false;
  let mut seen_formatting = false;
  while let Some(flag) = src.first().copied() {
    match flag {
      1 => {
        if seen_conference || src.len() < 5 {
          return Err(GGError::InvalidFrame("invalid conference block"));
        }
        let count = u32::from_le_bytes(src[1..5].try_into().unwrap()) as usize;
        if count > MAX_CONFERENCE_RECIPIENTS || src.len() < 5 + count * 4 {
          return Err(GGError::InvalidFrame("invalid conference length"));
        }
        recipients = decode_conference(&src.split_to(5 + count * 4))?;
        seen_conference = true;
      }
      2 => {
        if seen_formatting || src.len() < 3 {
          return Err(GGError::InvalidFrame("invalid rich text block"));
        }
        let length = u16::from_le_bytes(src[1..3].try_into().unwrap()) as usize + 3;
        if src.len() < length {
          return Err(GGError::InvalidFrame("invalid rich text length"));
        }
        let mut block = src.split_to(length);
        let formats = decode_richtext_formatting(&mut block, length)
          .ok_or(GGError::InvalidFrame("invalid rich text attributes"))?;
        if !formats.is_empty() {
          formatting = Some(formats);
        }
        seen_formatting = true;
      }
      4..=6 => {
        return Err(GGError::InvalidFrame(
          "image control cannot carry conversation attributes",
        ));
      }
      _ => break, // Unknown extensions have no length contract; never guess the next block boundary.
    }
  }
  Ok((message, formatting, recipients, None))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::packets::{ContactEntry, ContactStatus, ContactType};
  use claims::{assert_ok, assert_ok_eq};
  use std::net::Ipv4Addr;

  #[test]
  fn it_handles_welcome_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    assert_ok!(codec.encode(GGPacket::Welcome { seed: 5000 }, &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let packet = codec.decode(&mut output.clone());
    assert_ok_eq!(packet, Some(GGPacket::Welcome { seed: 5000 }));
  }

  #[test]
  fn it_handles_login60_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    let login = GGLogin60::login(123456, 33, "my password");
    assert_ok!(codec.encode(GGPacket::Login60(login.clone()), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let packet = codec.decode(&mut output.clone());
    assert_ok_eq!(packet, Some(GGPacket::Login60(login)));
  }

  #[test]
  fn it_handles_login_ok_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    assert_ok!(codec.encode(GGPacket::LoginOk, &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let packet = codec.decode(&mut output.clone());
    assert_ok_eq!(packet, Some(GGPacket::LoginOk));
  }

  #[test]
  fn it_handles_login_failed_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    assert_ok!(codec.encode(GGPacket::LoginFailed, &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let packet = codec.decode(&mut output.clone());
    assert_ok_eq!(packet, Some(GGPacket::LoginFailed));
  }

  #[test]
  fn it_handles_ping_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    assert_ok!(codec.encode(GGPacket::Ping, &mut output));
    let packet = codec.decode(&mut output.clone());
    assert_ok_eq!(packet, Some(GGPacket::Ping));
  }

  #[test]
  fn it_handles_pong_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    assert_ok!(codec.encode(GGPacket::Pong, &mut output));
    let packet = codec.decode(&mut output.clone());
    assert_ok_eq!(packet, Some(GGPacket::Pong));
  }

  #[test]
  fn it_handles_disconnect_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::client(); // Must use client mode to decode 0x000b as Disconnect

    assert_ok!(codec.encode(GGPacket::Disconnect, &mut output));
    let packet = codec.decode(&mut output.clone());
    assert_ok_eq!(packet, Some(GGPacket::Disconnect));
  }

  #[test]
  fn it_handles_empty_list_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    assert_ok!(codec.encode(GGPacket::ListEmpty, &mut output));
    let packet = codec.decode(&mut output.clone());
    assert_ok_eq!(packet, Some(GGPacket::ListEmpty));
  }

  #[test]
  fn it_handles_notify_first_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    let entries = vec![
      ContactEntry {
        uin: 1000,
        user_type: ContactType::Buddy,
      },
      ContactEntry {
        uin: 2000,
        user_type: ContactType::Friend,
      },
    ];
    let packet = GGPacket::NotifyFirst(entries.clone());

    assert_ok!(codec.encode(packet.clone(), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let decoded = codec.decode(&mut output.clone());
    assert_ok_eq!(decoded, Some(packet));
  }

  #[test]
  fn it_handles_notify_last_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    let entries = vec![ContactEntry {
      uin: 3000,
      user_type: ContactType::Blocked,
    }];
    let packet = GGPacket::NotifyLast(entries.clone());

    assert_ok!(codec.encode(packet.clone(), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let decoded = codec.decode(&mut output.clone());
    assert_ok_eq!(decoded, Some(packet));
  }

  #[test]
  fn it_handles_notify_reply60_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    let statuses = vec![
      // Entry WITH description - status must be a "with description" type (0x04)
      ContactStatus {
        uin: 1000,
        flags: 0x40,  // Voice
        status: 0x04, // Available WITH description (GG_STATUS_AVAIL_DESCR)
        remote_ip: Ipv4Addr::new(192, 168, 1, 100),
        remote_port: 1550,
        version: 0x20,
        image_size: 64,
        description: Some("Hello!".to_string()),
        time: Some(1234567890),
      },
      // Entry WITHOUT description - status must be a "without description" type
      ContactStatus {
        uin: 2000,
        flags: 0,
        status: 0x01, // Not available (GG_STATUS_NOT_AVAIL)
        remote_ip: Ipv4Addr::new(0, 0, 0, 0),
        remote_port: 0,
        version: 0x20,
        image_size: 0,
        description: None,
        time: None,
      },
    ];
    let packet = GGPacket::NotifyReply60(statuses);

    assert_ok!(codec.encode(packet.clone(), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let decoded = codec.decode(&mut output.clone());
    assert_ok_eq!(decoded, Some(packet));
  }

  #[test]
  fn it_handles_status60_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    let status = ContactStatus {
      uin: 12345,
      flags: 0x40,  // Voice
      status: 0x04, // Available WITH description
      remote_ip: Ipv4Addr::new(192, 168, 1, 50),
      remote_port: 1550,
      version: 0x20,
      image_size: 64,
      description: Some("Working".to_string()),
      time: Some(1234567890),
    };
    let packet = GGPacket::Status60(status);

    assert_ok!(codec.encode(packet.clone(), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    // Status60 decodes as NotifyFirst due to shared packet type 0x000f
    // This is expected - they're distinguished by direction in practice
  }

  #[test]
  fn it_handles_new_status_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    let new_status = NewStatus {
      status: GGStatus::AvailDescr,
      description: Some("Hello world!".to_string()),
      time: Some(1234567890),
    };
    let packet = GGPacket::NewStatus(new_status);

    assert_ok!(codec.encode(packet.clone(), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let decoded = codec.decode(&mut output.clone());
    assert_ok_eq!(decoded, Some(packet));
  }

  #[test]
  fn it_handles_new_status_packet_without_description() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    let new_status = NewStatus {
      status: GGStatus::Avail,
      description: None,
      time: None,
    };
    let packet = GGPacket::NewStatus(new_status);

    assert_ok!(codec.encode(packet.clone(), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let decoded = codec.decode(&mut output.clone());
    assert_ok_eq!(decoded, Some(packet));
  }

  #[test]
  fn it_handles_send_message_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::server();

    let msg = GGSendMessage {
      recipients: Vec::new(),
      recipient: 12345,
      seq: 1,
      class: GGMessageClass::Chat,
      message: "Hello!".to_string(),
      formatting: None,
    };
    let packet = GGPacket::SendMessage(msg);

    assert_ok!(codec.encode(packet.clone(), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let decoded = codec.decode(&mut output.clone());
    assert_ok_eq!(decoded, Some(packet));
  }

  #[test]
  fn it_handles_recv_message_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    let msg = GGRecvMessage {
      recipients: Vec::new(),
      sender: 54321,
      seq: 42,
      time: 1234567890,
      class: GGMessageClass::Msg,
      message: "Hi there!".to_string(),
      formatting: None,
    };
    let packet = GGPacket::RecvMessage(msg);

    assert_ok!(codec.encode(packet.clone(), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let decoded = codec.decode(&mut output.clone());
    assert_ok_eq!(decoded, Some(packet));
  }

  #[test]
  fn it_handles_send_message_ack_packet() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    let ack = GGSendMessageAck {
      status: AckStatus::Delivered,
      recipient: 12345,
      seq: 1,
    };
    let packet = GGPacket::SendMessageAck(ack);

    assert_ok!(codec.encode(packet.clone(), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let decoded = codec.decode(&mut output.clone());
    assert_ok_eq!(decoded, Some(packet));
  }

  #[test]
  fn it_handles_send_message_with_polish_chars() {
    let mut output = BytesMut::new();
    let mut codec = GGCodec::server();

    let msg = GGSendMessage {
      recipients: Vec::new(),
      recipient: 12345,
      seq: 2,
      class: GGMessageClass::Chat,
      message: "Zażółć gęślą jaźń".to_string(),
      formatting: None,
    };
    let packet = GGPacket::SendMessage(msg);

    assert_ok!(codec.encode(packet.clone(), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let decoded = codec.decode(&mut output.clone());
    assert_ok_eq!(decoded, Some(packet));
  }

  #[test]
  fn it_decodes_0x000b_as_disconnect_in_client_mode() {
    // In client mode, 0x000b should decode as Disconnect (from server)
    let mut codec = GGCodec::client();

    // Create a minimal 0x000b packet with no payload
    let mut input = BytesMut::new();
    input.put_u32_le(0x000b); // packet type
    input.put_u32_le(0); // length

    let decoded = codec.decode(&mut input);
    assert_ok_eq!(decoded, Some(GGPacket::Disconnect));
  }

  #[test]
  fn it_decodes_0x000b_as_send_message_in_server_mode() {
    // In server mode, 0x000b should decode as SendMessage (from client)
    let mut codec = GGCodec::server();

    // Create a 0x000b packet with message payload
    let mut input = BytesMut::new();
    input.put_u32_le(0x000b); // packet type
    input.put_u32_le(20); // length: 4 + 4 + 4 + 6 (message) + 1 (null) + 1 (extra)
    input.put_u32_le(12345); // recipient
    input.put_u32_le(1); // seq
    input.put_u32_le(0x0008); // class = Chat
    input.put_slice(b"Hello\0"); // message with null terminator
    input.put_slice(&[0, 0]); // padding to reach length 20

    let decoded = codec.decode(&mut input);
    let expected = GGPacket::SendMessage(GGSendMessage {
      recipients: Vec::new(),
      recipient: 12345,
      seq: 1,
      class: GGMessageClass::Chat,
      message: "Hello".to_string(),
      formatting: None,
    });
    assert_ok_eq!(decoded, Some(expected));
  }

  #[test]
  fn it_handles_send_message_with_bold_formatting() {
    use crate::packets::RichTextFormat;

    let mut output = BytesMut::new();
    let mut codec = GGCodec::server();

    // Message "Hello" with bold formatting at position 0
    let msg = GGSendMessage {
      recipients: Vec::new(),
      recipient: 12345,
      seq: 1,
      class: GGMessageClass::Chat,
      message: "Hello".to_string(),
      formatting: Some(vec![RichTextFormat::bold(0)]),
    };
    let packet = GGPacket::SendMessage(msg);

    assert_ok!(codec.encode(packet.clone(), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let decoded = codec.decode(&mut output.clone());
    assert_ok_eq!(decoded, Some(packet));
  }

  #[test]
  fn it_handles_recv_message_with_multiple_formats() {
    use crate::packets::{RgbColor, RichTextFormat};

    let mut output = BytesMut::new();
    let mut codec = GGCodec::default();

    // Message "Hello World" with bold "Hello" and colored "World"
    let msg = GGRecvMessage {
      recipients: Vec::new(),
      sender: 54321,
      seq: 42,
      time: 1234567890,
      class: GGMessageClass::Chat,
      message: "Hello World".to_string(),
      formatting: Some(vec![
        RichTextFormat::bold(0), // Bold starts at 0
        RichTextFormat::new(5),  // Normal at 5 (space)
        RichTextFormat {
          // Colored at 6 ("World")
          position: 6,
          bold: false,
          italic: false,
          underline: false,
          color: Some(RgbColor::new(255, 0, 0)),
          image: None,
        },
      ]),
    };
    let packet = GGPacket::RecvMessage(msg);

    assert_ok!(codec.encode(packet.clone(), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let decoded = codec.decode(&mut output.clone());
    assert_ok_eq!(decoded, Some(packet));
  }

  #[test]
  fn it_handles_message_with_italic_and_underline() {
    use crate::packets::RichTextFormat;

    let mut output = BytesMut::new();
    let mut codec = GGCodec::server();

    let msg = GGSendMessage {
      recipients: Vec::new(),
      recipient: 12345,
      seq: 1,
      class: GGMessageClass::Msg,
      message: "Styled text".to_string(),
      formatting: Some(vec![RichTextFormat {
        position: 0,
        bold: true,
        italic: true,
        underline: true,
        color: None,
        image: None,
      }]),
    };
    let packet = GGPacket::SendMessage(msg);

    assert_ok!(codec.encode(packet.clone(), &mut output));
    insta::assert_binary_snapshot!(".packet", output.to_vec());

    let decoded = codec.decode(&mut output.clone());
    assert_ok_eq!(decoded, Some(packet));
  }
}
