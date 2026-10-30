//! GG protocol packet types.

mod image;
mod login;
pub use image::{
  GGRecvImage, GGSendImage, ImageId, ImageTransfer, MAX_IMAGE_BYTES, MAX_IMAGE_FILENAME_BYTES,
  MAX_IMAGE_OPTION_BYTES,
};
mod message;
mod notify;

pub use login::GGLogin60;
pub use message::{
  GGRecvMessage, GGSendMessage, GGSendMessageAck, MAX_CONFERENCE_RECIPIENTS, RgbColor,
  RichTextFormat, RichTextFormats, conference_valid, decode_conference, encode_conference,
};
pub use notify::{
  ContactEntry, ContactList, ContactStatus, ContactStatuses, ContactType, NewStatus, UinFlag,
};

#[derive(Debug, Clone, PartialEq)]
pub enum GGPacket {
  SendImage(GGSendImage),
  RecvImage(GGRecvImage),
  AddNotify(ContactEntry),
  RemoveNotify(ContactEntry),
  UserListRequest {
    kind: u8,
    data: Vec<u8>,
  },
  UserListReply {
    kind: u8,
    data: Vec<u8>,
  },
  /// Unsupported extension, consumed without desynchronizing the stream.
  Unknown {
    packet_type: u32,
  },
  /// Welcome packet with seed for password hashing (S → C).
  /// Sent by server immediately after client connects.
  Welcome {
    seed: u32,
  },
  /// Login packet (C → S).
  /// Sent by client to authenticate with UIN and password hash.
  Login60(GGLogin60),
  /// Login successful (S → C).
  /// Sent by server when authentication succeeds.
  LoginOk,
  /// Login failed (S → C).
  /// Sent by server when authentication fails.
  LoginFailed,
  /// Ping (C → S).
  /// Sent by client to keep connection alive (must be sent every 5 minutes).
  Ping,
  /// Pong (S → C).
  /// Sent by server in response to Ping.
  Pong,
  /// Disconnect (S → C).
  /// Sent by server before closing connection (e.g., too many failed logins).
  Disconnect,
  /// Empty contact list (C → S).
  /// Sent by client after login when contact list is empty.
  ListEmpty,
  /// First chunk of contact list (C → S).
  /// Sent when contact list has more than 400 entries.
  NotifyFirst(ContactList),
  /// Last chunk of contact list (C → S).
  /// Sent as the final packet of contact list, or only packet if <= 400 entries.
  NotifyLast(ContactList),
  /// Contact status reply (S → C).
  /// Sent by server with current status of contacts.
  NotifyReply60(ContactStatuses),
  /// Single contact status change (S → C).
  /// Sent by server when a single contact changes status.
  Status60(ContactStatus),
  /// Change own status (C → S).
  /// Sent by client to change their status.
  NewStatus(NewStatus),
  /// Send message (C → S).
  /// Sent by client to send a message to another user.
  SendMessage(GGSendMessage),
  /// Receive message (S → C).
  /// Sent by server when a message is received from another user.
  RecvMessage(GGRecvMessage),
  /// Message acknowledgment (S → C).
  /// Sent by server to acknowledge message delivery status.
  SendMessageAck(GGSendMessageAck),
}
