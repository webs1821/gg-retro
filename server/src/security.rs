//! Protection for the legacy protocol. GG6 itself does not provide transport encryption.
use crate::models::RepositoryError;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
  XChaCha20Poly1305, XNonce,
  aead::{Aead, KeyInit, Payload},
};
use parking_lot::Mutex;
use rand::RngCore;
use std::{
  collections::HashMap,
  net::IpAddr,
  sync::Arc,
  time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub struct PasswordVault(XChaCha20Poly1305);
impl std::fmt::Debug for PasswordVault {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("PasswordVault([REDACTED])")
  }
}
impl PasswordVault {
  pub fn new(key: &[u8]) -> Result<Self, RepositoryError> {
    XChaCha20Poly1305::new_from_slice(key)
      .map(Self)
      .map_err(|_| RepositoryError::Secret)
  }
  pub fn seal(&self, password: &str, uin: u32) -> Result<String, RepositoryError> {
    let mut nonce = [0u8; 24];
    rand::rng().fill_bytes(&mut nonce);
    let encrypted = self
      .0
      .encrypt(
        &XNonce::from(nonce),
        Payload {
          msg: password.as_bytes(),
          aad: &uin.to_le_bytes(),
        },
      )
      .map_err(|_| RepositoryError::Secret)?;
    let mut output = nonce.to_vec();
    output.extend(encrypted);
    Ok(format!("ggv1:{}", STANDARD.encode(output)))
  }
  pub fn open(&self, sealed: &str, uin: u32) -> Result<Zeroizing<String>, RepositoryError> {
    let data = STANDARD
      .decode(
        sealed
          .strip_prefix("ggv1:")
          .ok_or(RepositoryError::Secret)?,
      )
      .map_err(|_| RepositoryError::Secret)?;
    if data.len() < 40 {
      return Err(RepositoryError::Secret);
    }
    let nonce: [u8; 24] = data[..24].try_into().map_err(|_| RepositoryError::Secret)?;
    let plain = self
      .0
      .decrypt(
        &XNonce::from(nonce),
        Payload {
          msg: &data[24..],
          aad: &uin.to_le_bytes(),
        },
      )
      .map_err(|_| RepositoryError::Secret)?;
    String::from_utf8(plain)
      .map(Zeroizing::new)
      .map_err(|_| RepositoryError::Secret)
  }
}

#[derive(Debug)]
struct Peer {
  window: Instant,
  attempts: u32,
  active: u32,
}
#[derive(Debug, Default)]
pub struct Admission(Mutex<HashMap<IpAddr, Peer>>);
pub struct ConnectionPermit {
  admission: Arc<Admission>,
  ip: IpAddr,
}
impl Admission {
  pub fn admit(self: &Arc<Self>, ip: IpAddr) -> Option<ConnectionPermit> {
    let mut peers = self.0.lock();
    peers.retain(|_, p| p.active > 0 || p.window.elapsed() < Duration::from_secs(300));
    if peers.values().map(|p| p.active).sum::<u32>() >= 256
      || (!peers.contains_key(&ip) && peers.len() >= 4096)
    {
      return None;
    }
    let peer = peers.entry(ip).or_insert(Peer {
      window: Instant::now(),
      attempts: 0,
      active: 0,
    });
    if peer.window.elapsed() >= Duration::from_secs(300) {
      peer.window = Instant::now();
      peer.attempts = 0;
    }
    if peer.active >= 8 || peer.attempts >= 30 {
      return None;
    }
    peer.active += 1;
    peer.attempts += 1;
    Some(ConnectionPermit {
      admission: self.clone(),
      ip,
    })
  }
}
impl Drop for ConnectionPermit {
  fn drop(&mut self) {
    if let Some(peer) = self.admission.0.lock().get_mut(&self.ip) {
      peer.active -= 1;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn encrypted_password_is_bound_to_key_and_account() {
    let vault = PasswordVault::new(&[42; 32]).unwrap();
    let sealed = vault.seal("TestOnly123", 100).unwrap();
    assert!(!sealed.contains("TestOnly123"));
    assert_eq!(&**vault.open(&sealed, 100).unwrap(), "TestOnly123");
    assert!(vault.open(&sealed, 101).is_err());
    assert!(
      PasswordVault::new(&[43; 32])
        .unwrap()
        .open(&sealed, 100)
        .is_err()
    );
    assert_ne!(sealed, vault.seal("TestOnly123", 100).unwrap());
  }
  #[test]
  fn admission_caps_connections_and_releases_slots() {
    let guard = Arc::new(Admission::default());
    let ip = "127.0.0.1".parse().unwrap();
    let mut permits: Vec<_> = (0..8).map(|_| guard.admit(ip).unwrap()).collect();
    assert!(guard.admit(ip).is_none());
    permits.pop();
    assert!(guard.admit(ip).is_some());
  }
}
