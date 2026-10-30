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
  hostname: &'a str,
  version: &'a str,
  gg61_sha256: &'a str,
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

  let template = HomeTemplate {
    online: app_state.presence_hub().online(),
    hostname: app_state.config().hostname(),
    version: VERSION,
    gg61_sha256: GG61_SHA256,
  };
  Ok(Html(template.render()?))
}

/// Create router for static web assets.
pub fn router() -> Router<SharedAppState> {
  let serve_assets = ServeEmbed::<Assets>::new();

  Router::new()
    .route("/", get(index))
    .nest_service("/static", serve_assets)
}
