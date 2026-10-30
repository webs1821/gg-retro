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
fn body(option: &[u8]) -> Vec<u8> {
  let mut b = vec![];
  for x in [123u32, 0, 4] {
    b.extend(x.to_le_bytes());
  }
  b.push(0);
  b.extend(option);
  b
}
#[test]
fn request_matches_documented_little_endian_wire_format() {
  let option = [4, 0x10, 0x27, 0, 0, 0x78, 0x56, 0x34, 0x12];
  let expected = GGPacket::SendImage(GGSendImage {
    recipient: 123,
    seq: 0,
    class: GGMessageClass::Msg,
    transfer: ImageTransfer::Request(ImageId {
      size: 10000,
      crc32: 0x12345678,
    }),
  });
  let wire = frame(0x0b, &body(&option));
  assert_eq!(
    GGCodec::server().decode(&mut wire.clone()).unwrap(),
    Some(expected.clone())
  );
  let mut encoded = BytesMut::new();
  GGCodec::client().encode(expected, &mut encoded).unwrap();
  assert_eq!(encoded, wire);
}
#[test]
fn binary_replies_and_missing_image_round_trip_both_directions() {
  let id = ImageId {
    size: 12345,
    crc32: 0x87654321,
  };
  for transfer in [
    ImageTransfer::unavailable(id),
    ImageTransfer::Reply {
      id,
      first: true,
      filename: Some(b"test.gif".to_vec()),
      data: vec![0, 255, 128, 0, 9],
    },
    ImageTransfer::Reply {
      id,
      first: false,
      filename: None,
      data: vec![0, 255, 128, 0, 9],
    },
  ] {
    let packet = GGPacket::SendImage(GGSendImage {
      recipient: 123,
      seq: 7,
      class: GGMessageClass::Msg,
      transfer: transfer.clone(),
    });
    let mut b = BytesMut::new();
    GGCodec::client().encode(packet.clone(), &mut b).unwrap();
    assert_eq!(GGCodec::server().decode(&mut b).unwrap(), Some(packet));
    let packet = GGPacket::RecvImage(GGRecvImage {
      sender: 456,
      seq: 7,
      time: 42,
      class: GGMessageClass::Msg,
      transfer,
    });
    let mut b = BytesMut::new();
    GGCodec::server().encode(packet.clone(), &mut b).unwrap();
    assert_eq!(GGCodec::client().decode(&mut b).unwrap(), Some(packet));
  }
}
#[test]
fn image_attributes_preserve_position_color_and_following_text_format() {
  let attrs = vec![
    RichTextFormat {
      position: 3,
      bold: true,
      color: Some(RgbColor::new(1, 2, 3)),
      image: Some(ImageId {
        size: 10000,
        crc32: 0x12345678,
      }),
      ..Default::default()
    },
    RichTextFormat::italic(4),
  ];
  let wire: Vec<u8> = RichTextFormats::from(attrs.clone()).into();
  assert_eq!(
    wire,
    vec![
      2, 19, 0, 3, 0, 0x89, 1, 2, 3, 9, 1, 0x10, 0x27, 0, 0, 0x78, 0x56, 0x34, 0x12, 4, 0, 2
    ]
  );
  assert_eq!(RichTextFormats::try_from(wire.as_slice()).unwrap().0, attrs);
  for end in 1..wire.len() {
    assert!(RichTextFormats::try_from(&wire[..end]).is_err());
  }
}
#[test]
fn malformed_image_options_fail_without_consuming_next_frame() {
  let id = ImageId {
    size: 2000,
    crc32: 5,
  };
  let good = ImageTransfer::Request(id).encode().unwrap();
  let mut bad: Vec<Vec<u8>> = (1..9).map(|n| good[..n].to_vec()).collect();
  let mut extra = good.to_vec();
  extra.push(0);
  bad.push(extra);
  let mut long = vec![0; 1910];
  long[..9].copy_from_slice(&good);
  long[0] = 6;
  bad.push(long);
  let mut missing_name = good.to_vec();
  missing_name[0] = 5;
  missing_name.extend(b"no-terminator");
  bad.push(missing_name);
  let mut zero = good.to_vec();
  zero[1..5].fill(0);
  bad.push(zero);
  let mut oversized = good.to_vec();
  oversized[1..5].copy_from_slice(&(MAX_IMAGE_BYTES + 1).to_le_bytes());
  bad.push(oversized);
  for option in bad {
    let mut wire = frame(0x0b, &body(&option));
    wire.extend_from_slice(&frame(8, &[]));
    assert!(GGCodec::server().decode(&mut wire).is_err());
    assert_eq!(
      GGCodec::server().decode(&mut wire).unwrap(),
      Some(GGPacket::Ping)
    );
  }
}
#[test]
fn fragment_limits_and_empty_text_are_enforced() {
  let id = ImageId {
    size: MAX_IMAGE_BYTES,
    crc32: 1,
  };
  let option = ImageTransfer::Reply {
    id,
    first: false,
    filename: None,
    data: vec![1; 1900],
  }
  .encode()
  .unwrap();
  assert_eq!(option.len(), 1909);
  assert!(
    ImageTransfer::Reply {
      id,
      first: false,
      filename: None,
      data: vec![1; 1901]
    }
    .encode()
    .is_err()
  );
  let mut nonempty = body(&option);
  nonempty.insert(12, b'x');
  assert!(
    GGCodec::server()
      .decode(&mut frame(0x0b, &nonempty))
      .is_err()
  );
  let mut wire = frame(0x0b, &body(&option));
  let tail = wire.split_off(15);
  assert!(GGCodec::server().decode(&mut wire).unwrap().is_none());
  wire.extend_from_slice(&tail);
  assert!(matches!(
    GGCodec::server().decode(&mut wire).unwrap(),
    Some(GGPacket::SendImage(_))
  ));
}
