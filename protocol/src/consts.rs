//! Protocol constants for GG 6.0.

/// Packet type constants for GG protocol headers.
pub mod packet_type {
  /// Welcome packet (S → C) - contains seed for password hash (1).
  pub const GG_WELCOME: u32 = 0x0001;
  /// Login packet (C → S) - GG 6.0 authentication (21).
  pub const GG_LOGIN60: u32 = 0x0015;
  /// Login success (S → C) (3).
  pub const GG_LOGIN_OK: u32 = 0x0003;
  /// Login failure (S → C) (9).
  pub const GG_LOGIN_FAILED: u32 = 0x0009;
  /// Ping (C → S) - keep connection alive (8).
  pub const GG_PING: u32 = 0x0008;
  /// Pong (S → C) - response to ping (7).
  pub const GG_PONG: u32 = 0x0007;
  /// Disconnecting (S → C) - server closing connection (11).
  pub const GG_DISCONNECTING: u32 = 0x000b;
  /// Empty contact list (C → S) - no contacts to sync (18).
  pub const GG_LIST_EMPTY: u32 = 0x0012;
  /// First chunk of contact list (C → S) (15).
  pub const GG_NOTIFY_FIRST: u32 = 0x000f;
  /// Last chunk of contact list (C → S) (16).
  pub const GG_NOTIFY_LAST: u32 = 0x0010;
  /// Contact status reply (S → C) (17).
  pub const GG_NOTIFY_REPLY60: u32 = 0x0011;
  /// Single contact status change (S → C) (15).
  pub const GG_STATUS60: u32 = 0x000f;
  /// Change own status (C → S) (2).
  pub const GG_NEW_STATUS: u32 = 0x0002;
  /// Send message (C → S) (11). Note: same value as GG_DISCONNECTING, direction-dependent.
  pub const GG_SEND_MSG: u32 = 0x000b;
  /// Receive message (S → C) (10).
  pub const GG_RECV_MSG: u32 = 0x000a;
  /// Message acknowledgment (S → C) (5).
  pub const GG_SEND_MSG_ACK: u32 = 0x0005;
}

pub type GGNumber = u32;
/// Maximum UIN supported by GG 6.0 protocol (24 bits, upper 8 bits reserved for flags)
pub const GG60_MAX_UIN: GGNumber = 0x00FFFFFF; // 16,777,215

/// Friends only mask - OR with status to enable "friends only" mode.
pub const STATUS_FRIENDS_MASK: u32 = 0x8000;

/// Wire bit field: combinations must survive decoding and forwarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GGStatus(u32);
#[allow(non_upper_case_globals)]
impl GGStatus {
  pub const NotAvail: Self = Self(1);
  pub const NotAvailDescr: Self = Self(0x15);
  pub const Avail: Self = Self(2);
  pub const AvailDescr: Self = Self(4);
  pub const Busy: Self = Self(3);
  pub const BusyDescr: Self = Self(5);
  pub const Invisible: Self = Self(0x14);
  pub const InvisibleDescr: Self = Self(0x16);
  pub const Blocked: Self = Self(6);
  pub fn bits(self) -> u32 {
    self.0
  }

  pub fn base(self) -> Self {
    Self(self.0 & !0x8000)
  }
  pub fn friends_only(self) -> bool {
    self.0 & 0x8000 != 0
  }
  pub fn has_description(self) -> bool {
    status_has_description(self.0 as u8)
  }
}
impl Default for GGStatus {
  fn default() -> Self {
    Self::NotAvail
  }
}
impl TryFrom<u32> for GGStatus {
  type Error = u32;
  fn try_from(value: u32) -> Result<Self, Self::Error> {
    if matches!(value & !0x8000, 1..=6 | 0x14..=0x16) {
      Ok(Self(value))
    } else {
      Err(value)
    }
  }
}
pub fn status_has_description(status: u8) -> bool {
  matches!(status, 0x04 | 0x05 | 0x15 | 0x16)
}

/// Client version constants for GG 6.0.
pub mod version {
  /// GG 6.0 client version.
  pub const GG_VERSION_60: u32 = 0x20;
  /// GG 5.7 beta (build 121).
  pub const GG_VERSION_57_BETA: u32 = 0x1e;
  /// GG 5.0.5 client version.
  pub const GG_VERSION_505: u32 = 0x1b;
  /// GG 5.0.3 client version.
  pub const GG_VERSION_503: u32 = 0x19;
  /// Add this mask if client supports voice calls.
  pub const GG_HAS_AUDIO_MASK: u32 = 0x40000000;
}

/// Wire bit field: combinations must survive decoding and forwarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GGMessageClass(u32);
#[allow(non_upper_case_globals)]
impl GGMessageClass {
  pub const Queued: Self = Self(1);
  pub const Msg: Self = Self(4);
  pub const Chat: Self = Self(8);
  pub const Ctcp: Self = Self(16);
  pub const Ack: Self = Self(32);
  pub fn bits(self) -> u32 {
    self.0
  }
}
impl Default for GGMessageClass {
  fn default() -> Self {
    Self::Msg
  }
}
impl TryFrom<u32> for GGMessageClass {
  type Error = u32;
  fn try_from(value: u32) -> Result<Self, Self::Error> {
    if value & !0x3d == 0 {
      Ok(Self(value))
    } else {
      Err(value)
    }
  }
}
/// Font attribute flags for rich text messages.
/// These can be combined (bitwise OR) except GG_FONT_IMAGE which is special.
pub mod font {
  /// Bold text.
  pub const GG_FONT_BOLD: u8 = 0x01;
  /// Italic text.
  pub const GG_FONT_ITALIC: u8 = 0x02;
  /// Underlined text.
  pub const GG_FONT_UNDERLINE: u8 = 0x04;
  /// Colored text - when set, 3 bytes RGB follow the font byte.
  pub const GG_FONT_COLOR: u8 = 0x08;
  /// Image marker - used for inline images.
  pub const GG_FONT_IMAGE: u8 = 0x80;
}

/// Message acknowledgment status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AckStatus {
  /// Message blocked by recipient.
  Blocked = 0x0001,
  /// Message delivered.
  Delivered = 0x0002,
  /// Message queued for offline user.
  Queued = 0x0003,
  /// Recipient mailbox full.
  MboxFull = 0x0004,
  /// Message not delivered.
  NotDelivered = 0x0006,
}

impl TryFrom<u32> for AckStatus {
  type Error = u32;

  fn try_from(value: u32) -> Result<Self, Self::Error> {
    match value {
      0x0001 => Ok(AckStatus::Blocked),
      0x0002 => Ok(AckStatus::Delivered),
      0x0003 => Ok(AckStatus::Queued),
      0x0004 => Ok(AckStatus::MboxFull),
      0x0006 => Ok(AckStatus::NotDelivered),
      _ => Err(value),
    }
  }
}
