//! Registration on this private GG network. Existing-account operations remain disabled.
use crate::core::SharedAppState;
use crate::models::{
  RepositoryError,
  registration::{self, NewAccount, RegistrationFailure},
};
use axum::{Form, Router, extract::State, routing::post};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct RegisterForm {
  pub invite: Option<String>,
  pub pwd: Option<String>,
  pub email: Option<String>,
  pub tokenid: Option<String>,
  pub tokenval: Option<String>,
  pub requested_uin: Option<String>,
  pub fmnumber: Option<u32>,
}

#[tracing::instrument(skip_all)]
pub async fn register(
  State(app): State<SharedAppState>,
  Form(form): Form<RegisterForm>,
) -> Result<String, RepositoryError> {
  if form.fmnumber.is_some() {
    return Ok("reg_failed".into());
  }
  let (Some(pwd), Some(email), Some(tokenid), Some(captcha), Some(invite)) = (
    form.pwd,
    form.email,
    form.tokenid,
    form.tokenval,
    form.invite,
  ) else {
    return Ok("reg_failed".into());
  };
  if !(8..=15).contains(&pwd.len())
    || !pwd.bytes().all(|b| (33..=126).contains(&b))
    || email.len() > 254
    || !email.contains('@')
    || email.chars().any(char::is_control)
    || tokenid.len() != 32
    || captcha.len() != 4
  {
    return Ok("reg_failed".into());
  }
  let requested = form.requested_uin.as_deref().unwrap_or("").trim();
  let requested_uin = if requested.is_empty() {
    None
  } else {
    if !requested.bytes().all(|b| b.is_ascii_digit()) {
      return Err(RegistrationFailure::NumberInvalid.into());
    }
    Some(
      requested
        .parse::<u32>()
        .map_err(|_| RegistrationFailure::NumberInvalid)?,
    )
  };
  let user = registration::create(
    app.db_pool(),
    app.vault(),
    NewAccount {
      email: &email,
      password: &pwd,
      requested_uin,
      invite: &invite,
      token_id: &tokenid,
      captcha: &captcha,
    },
  )
  .await?;
  Ok(format!("reg_success:{}", user.uin))
}

pub fn router() -> Router<SharedAppState> {
  Router::new().route("/appsvc/fmregister3.asp", post(register))
}
