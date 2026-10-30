//! Protocol error types.

use crate::packets::GGPacket;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GGError {
  #[error("Invalid GG frame: {0}")]
  InvalidFrame(&'static str),
  #[error("IO error: {0}")]
  Io(#[from] std::io::Error),
  #[error("Unsupported packet type: {0}")]
  UnsupportedPacketType(u32),
  #[error("Unsupported packet: {0:?}")]
  UnsupportedPacket(GGPacket),
}
