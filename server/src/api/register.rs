//! Account management endpoint.
//!
//! `/appsvc/fmregister3.asp` - Handles account registration, deletion, and password changes.

use crate::core::SharedAppState;
use crate::models::{RepositoryError, TokenRepository, UserRepository};
use axum::{Form, Router, extract::State, routing::post};
use serde::Deserialize;

/// Form fields for account operations.
#[derive(Deserialize)]
pub struct RegisterForm {
  pub invite: Option<String>,
  /// New password (for registration/change).
  pub pwd: Option<String>,
  /// Email address.
  pub email: Option<String>,
  /// Token ID from regtoken endpoint.
  pub tokenid: Option<String>,
  /// User-entered token value (captcha answer).
  pub tokenval: Option<String>,
  /// GG number (for existing account operations).
  pub fmnumber: Option<u32>,
}

/// Handle account registration, deletion, or password change.
///
/// **Registration:**
/// - Required: `pwd`, `email`, `tokenid`, `tokenval`, `code`
/// - Success: `reg_success:UIN`
///
/// **Deletion:**
/// - Required: `fmnumber`, `fmpwd`, `delete=1`, `tokenid`, `tokenval`, `code`
///
/// **Password Change:**
/// - Required: `fmnumber`, `fmpwd`, `pwd`, `email`, `tokenid`, `tokenval`, `code`
#[tracing::instrument(skip_all)]
pub async fn register(
  State(app_state): State<SharedAppState>,
  Form(form): Form<RegisterForm>,
) -> Result<String, RepositoryError> {
  if !app_state.invite_matches(form.invite.as_deref().unwrap_or("")) {
    return Ok("reg_failed".into());
  }
  let users = UserRepository::new(app_state.db_pool());
  let tokens = TokenRepository::new(app_state.db_pool());

  // Registration: no fmnumber means new account
  if form.fmnumber.is_none() {
    if let (Some(pwd), Some(email), Some(tokenid), Some(captcha_code)) =
      (form.pwd, form.email, form.tokenid, form.tokenval)
    {
      if !(8..=15).contains(&pwd.len())
        || !pwd.bytes().all(|b| (33..=126).contains(&b))
        || email.len() > 254
        || !email.contains('@')
        || email.chars().any(char::is_control)
        || tokenid.len() != 32
        || captcha_code.len() != 4
      {
        return Ok("reg_failed".into());
      }
      if !tokens
        .validate(&tokenid, &captcha_code.to_ascii_uppercase())
        .await?
      {
        tracing::error!("invalid captcha");
        return Ok("reg_failed".to_string());
      }

      let name = email.split('@').next().unwrap_or("user");
      let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(app_state.db_pool())
        .await?;
      if count >= 1000 || users.find_by_email(&email).await?.is_some() {
        return Ok("reg_failed".into());
      }
      let user = users.create(name, &email, &pwd, app_state.vault()).await?;

      tracing::info!(uin = user.uin, "User registered");
      return Ok(format!("reg_success:{}", user.uin));
    }
  }

  // TODO: Implement deletion (fmnumber + fmpwd + delete="1")
  // TODO: Implement password change (fmnumber + fmpwd + pwd + email)

  Ok("reg_failed".to_string())
}

pub fn router() -> Router<SharedAppState> {
  Router::new().route("/appsvc/fmregister3.asp", post(register))
}
