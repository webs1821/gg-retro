//! The GG 5.0.2/6.x session directory protocol (CP1250, NUL-delimited pairs).
use crate::error::GGError;
use encoding_rs::WINDOWS_1250;

#[derive(Clone, PartialEq, Eq)]
pub struct DirectoryPacket {
  pub kind: u8,
  pub seq: u32,
  pub records: Vec<Vec<(String, String)>>,
}
impl std::fmt::Debug for DirectoryPacket {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("DirectoryPacket")
      .field("kind", &self.kind)
      .field("seq", &self.seq)
      .field("records", &self.records.len())
      .finish()
  }
}
impl DirectoryPacket {
  pub fn decode(data: &[u8], reply: bool) -> Result<Self, GGError> {
    let bad = || GGError::InvalidFrame("invalid public directory packet");
    if data.len() < 5 || data.len() > if reply { 32768 } else { 4096 } {
      return Err(bad());
    }
    let kind = data[0];
    if !(if reply {
      matches!(kind, 1 | 2 | 5)
    } else {
      matches!(kind, 1..=3)
    }) {
      return Err(bad());
    }
    let seq = u32::from_le_bytes(data[1..5].try_into().unwrap());
    if seq == 0 {
      return Err(bad());
    }
    let mut records = Vec::new();
    let mut record: Vec<(String, String)> = Vec::new();
    let mut rest = &data[5..];
    while !rest.is_empty() {
      if rest[0] == 0 {
        if !reply || record.is_empty() || rest.len() == 1 {
          return Err(bad());
        }
        records.push(std::mem::take(&mut record));
        if records.len() >= 21 {
          return Err(bad());
        }
        rest = &rest[1..];
      }
      let end = rest.iter().position(|b| *b == 0).ok_or_else(bad)?;
      if end == 0 || end > 32 || !rest[..end].iter().all(u8::is_ascii_alphanumeric) {
        return Err(bad());
      }
      let key = String::from_utf8(rest[..end].to_vec())
        .map_err(|_| bad())?
        .to_ascii_lowercase();
      rest = &rest[end + 1..];
      let end = rest.iter().position(|b| *b == 0).ok_or_else(bad)?;
      if end > 128 || record.len() >= 16 || record.iter().any(|(k, _)| k == &key) {
        return Err(bad());
      }
      let (value, _, errors) = WINDOWS_1250.decode(&rest[..end]);
      if errors || value.chars().any(char::is_control) {
        return Err(bad());
      }
      record.push((key, value.into_owned()));
      rest = &rest[end + 1..];
    }
    if !record.is_empty() {
      records.push(record);
    }
    Ok(Self { kind, seq, records })
  }
  pub fn encode(&self, reply: bool) -> Result<Vec<u8>, GGError> {
    let mut data = vec![self.kind];
    data.extend_from_slice(&self.seq.to_le_bytes());
    for (index, record) in self.records.iter().enumerate() {
      if record.is_empty() {
        return Err(GGError::InvalidFrame("empty directory record"));
      }
      if index > 0 {
        data.push(0);
      }
      for (key, value) in record {
        data.extend_from_slice(key.as_bytes());
        data.push(0);
        let (bytes, _, errors) = WINDOWS_1250.encode(value);
        if errors || value.contains('\0') {
          return Err(GGError::InvalidFrame("invalid directory value"));
        }
        data.extend_from_slice(&bytes);
        data.push(0);
      }
    }
    Self::decode(&data, reply)?;
    Ok(data)
  }
}
