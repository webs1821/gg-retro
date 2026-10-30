//! Legacy GG 6 server discovery. The advertised endpoint may differ from the bind address.
use crate::core::SharedAppState;
use axum::{Router, extract::State, routing::get};

async fn discovery(State(state): State<SharedAppState>) -> String {
  format!(
    "0 0 {}:{} {}",
    state.host_ip,
    state.config().advertised_port,
    state.host_ip
  )
}
pub fn router() -> Router<SharedAppState> {
  Router::new()
    .route("/appsvc/appmsg4.asp", get(discovery))
    .route("/appsvc/appmsg3.asp", get(discovery))
}
