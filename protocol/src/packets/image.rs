//! GG6 inline image options inside SEND_MSG / RECV_MSG, not DCC.
use crate::{GGError, GGNumber, consts::GGMessageClass};
use bytes::{BufMut, BytesMut};

pub const MAX_IMAGE_BYTES: u32 = 255 * 1024;
/// Includes the option header and filename, excludes the empty message's NUL.
pub const MAX_IMAGE_OPTION_BYTES: usize = 1909;
pub const MAX_IMAGE_FILENAME_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageId {
  pub size: u32,
  pub crc32: u32,
}
impl ImageId {
  pub fn valid(self) -> bool {
    self.size > 0 && self.size <= MAX_IMAGE_BYTES
  }
}

#[derive(Clone, PartialEq, Eq)]
pub enum ImageTransfer {
  Request(ImageId),
  /// A first reply without filename/data means that the image is unavailable.
  Reply {
    id: ImageId,
    first: bool,
    filename: Option<Vec<u8>>,
    data: Vec<u8>,
  },
}
impl std::fmt::Debug for ImageTransfer {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("ImageTransfer")
      .field("id", &self.id())
      .finish_non_exhaustive()
  }
}
impl ImageTransfer {
  pub fn id(&self) -> ImageId {
    match self {
      Self::Request(id) | Self::Reply { id, .. } => *id,
    }
  }
  pub fn unavailable(id: ImageId) -> Self {
    Self::Reply {
      id,
      first: true,
      filename: None,
      data: vec![],
    }
  }
  pub fn decode(bytes: &[u8]) -> Result<Self, GGError> {
    let invalid = || GGError::InvalidFrame("invalid inline image option");
    if !(9..=MAX_IMAGE_OPTION_BYTES).contains(&bytes.len()) {
      return Err(invalid());
    }
    let id = ImageId {
      size: u32::from_le_bytes(bytes[1..5].try_into().unwrap()),
      crc32: u32::from_le_bytes(bytes[5..9].try_into().unwrap()),
    };
    if !id.valid() {
      return Err(invalid());
    }
    match bytes[0] {
      4 if bytes.len() == 9 => Ok(Self::Request(id)),
      5 if bytes.len() == 9 => Ok(Self::unavailable(id)),
      5 => {
        let end = bytes[9..]
          .iter()
          .position(|b| *b == 0)
          .ok_or_else(invalid)?;
        if end == 0 || end > MAX_IMAGE_FILENAME_BYTES {
          return Err(invalid());
        }
        let data = &bytes[10 + end..];
        if data.len() > id.size as usize {
          return Err(invalid());
        }
        Ok(Self::Reply {
          id,
          first: true,
          filename: Some(bytes[9..9 + end].to_vec()),
          data: data.to_vec(),
        })
      }
      6 if bytes.len() > 9 && bytes.len() - 9 <= id.size as usize => Ok(Self::Reply {
        id,
        first: false,
        filename: None,
        data: bytes[9..].to_vec(),
      }),
      _ => Err(invalid()),
    }
  }
  pub fn encode(&self) -> Result<BytesMut, GGError> {
    let mut bytes = BytesMut::new();
    let id = self.id();
    bytes.put_u8(match self {
      Self::Request(_) => 4,
      Self::Reply { first: true, .. } => 5,
      _ => 6,
    });
    bytes.put_u32_le(id.size);
    bytes.put_u32_le(id.crc32);
    if let Self::Reply { filename, data, .. } = self {
      if let Some(filename) = filename {
        bytes.extend_from_slice(filename);
        bytes.put_u8(0);
      }
      bytes.extend_from_slice(data);
    }
    // Validate both directions: callers cannot emit an ambiguous filename or invalid continuation.
    if Self::decode(&bytes)? != *self {
      return Err(GGError::InvalidFrame("invalid outgoing image option"));
    }
    Ok(bytes)
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GGSendImage {
  pub recipient: GGNumber,
  pub seq: u32,
  pub class: GGMessageClass,
  pub transfer: ImageTransfer,
}
#[derive(Debug, Clone, PartialEq)]
pub struct GGRecvImage {
  pub sender: GGNumber,
  pub seq: u32,
  pub time: u32,
  pub class: GGMessageClass,
  pub transfer: ImageTransfer,
}
