//! Static web assets embedded in the binary.
//!
//! Serves the landing page and static assets like logo.

use crate::api::ApiRequestError;
use crate::core::SharedAppState;
use askama::Template;
use axum::{
  Router,
  extract::{Query, State},
  response::{Html, IntoResponse},
  routing::get,
};
use axum_embed::ServeEmbed;
use rust_embed::RustEmbed;
use serde::Deserialize;

/// Embedded static assets from the static directory.
#[derive(RustEmbed, Clone)]
#[folder = "static/"]
struct Assets;

/// Package version from Cargo.toml.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// SHA256 checksum of gg61.exe installer.
const GG61_SHA256: &str = "bcc8157aa6bface009d8018c308bf3cef8725546b4f826bdbaf6bbeaa953b06f";

/// Query parameters for index page.
#[derive(Deserialize)]
struct IndexQuery {
  adid: Option<String>,
}

#[derive(Template)]
#[template(path = "home.html")]
struct HomeTemplate<'a> {
  online: usize,
  registered: String,
  hostname: &'a str,
  version: &'a str,
  gg61_sha256: &'a str,
  has_client: bool,
  has_installer: bool,
  client_sha256: &'a str,
  installer_sha256: &'a str,
}

#[derive(Template)]
#[template(path = "ads.html")]
struct AdsTemplate;

/// Serve the index page or ads based on query parameters.
async fn index(
  State(app_state): State<SharedAppState>,
  Query(query): Query<IndexQuery>,
) -> Result<impl IntoResponse, ApiRequestError> {
  if query.adid.is_some() {
    let template = AdsTemplate;
    return Ok(Html(template.render()?));
  }

  let registered = match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
    .fetch_one(app_state.db_pool())
    .await
  {
    Ok(count) => count.to_string(),
    Err(error) => {
      tracing::warn!(%error, "Could not count registered users");
      "—".to_owned()
    }
  };
  let template = HomeTemplate {
    online: app_state.presence_hub().online(),
    registered,
    hostname: app_state.config().hostname(),
    version: VERSION,
    gg61_sha256: GG61_SHA256,
    has_client: app_state.client_downloads.is_some(),
    has_installer: app_state
      .client_downloads
      .as_ref()
      .is_some_and(|d| d.installer.is_some()),
    client_sha256: app_state
      .client_downloads
      .as_ref()
      .map(|d| d.client.sha256.as_str())
      .unwrap_or(""),
    installer_sha256: app_state
      .client_downloads
      .as_ref()
      .and_then(|d| d.installer.as_ref())
      .map(|d| d.sha256.as_str())
      .unwrap_or(""),
  };
  Ok(Html(template.render()?))
}

/// Create router for static web assets.
pub fn router() -> Router<SharedAppState> {
  let serve_assets = ServeEmbed::<Assets>::new();

  Router::new()
    .route("/", get(index))
    .route("/downloads/gg.exe", get(client_download))
    .route("/downloads/gg-setup.exe", get(installer_download))
    .nest_service("/static", serve_assets)
}

fn download(
  file: Option<&crate::downloads::Download>,
  filename: &'static str,
) -> axum::response::Response {
  match file {
    Some(file) => (
      [
        ("content-type", "application/octet-stream"),
        ("content-disposition", filename),
        ("x-content-type-options", "nosniff"),
      ],
      file.bytes.clone(),
    )
      .into_response(),
    None => axum::http::StatusCode::NOT_FOUND.into_response(),
  }
}
async fn client_download(State(app): State<SharedAppState>) -> axum::response::Response {
  download(
    app.client_downloads.as_ref().map(|d| &d.client),
    "attachment; filename=gg.exe",
  )
}
async fn installer_download(State(app): State<SharedAppState>) -> axum::response::Response {
  download(
    app
      .client_downloads
      .as_ref()
      .and_then(|d| d.installer.as_ref()),
    "attachment; filename=gg-retro-setup.exe",
  )
}
