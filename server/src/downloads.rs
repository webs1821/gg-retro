//! Optional deployment-specific client files. Never embedded in the generic server binary.
use axum::body::Bytes;
use figment::{
  Figment,
  providers::{Format, Toml},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{error::Error, path::Path};

#[derive(Deserialize)]
struct Manifest {
  hostname: String,
  gg_exe_sha256: String,
  setup_exe_sha256: Option<String>,
}
pub struct Download {
  pub bytes: Bytes,
  pub sha256: String,
}
pub struct ClientDownloads {
  pub client: Download,
  pub installer: Option<Download>,
}
fn read_file(dir: &Path, name: &str, expected: &str) -> Result<Download, Box<dyn Error>> {
  let path = dir.join(name);
  if std::fs::metadata(&path)?.len() > 16 * 1024 * 1024 {
    return Err("Client download exceeds 16 MiB".into());
  }
  let data = std::fs::read(path)?;
  let sha256 = format!("{:x}", Sha256::digest(&data));
  if !data.starts_with(b"MZ") || sha256 != expected {
    return Err("Client download hash or EXE format mismatch".into());
  }
  Ok(Download {
    bytes: Bytes::from(data),
    sha256,
  })
}
pub fn load(directory: &str, hostname: &str) -> Result<Option<ClientDownloads>, Box<dyn Error>> {
  if directory.is_empty() {
    return Ok(None);
  }
  let dir = Path::new(directory);
  let manifest: Manifest = Figment::from(Toml::file(dir.join("manifest.toml"))).extract()?;
  if manifest.hostname != hostname {
    return Err(
      "Client download hostname differs from server configuration; rebuild client package".into(),
    );
  }
  let client = read_file(dir, "gg.exe", &manifest.gg_exe_sha256)?;
  let installer = manifest
    .setup_exe_sha256
    .as_deref()
    .map(|hash| read_file(dir, "gg-setup.exe", hash))
    .transpose()?;
  Ok(Some(ClientDownloads { client, installer }))
}
