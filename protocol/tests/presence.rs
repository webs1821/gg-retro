use bytes::BytesMut;
use gg_protocol::{GGCodec, GGPacket, packets::ContactStatus};
use tokio_util::codec::{Decoder, Encoder};

#[test]
fn offline_descriptions_with_and_without_time_do_not_consume_the_next_contact() {
  for description in ["a", "Wrócę wieczorem ąćęłńóśźż", &"ą".repeat(70)] {
    for time in [None, Some(0), Some(0x01000000), Some(u32::MAX)] {
      let contact = ContactStatus {
        uin: 112233,
        flags: 0,
        status: 0x15,
        remote_ip: [0, 0, 0, 0].into(),
        remote_port: 0,
        version: 0x20,
        image_size: 0,
        description: Some(description.into()),
        time,
      };
      let next = ContactStatus {
        uin: 112234,
        status: 1,
        description: None,
        time: None,
        ..contact.clone()
      };
      for packet in [
        GGPacket::NotifyReply60(vec![contact.clone(), next]),
        GGPacket::Status60(contact.clone()),
      ] {
        let mut bytes = BytesMut::new();
        GGCodec::server()
          .encode(packet.clone(), &mut bytes)
          .unwrap();
        assert_eq!(GGCodec::client().decode(&mut bytes).unwrap(), Some(packet));
        assert!(bytes.is_empty());
      }
    }
  }
}
