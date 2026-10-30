use bytes::{BufMut, BytesMut};
use gg_protocol::{
  GGCodec, GGPacket,
  consts::{GGMessageClass, GGStatus},
  packets::GGSendMessage,
};
use tokio_util::codec::{Decoder, Encoder};

fn frame(kind: u32, data: &[u8]) -> BytesMut {
  let mut bytes = BytesMut::new();
  bytes.put_u32_le(kind);
  bytes.put_u32_le(data.len() as u32);
  bytes.extend_from_slice(data);
  bytes
}
#[test]
fn short_and_arbitrary_frames_never_panic() {
  for client in [false, true] {
    for kind in 0..256 {
      for length in 0..64 {
        let data: Vec<_> = (0..length).map(|n| ((n * 31 + kind) & 255) as u8).collect();
        let mut codec = if client {
          GGCodec::client()
        } else {
          GGCodec::server()
        };
        let _ = codec.decode(&mut frame(kind, &data));
      }
    }
  }
}
#[test]
fn frame_length_is_rejected_before_allocating_payload() {
  let mut bytes = BytesMut::from(&[0x15, 0, 0, 0, 255, 255, 255, 255][..]);
  assert!(GGCodec::server().decode(&mut bytes).is_err());
}
#[test]
fn extensions_and_trailing_bytes_cannot_consume_the_next_frame() {
  let mut codec = GGCodec::server();
  let mut bytes = frame(0x999, b"extension");
  bytes.extend(frame(8, &[]));
  assert_eq!(
    codec.decode(&mut bytes).unwrap(),
    Some(GGPacket::Unknown { packet_type: 0x999 })
  );
  assert_eq!(codec.decode(&mut bytes).unwrap(), Some(GGPacket::Ping));
  let mut bytes = frame(2, &[2, 0, 0, 0, 0, 123]);
  bytes.extend(frame(8, &[]));
  assert!(matches!(
    codec.decode(&mut bytes).unwrap(),
    Some(GGPacket::NewStatus(_))
  ));
  assert_eq!(codec.decode(&mut bytes).unwrap(), Some(GGPacket::Ping));
}
#[test]
fn queued_chat_flags_and_friends_only_are_preserved() {
  let class = GGMessageClass::try_from(9).unwrap();
  let message = GGPacket::SendMessage(GGSendMessage {
    recipient: 1,
    seq: 2,
    class,
    message: "test".into(),
    formatting: None,
  });
  let mut bytes = BytesMut::new();
  GGCodec::client()
    .encode(message.clone(), &mut bytes)
    .unwrap();
  assert_eq!(GGCodec::server().decode(&mut bytes).unwrap(), Some(message));
  let status = GGStatus::try_from(0x8002).unwrap();
  assert!(status.friends_only());
  assert_eq!(status.base(), GGStatus::Avail);
}
#[test]
fn userlist_and_notify_packets_are_direction_specific() {
  let mut bytes = frame(0x10, &[6]);
  assert_eq!(
    GGCodec::client().decode(&mut bytes).unwrap(),
    Some(GGPacket::UserListReply {
      kind: 6,
      data: vec![]
    })
  );
  assert!(GGCodec::server().decode(&mut frame(0x10, &[6])).is_err());
  assert!(matches!(
    GGCodec::server()
      .decode(&mut frame(0x0d, &[1, 0, 0, 0, 3]))
      .unwrap(),
    Some(GGPacket::AddNotify(_))
  ));
}
