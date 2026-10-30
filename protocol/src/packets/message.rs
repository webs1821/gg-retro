//! Message packets for GG protocol.
//!
//! These packets are used to send and receive messages between users.
//! Supports rich text formatting, colors, and inline image descriptors.

use super::ImageId;
use crate::codec_helpers::{decode_richtext_formatting, encode_richtext_formatting};
use crate::consts::{AckStatus, GGMessageClass, GGNumber, font};
use bytes::BytesMut;

/// RGB color for text formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RgbColor {
  pub r: u8,
  pub g: u8,
  pub b: u8,
}

impl RgbColor {
  pub fn new(r: u8, g: u8, b: u8) -> Self {
    Self { r, g, b }
  }
}

/// A single rich text format entry describing text attributes at a position.
///
/// Each entry specifies formatting that applies from `position` until the next
/// entry or end of text. The position is a 0-based byte offset in the message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RichTextFormat {
  /// Position in text (0-based byte offset) where this format starts.
  pub position: u16,
  /// Bold text.
  pub bold: bool,
  /// Italic text.
  pub italic: bool,
  /// Underlined text.
  pub underline: bool,
  /// Text color (if Some, the color is applied).
  pub color: Option<RgbColor>,
  /// Inline image descriptor; image data travels in separate messages.
  pub image: Option<ImageId>,
}

impl RichTextFormat {
  /// Create a new format entry at the given position with no formatting.
  pub fn new(position: u16) -> Self {
    Self {
      position,
      ..Default::default()
    }
  }

  /// Create a format entry with bold text.
  pub fn bold(position: u16) -> Self {
    Self {
      position,
      bold: true,
      ..Default::default()
    }
  }

  /// Create a format entry with italic text.
  pub fn italic(position: u16) -> Self {
    Self {
      position,
      italic: true,
      ..Default::default()
    }
  }

  /// Create a format entry with underlined text.
  pub fn underline(position: u16) -> Self {
    Self {
      position,
      underline: true,
      ..Default::default()
    }
  }

  /// Create a format entry with colored text.
  pub fn colored(position: u16, r: u8, g: u8, b: u8) -> Self {
    Self {
      position,
      color: Some(RgbColor::new(r, g, b)),
      ..Default::default()
    }
  }

  /// Convert font attribute flags to a RichTextFormat.
  pub fn from_font_byte(position: u16, font_byte: u8, color: Option<RgbColor>) -> Self {
    Self {
      position,
      bold: (font_byte & font::GG_FONT_BOLD) != 0,
      italic: (font_byte & font::GG_FONT_ITALIC) != 0,
      underline: (font_byte & font::GG_FONT_UNDERLINE) != 0,
      color,
      image: None,
    }
  }

  /// Convert to font attribute byte.
  pub fn to_font_byte(&self) -> u8 {
    let mut byte = 0u8;
    if self.bold {
      byte |= font::GG_FONT_BOLD;
    }
    if self.italic {
      byte |= font::GG_FONT_ITALIC;
    }
    if self.underline {
      byte |= font::GG_FONT_UNDERLINE;
    }
    if self.color.is_some() {
      byte |= font::GG_FONT_COLOR;
    }
    if self.image.is_some() {
      byte |= font::GG_FONT_IMAGE;
    }
    byte
  }

  /// Three base bytes, optionally three color bytes and ten image bytes.
  pub fn encoded_size(&self) -> usize {
    3 + if self.color.is_some() { 3 } else { 0 } + if self.image.is_some() { 10 } else { 0 }
  }
}

/// Collection of rich text format entries with serialization support.
///
/// This newtype wrapper enables `From`/`TryFrom` trait implementations
/// for converting between `Vec<RichTextFormat>` and `Vec<u8>`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RichTextFormats(pub Vec<RichTextFormat>);

impl RichTextFormats {
  pub fn new() -> Self {
    Self(Vec::new())
  }

  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }
}

impl From<Vec<RichTextFormat>> for RichTextFormats {
  fn from(formats: Vec<RichTextFormat>) -> Self {
    Self(formats)
  }
}

impl From<RichTextFormats> for Vec<RichTextFormat> {
  fn from(formats: RichTextFormats) -> Self {
    formats.0
  }
}

/// Serialize to protocol bytes.
impl From<RichTextFormats> for Vec<u8> {
  fn from(formats: RichTextFormats) -> Self {
    if formats.is_empty() {
      Vec::new()
    } else {
      encode_richtext_formatting(&formats.0).to_vec()
    }
  }
}

/// Serialize from slice reference.
impl From<&[RichTextFormat]> for RichTextFormats {
  fn from(formats: &[RichTextFormat]) -> Self {
    Self(formats.to_vec())
  }
}

/// Deserialize from protocol bytes.
impl TryFrom<&[u8]> for RichTextFormats {
  type Error = ();

  fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
    if data.is_empty() {
      return Ok(Self::new());
    }
    let mut buf = BytesMut::from(data);
    decode_richtext_formatting(&mut buf, data.len())
      .map(Self)
      .ok_or(())
  }
}

/// Send message packet (C → S).
#[derive(Debug, Clone, PartialEq)]
pub struct GGSendMessage {
  /// Recipient user number.
  pub recipient: GGNumber,
  /// Sequence number for tracking acknowledgments.
  pub seq: u32,
  /// Message class/type.
  pub class: GGMessageClass,
  /// Message content.
  pub message: String,
  /// Rich text formatting (optional).
  pub formatting: Option<Vec<RichTextFormat>>,
}

/// Receive message packet (S → C).
#[derive(Debug, Clone, PartialEq)]
pub struct GGRecvMessage {
  /// Sender user number.
  pub sender: GGNumber,
  /// Sequence number.
  pub seq: u32,
  /// Unix timestamp (UTC) of when a message was sent.
  pub time: u32,
  /// Message class/type.
  pub class: GGMessageClass,
  /// Message content.
  pub message: String,
  /// Rich text formatting (optional).
  pub formatting: Option<Vec<RichTextFormat>>,
}

/// Message acknowledgment packet (S → C).
#[derive(Debug, Clone, PartialEq)]
pub struct GGSendMessageAck {
  /// Delivery status.
  pub status: AckStatus,
  /// Recipient user number.
  pub recipient: GGNumber,
  /// Sequence number (matches the sent message).
  pub seq: u32,
}
