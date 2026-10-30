//! Password recovery is intentionally unavailable; never panic or leak stored passwords.
use crate::core::SharedAppState;
use axum::{Router, routing::post};
pub fn router() -> Router<SharedAppState> {
  Router::new().route(
    "/appsvc/fmsendpwd3.asp",
    post(|| async { "pwdsend_failed" }),
  )
}
