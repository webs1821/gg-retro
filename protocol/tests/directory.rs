use bytes::BytesMut;
use gg_protocol::{GGCodec, GGPacket, packets::DirectoryPacket};
use tokio_util::codec::{Decoder, Encoder};

#[test]
fn directory_roundtrip_fragmentation_and_contact_direction() {
  let request = DirectoryPacket {
    kind: 3,
    seq: 0x12345678,
    records: vec![vec![
      ("firstname".into(), "Łucja".into()),
      ("city".into(), "Łódź".into()),
    ]],
  };
  let mut wire = BytesMut::new();
  GGCodec::client()
    .encode(GGPacket::DirectoryRequest(request.clone()), &mut wire)
    .unwrap();
  assert_eq!(&wire[..8], &[0x14, 0, 0, 0, 31, 0, 0, 0]);
  let mut part = wire.split_to(10);
  assert!(GGCodec::server().decode(&mut part).unwrap().is_none());
  part.extend_from_slice(&wire);
  part.extend_from_slice(&[8, 0, 0, 0, 0, 0, 0, 0]);
  assert_eq!(
    GGCodec::server().decode(&mut part).unwrap(),
    Some(GGPacket::DirectoryRequest(request))
  );
  assert_eq!(
    GGCodec::server().decode(&mut part).unwrap(),
    Some(GGPacket::Ping)
  );
  let reply = DirectoryPacket {
    kind: 5,
    seq: 1,
    records: vec![
      vec![("fmnumber".into(), "123".into())],
      vec![("nextstart".into(), "0".into())],
    ],
  };
  let mut wire = BytesMut::new();
  GGCodec::server()
    .encode(GGPacket::DirectoryReply(reply.clone()), &mut wire)
    .unwrap();
  assert_eq!(&wire[..4], &[14, 0, 0, 0]);
  assert_eq!(
    GGCodec::client().decode(&mut wire).unwrap(),
    Some(GGPacket::DirectoryReply(reply))
  );
  // 0x0e in the other direction remains remove-contact, not directory.
  let mut remove = BytesMut::from(&[14, 0, 0, 0, 5, 0, 0, 0, 123, 0, 0, 0, 3][..]);
  assert!(matches!(
    GGCodec::server().decode(&mut remove).unwrap(),
    Some(GGPacket::RemoveNotify(_))
  ));
}

#[test]
fn directory_rejects_malformed_bounded_fields_without_logging_values() {
  for body in [
    b"firstname\0private".as_slice(),
    b"firstname\0x\0FirstName\0y\0",
    b"\0",
    b"x\0a\nb\0",
    b"x\0y\0\0city\0z\0",
  ] {
    let mut payload = vec![3, 1, 0, 0, 0];
    payload.extend_from_slice(body);
    assert!(DirectoryPacket::decode(&payload, false).is_err());
  }
  for payload in [vec![1, 0, 0, 0, 0], vec![9, 1, 0, 0, 0], vec![3; 4097]] {
    assert!(DirectoryPacket::decode(&payload, false).is_err());
  }
  let request = DirectoryPacket {
    kind: 1,
    seq: 1,
    records: vec![vec![("firstname".into(), "DO-NOT-LOG".into())]],
  };
  assert!(!format!("{:?}", GGPacket::DirectoryRequest(request)).contains("DO-NOT-LOG"));
  let mut frame = BytesMut::from(&[20, 0, 0, 0, 1, 16, 0, 0][..]);
  assert!(GGCodec::server().decode(&mut frame).is_err()); // Reject oversized headers immediately.
}
