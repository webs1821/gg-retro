//! Helper functions for GG protocol codec encoding/decoding.

use crate::consts::{font, status_has_description};
use crate::packets::{ContactEntry, ContactStatus, ContactType, ImageId, RgbColor, RichTextFormat};
use bytes::{Buf, BufMut, BytesMut};
use encoding_rs::WINDOWS_1250;
use std::borrow::Cow;
use std::net::Ipv4Addr;

/// Decode CP1250 (Windows-1250) bytes to a Rust String.
pub fn decode_cp1250(bytes: &[u8]) -> String {
  let (decoded, _, _) = WINDOWS_1250.decode(bytes);
  decoded.into_owned()
}

/// Encode a Rust String to CP1250 (Windows-1250) bytes.
pub fn encode_cp1250(s: &str) -> Cow<'_, [u8]> {
  let (encoded, _, _) = WINDOWS_1250.encode(s);
  encoded
}

pub fn decode_contact_entries(src: &mut BytesMut, length: usize) -> Vec<ContactEntry> {
  let entry_count = length / 5; // 4 bytes uin + 1 byte type
  let mut entries = Vec::with_capacity(entry_count);

  for _ in 0..entry_count {
    let uin = src.get_u32_le();
    let type_byte = src.get_u8();
    let user_type = ContactType::try_from(type_byte).unwrap_or(ContactType::Buddy);
    entries.push(ContactEntry { uin, user_type });
  }

  entries
}

pub fn decode_contact_statuses(src: &mut BytesMut, mut length: usize) -> Vec<ContactStatus> {
  let mut statuses = Vec::new();

  while length >= 14 {
    // base size: 4 + 1 + 4 + 2 + 1 + 1 + 1 = 14 bytes
    let uin_with_flags = src.get_u32_le();
    let uin = uin_with_flags & 0x00FFFFFF;
    let flags = ((uin_with_flags >> 24) & 0xFF) as u8;

    let status = src.get_u8();
    let remote_ip = Ipv4Addr::new(src[0], src[1], src[2], src[3]);
    src.advance(4);
    let remote_port = src.get_u16_le();
    let version = src.get_u8();
    let image_size = src.get_u8();
    let _unknown1 = src.get_u8();

    length -= 14;

    // Only read description fields if status indicates description (GG_S_D equivalent)
    let (description, time) = if status_has_description(status) && length >= 1 {
      let description_size = src.get_u8() as usize;
      length -= 1;

      if description_size > 0 && length >= description_size {
        let block = src.split_to(description_size);
        // A long description alone is not a timestamp. GG6 appends NUL + u32.
        let has_time = description_size >= 5 && block[description_size - 5] == 0;
        let end = block
          .iter()
          .position(|b| *b == 0)
          .unwrap_or(description_size);
        let desc = (end > 0).then(|| decode_cp1250(&block[..end]));
        let time =
          has_time.then(|| u32::from_le_bytes(block[description_size - 4..].try_into().unwrap()));
        length -= description_size;
        (desc, time)
      } else {
        (None, None)
      }
    } else {
      (None, None)
    };

    statuses.push(ContactStatus {
      uin,
      flags,
      status,
      remote_ip,
      remote_port,
      version,
      image_size,
      description,
      time,
    });
  }

  statuses
}

/// Encode contact status for GG_NOTIFY_REPLY60 (has description_size prefix)
pub fn encode_contact_status_with_size(payload: &mut BytesMut, status: &ContactStatus) {
  // uin (24 bits) with flags in MSB (8 bits)
  let uin_with_flags = (status.uin & 0x00FFFFFF) | ((status.flags as u32) << 24);
  payload.put_u32_le(uin_with_flags);
  payload.put_u8(status.status);
  payload.put_slice(&status.remote_ip.octets());
  payload.put_u16_le(status.remote_port);
  payload.put_u8(status.version);
  payload.put_u8(status.image_size);
  payload.put_u8(0); // unknown1

  // Only write description fields if status indicates description (GG_S_D equivalent)
  if status_has_description(status.status) {
    if let Some(ref desc) = status.description {
      let desc_bytes = encode_cp1250(desc);
      // description_size = description + null terminator + time (if present)
      let desc_size = desc_bytes.len() + 1 + if status.time.is_some() { 4 } else { 0 };
      payload.put_u8(desc_size as u8);
      payload.put_slice(&desc_bytes);
      payload.put_u8(0); // null terminator
      if let Some(time) = status.time {
        payload.put_u32_le(time);
      }
    } else {
      payload.put_u8(0); // empty description
    }
  }
}

/// Encode contact status for GG_STATUS60 (no description_size, null-terminated)
pub fn encode_contact_status_no_size(payload: &mut BytesMut, status: &ContactStatus) {
  // uin (24 bits) with flags in MSB (8 bits)
  let uin_with_flags = (status.uin & 0x00FFFFFF) | ((status.flags as u32) << 24);
  payload.put_u32_le(uin_with_flags);
  payload.put_u8(status.status);
  payload.put_slice(&status.remote_ip.octets());
  payload.put_u16_le(status.remote_port);
  payload.put_u8(status.version);
  payload.put_u8(status.image_size);
  payload.put_u8(0); // unknown1

  // Only write description fields if status indicates description (GG_S_D equivalent)
  if status_has_description(status.status) {
    if let Some(ref desc) = status.description {
      let desc_bytes = encode_cp1250(desc);
      payload.put_slice(&desc_bytes);
      payload.put_u8(0); // null terminator
      if let Some(time) = status.time {
        payload.put_u32_le(time);
      }
    }
  }
}

/// Encode rich text formatting data.
/// Returns the encoded bytes (flag + length + format entries).
pub fn encode_richtext_formatting(formats: &[RichTextFormat]) -> BytesMut {
  let mut data = BytesMut::new();

  // Calculate total length of format entries
  let formats_len: usize = formats.iter().map(|f| f.encoded_size()).sum();

  // Rich text header: flag (1 byte) + length (2 bytes little-endian)
  data.put_u8(0x02); // GG_MSG_RICHTEXT flag
  data.put_u16_le(formats_len as u16);

  // Write each format entry
  for fmt in formats {
    data.put_u16_le(fmt.position);
    data.put_u8(fmt.to_font_byte());
    if let Some(ref color) = fmt.color {
      data.put_u8(color.r);
      data.put_u8(color.g);
      data.put_u8(color.b);
    }
    if let Some(image) = fmt.image {
      data.put_slice(&[9, 1]);
      data.put_u32_le(image.size);
      data.put_u32_le(image.crc32);
    }
  }

  data
}

/// Decode rich text formatting from message payload.
/// `remaining` is the number of bytes after the message null terminator.
pub fn decode_richtext_formatting(
  src: &mut BytesMut,
  remaining: usize,
) -> Option<Vec<RichTextFormat>> {
  if remaining < 3 || remaining > src.len() || src[0] != 2 {
    return None;
  }
  let length = u16::from_le_bytes([src[1], src[2]]) as usize;
  if length + 3 != remaining {
    return None;
  }
  src.advance(3);
  let mut formats = Vec::new();
  while !src.is_empty() {
    if src.len() < 3 {
      return None;
    }
    let position = src.get_u16_le();
    let flags = src.get_u8();
    if flags & !0x8f != 0 {
      return None;
    }
    let color = if flags & font::GG_FONT_COLOR != 0 {
      if src.len() < 3 {
        return None;
      }
      Some(RgbColor::new(src.get_u8(), src.get_u8(), src.get_u8()))
    } else {
      None
    };
    let mut format = RichTextFormat::from_font_byte(position, flags, color);
    if flags & font::GG_FONT_IMAGE != 0 {
      if src.len() < 10 || src[0] != 9 || src[1] != 1 {
        return None;
      }
      src.advance(2);
      let image = ImageId {
        size: src.get_u32_le(),
        crc32: src.get_u32_le(),
      };
      if !image.valid() {
        return None;
      }
      format.image = Some(image);
    }
    formats.push(format);
  }
  Some(formats)
}
