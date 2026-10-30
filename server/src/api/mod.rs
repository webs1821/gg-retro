//! HTTP endpoints for GG protocol services.
//!
//! These endpoints implement the HTTP services described in the GG 6.0 protocol:
//! - Server discovery (`appmsg`)
//! - Captcha token and image (`captcha`)
//! - Account registration/management (`register`)
//! - Password recovery (`sendpwd`)
//! - Web landing page (`web`)

mod appmsg;
mod captcha;
mod register;
mod sendpwd;
mod web;

use crate::core::SharedAppState;
use axum::Router;
use axum::response::IntoResponse;
use std::fmt::{Display, Formatter};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

#[derive(Debug, Error)]
pub enum ApiRequestError {
  Render(#[from] askama::Error),
}

impl Display for ApiRequestError {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      ApiRequestError::Render(e) => write!(f, "{}", e),
    }
  }
}

impl IntoResponse for ApiRequestError {
  fn into_response(self) -> axum::response::Response {
    (
      axum::http::StatusCode::INTERNAL_SERVER_ERROR,
      (),
      self.to_string(),
    )
      .into_response()
  }
}

/// Create the HTTP router with all GG protocol endpoints.
pub fn router(app_state: SharedAppState) -> Router {
  Router::new()
    .route("/healthz", axum::routing::get(health))
    .merge(web::router())
    .merge(appmsg::router())
    .merge(captcha::router())
    .merge(register::router())
    .merge(sendpwd::router())
    .layer(
      TraceLayer::new_for_http()
        .make_span_with(|request: &axum::http::Request<axum::body::Body>| tracing::info_span!("http", method = %request.method(), path = %request.uri().path()))
        .on_response(DefaultOnResponse::new().level(Level::INFO))
    )
    .layer(axum::extract::DefaultBodyLimit::max(8192))
    .with_state(app_state)
}

/// Start the HTTP server on the given listener with graceful shutdown.
#[tracing::instrument(skip(listener, shutdown, app_state))]
pub async fn http_server(
  listener: TcpListener,
  shutdown: CancellationToken,
  app_state: SharedAppState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  tracing::info!(bind = ?listener.local_addr()?, "HTTP server listening");
  axum::serve(listener, router(app_state))
    .with_graceful_shutdown(async move {
      shutdown.cancelled().await;
      tracing::info!("HTTP server shutting down...");
    })
    .await?;
  Ok(())
}

async fn health(
  axum::extract::State(state): axum::extract::State<SharedAppState>,
) -> axum::http::StatusCode {
  if sqlx::query("SELECT 1")
    .execute(state.db_pool())
    .await
    .is_ok()
  {
    axum::http::StatusCode::OK
  } else {
    axum::http::StatusCode::SERVICE_UNAVAILABLE
  }
}
