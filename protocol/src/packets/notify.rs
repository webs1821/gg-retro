//! Contact list notification packets.
//!
//! These packets are sent by the client after login to inform the server
//! about the contact list. The list is split into chunks of max 400 entries.

use crate::consts::{GGNumber, GGStatus};
use std::net::Ipv4Addr;

/// Wire bit field: combinations must survive decoding and forwarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContactType(u8);
#[allow(non_upper_case_globals)]
impl ContactType {
  pub const Buddy: Self = Self(1);
  pub const Friend: Self = Self(2);
  pub const Blocked: Self = Self(4);
  pub fn bits(self) -> u8 {
    self.0
  }
}
impl Default for ContactType {
  fn default() -> Self {
    Self::Buddy
  }
}
impl TryFrom<u8> for ContactType {
  type Error = u8;
  fn try_from(value: u8) -> Result<Self, Self::Error> {
    if value & !7 == 0 {
      Ok(Self(value))
    } else {
      Err(value)
    }
  }
}
/// Single entry in the contact list.
#[derive(Debug, Clone, PartialEq)]
pub struct ContactEntry {
  /// User number.
  pub uin: GGNumber,
  /// Type of user.
  pub user_type: ContactType,
}

pub type ContactList = Vec<ContactEntry>;

/// Flags in the most significant byte of UIN in notify reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UinFlag {
  /// Unknown flag.
  Unknown1 = 0x10,
  /// User becomes unavailable.
  Unavailable = 0x20,
  /// User can conduct voice conversations.
  Voice = 0x40,
}

/// Contact status entry in GG_NOTIFY_REPLY60.
#[derive(Debug, Clone, PartialEq)]
pub struct ContactStatus {
  /// User number (lower 24 bits).
  pub uin: GGNumber,
  /// Flags from most significant byte.
  pub flags: u8,
  /// User's current status.
  pub status: u8,
  /// IP address for direct connections.
  pub remote_ip: Ipv4Addr,
  /// Port for direct connections (0 = no direct, 1 = NAT, 2 = not in their list).
  pub remote_port: u16,
  /// Client version.
  pub version: u8,
  /// Maximum image size in KB.
  pub image_size: u8,
  /// Status description (optional).
  pub description: Option<String>,
  /// Time when status was set (optional).
  pub time: Option<u32>,
}

pub type ContactStatuses = Vec<ContactStatus>;

/// New status packet sent by client to change own status (GG_NEW_STATUS).
#[derive(Debug, Clone, PartialEq)]
pub struct NewStatus {
  /// New status value.
  pub status: GGStatus,
  /// Status description (optional, max 70 characters).
  pub description: Option<String>,
  /// Return time (optional).
  pub time: Option<u32>,
}
