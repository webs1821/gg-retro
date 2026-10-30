//! Database models and repositories.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use sqlx::{Pool, Sqlite};
use thiserror::Error;

pub mod message;
pub mod token;
pub mod user;

pub type DatabasePool = Pool<Sqlite>;
pub use message::MessageRepository;
pub use token::TokenRepository;
pub use user::UserRepository;

#[derive(Error, Debug)]
pub enum RepositoryError {
  #[error("Credential encryption failed (check the separate key file)")]
  Secret,
  #[error("Mailbox capacity reached")]
  MailboxFull,
  #[error("Database operation failed: {0}")]
  DatabaseError(#[from] sqlx::Error),
}

impl IntoResponse for RepositoryError {
  fn into_response(self) -> Response {
    match self {
      error => {
        tracing::error!(error = ?error, "failed to finish response because of repository error");
        (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
      }
    }
  }
}
