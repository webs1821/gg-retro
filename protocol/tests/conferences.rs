use bytes::{BufMut, BytesMut};
use gg_protocol::{GGCodec, GGPacket, consts::GGMessageClass, packets::*};
use tokio_util::codec::{Decoder, Encoder};

fn frame(kind: u32, body: &[u8]) -> BytesMut {
  let mut b = BytesMut::new();
  b.put_u32_le(kind);
  b.put_u32_le(body.len() as u32);
  b.extend_from_slice(body);
  b
}
fn message(attrs: &[u8]) -> BytesMut {
  let mut body = BytesMut::new();
  for n in [123u32, 7, 8] {
    body.put_u32_le(n);
  }
  body.extend_from_slice(b"Test\0");
  body.extend_from_slice(attrs);
  frame(11, &body)
}
#[test]
fn conference_matches_documented_bytes_and_round_trips_both_directions() {
  let attrs = [1, 2, 0, 0, 0, 0xc8, 1, 0, 0, 0x15, 3, 0, 0]; // Other members 456 and 789.
  assert_eq!(&encode_conference(&[456, 789]).unwrap()[..], &attrs);
  let send = GGPacket::SendMessage(GGSendMessage {
    recipient: 123,
    seq: 7,
    class: GGMessageClass::Chat,
    message: "Test".into(),
    formatting: None,
    recipients: vec![456, 789],
  });
  let wire = message(&attrs);
  assert_eq!(
    GGCodec::server().decode(&mut wire.clone()).unwrap(),
    Some(send.clone())
  );
  let mut encoded = BytesMut::new();
  GGCodec::client().encode(send, &mut encoded).unwrap();
  assert_eq!(encoded, wire);
  let recv = GGPacket::RecvMessage(GGRecvMessage {
    sender: 123,
    seq: 7,
    time: 99,
    class: GGMessageClass::Chat,
    message: "Test".into(),
    formatting: None,
    recipients: vec![456, 789],
  });
  encoded.clear();
  GGCodec::server()
    .encode(recv.clone(), &mut encoded)
    .unwrap();
  assert_eq!(GGCodec::client().decode(&mut encoded).unwrap(), Some(recv));
}
#[test]
fn conference_and_image_formatting_are_independent_of_incoming_block_order() {
  let conference = encode_conference(&[456]).unwrap();
  let formats = vec![RichTextFormat {
    bold: true,
    image: Some(ImageId {
      size: 1234,
      crc32: 4567,
    }),
    ..Default::default()
  }];
  let formatting: Vec<u8> = RichTextFormats(formats.clone()).into();
  for attrs in [
    [conference.as_ref(), formatting.as_slice()].concat(),
    [formatting.as_slice(), conference.as_ref()].concat(),
  ] {
    let Some(GGPacket::SendMessage(decoded)) =
      GGCodec::server().decode(&mut message(&attrs)).unwrap()
    else {
      panic!("wrong packet");
    };
    assert_eq!(decoded.recipients, vec![456]);
    assert_eq!(decoded.formatting, Some(formats.clone()));
    let mut encoded = BytesMut::new();
    GGCodec::client()
      .encode(GGPacket::SendMessage(decoded), &mut encoded)
      .unwrap();
    assert_eq!(
      encoded,
      message(&[conference.as_ref(), formatting.as_slice()].concat())
    );
  }
}
#[test]
fn malformed_lists_cannot_escape_the_current_frame() {
  let good = encode_conference(&[456, 789]).unwrap();
  let mut malformed: Vec<Vec<u8>> = (1..good.len()).map(|n| good[..n].to_vec()).collect();
  malformed.push(vec![1, 255, 255, 255, 255]);
  malformed.push([good.as_ref(), good.as_ref()].concat());
  malformed.push([vec![1, 1, 0, 0, 0], 0u32.to_le_bytes().to_vec()].concat());
  malformed.push([vec![1, 1, 0, 0, 0], 0x01000000u32.to_le_bytes().to_vec()].concat());
  malformed.push(
    [
      vec![1, 2, 0, 0, 0],
      456u32.to_le_bytes().to_vec(),
      456u32.to_le_bytes().to_vec(),
    ]
    .concat(),
  );
  malformed.push(encode_conference(&[123]).unwrap().to_vec()); // Includes destination.
  for attrs in malformed {
    let mut wire = message(&attrs);
    wire.extend(frame(8, &[]));
    assert!(GGCodec::server().decode(&mut wire).is_err());
    assert_eq!(
      GGCodec::server().decode(&mut wire).unwrap(),
      Some(GGPacket::Ping)
    );
  }
}
#[test]
fn limits_and_empty_group_are_explicit() {
  assert!(encode_conference(&(1..=18).collect::<Vec<_>>()).is_ok());
  assert!(encode_conference(&(1..=19).collect::<Vec<_>>()).is_err());
  let Some(GGPacket::SendMessage(decoded)) = GGCodec::server()
    .decode(&mut message(&[1, 0, 0, 0, 0]))
    .unwrap()
  else {
    panic!("wrong packet");
  };
  assert!(decoded.recipients.is_empty());
  let conference = encode_conference(&[456]).unwrap();
  let mut control = vec![4];
  control.extend(5u32.to_le_bytes());
  control.extend(6u32.to_le_bytes());
  assert!(
    GGCodec::server()
      .decode(&mut message(
        &[conference.as_ref(), control.as_slice()].concat()
      ))
      .is_err()
  );
}
