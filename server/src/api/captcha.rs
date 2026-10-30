//! Captcha token and image endpoints.
//!
//! - `/appsvc/regtoken.asp` - Generates a token for account operations.
//! - `/appsvc/tokenpic.asp` - Returns the captcha image for a given token ID.
//!
//! All account operations (register, delete, change password) require a valid token.

use axum::response::{IntoResponse, Response};
use axum::{
  Router,
  extract::{Query, State},
  http::{StatusCode, header},
  routing::get,
};

use ab_glyph::{FontRef, PxScale};
use image::{ImageBuffer, Rgba, codecs::gif::GifEncoder};
use imageproc::drawing::draw_text_mut;
use serde::Deserialize;

use crate::core::SharedAppState;
use crate::models::{RepositoryError, TokenRepository};

/// Query parameters for token image.
#[derive(Debug, Deserialize)]
pub struct TokenPicParams {
  /// Token ID from regtoken endpoint.
  pub tokenid: String,
}

/// Embedded font for captcha rendering (DejaVu Sans Mono).
const FONT_DATA: &[u8] = include_bytes!("../../assets/DejaVuSansMono.ttf");
const WIDTH: u32 = 60;
const HEIGHT: u32 = 20;

/// Render captcha text to a GIF image.
fn render_captcha_gif(text: &str) -> Vec<u8> {
  let white = Rgba([255u8, 255, 255, 255]);
  let black = Rgba([0u8, 0, 0, 255]);

  let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_pixel(WIDTH, HEIGHT, white);

  let font = FontRef::try_from_slice(FONT_DATA).expect("failed to load font");
  let scale = PxScale::from(14.0);

  draw_text_mut(&mut img, black, 5, 2, scale, &font, text);

  let mut buf = Vec::new();
  {
    let mut encoder = GifEncoder::new(&mut buf);
    encoder
      .encode_frame(image::Frame::new(img))
      .expect("failed to encode gif");
  }
  buf
}

/// Generate a captcha token for account operations.
///
/// Response format:
/// ```text
/// WIDTH HEIGHT LENGTH
/// TOKEN_ID
/// IMAGE_URL
/// ```
///
/// Example:
/// ```text
/// 60 20 4
/// abc123def456
/// http://192.168.1.100:80/appsvc/tokenpic.asp
/// ```
#[tracing::instrument(skip_all)]
pub async fn regtoken(State(app_state): State<SharedAppState>) -> Result<String, RepositoryError> {
  let repo = TokenRepository::new(app_state.db_pool());
  let token = repo.create().await?;

  let endpoint = app_state
    .host_uri("/appsvc/tokenpic.asp")
    .unwrap_or_default()
    .to_string();
  Ok(format!(
    "{} {} 4\r\n{}\r\n{}",
    WIDTH, HEIGHT, token.token_id, endpoint
  ))
}

/// Get captcha image for the given token.
///
/// Returns a GIF image with the captcha text.
#[tracing::instrument(skip_all)]
pub async fn tokenpic(
  State(app_state): State<SharedAppState>,
  Query(params): Query<TokenPicParams>,
) -> Response {
  let repo = TokenRepository::new(app_state.db_pool());

  let token = match repo.find_by_token_id(&params.tokenid).await {
    Ok(Some(t)) => t,
    Ok(None) => return StatusCode::NOT_FOUND.into_response(),
    Err(e) => return e.into_response(),
  };

  let gif_data = render_captcha_gif(&token.captcha_code);

  (
    StatusCode::OK,
    [(header::CONTENT_TYPE, "image/gif")],
    gif_data,
  )
    .into_response()
}

pub fn router() -> Router<SharedAppState> {
  Router::new()
    .route("/appsvc/regtoken.asp", get(regtoken))
    .route("/appsvc/tokenpic.asp", get(tokenpic))
}
