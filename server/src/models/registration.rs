//! Atomic registration: invitation, CAPTCHA, number allocation and account commit together.
use super::{DatabasePool, RepositoryError, user::User};
use crate::security::PasswordVault;
use gg_protocol::consts::GG60_MAX_UIN;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegistrationFailure {
  #[error("reg_failed:invite")]
  Invite,
  #[error("reg_failed:captcha")]
  Captcha,
  #[error("reg_failed:number_invalid")]
  NumberInvalid,
  #[error("reg_failed:number_taken")]
  NumberTaken,
  #[error("reg_failed:email_taken")]
  EmailTaken,
  #[error("reg_failed:capacity")]
  Capacity,
}

pub struct NewAccount<'a> {
  pub email: &'a str,
  pub password: &'a str,
  pub requested_uin: Option<u32>,
  pub invite: &'a str,
  pub token_id: &'a str,
  pub captcha: &'a str,
}

/// Only hashes reach SQLite. Removing a code revokes it; re-adding never resets its usage.
pub async fn sync_invites(
  pool: &DatabasePool,
  source: &str,
) -> Result<(), Box<dyn std::error::Error>> {
  let mut hashes = HashSet::new();
  for code in source
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty())
  {
    if !(24..=128).contains(&code.len())
      || !code
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
      return Err("Invitation file must contain one 24-128 character ASCII code per line".into());
    }
    if !hashes.insert(Sha256::digest(code.as_bytes()).to_vec()) || hashes.len() > 10000 {
      return Err("Invitation file contains duplicate codes or more than 10000 entries".into());
    }
  }
  let mut tx = pool.begin().await?;
  sqlx::query("UPDATE registration_invites SET active=0")
    .execute(&mut *tx)
    .await?;
  for hash in hashes {
    sqlx::query("INSERT INTO registration_invites(code_hash) VALUES (?) ON CONFLICT(code_hash) DO UPDATE SET active=1")
      .bind(hash).execute(&mut *tx).await?;
  }
  tx.commit().await?;
  Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn create(
  pool: &DatabasePool,
  vault: &PasswordVault,
  form: NewAccount<'_>,
) -> Result<User, RepositoryError> {
  let invite = form.invite.trim();
  if !(24..=128).contains(&invite.len()) {
    return Err(RegistrationFailure::Invite.into());
  }
  if form
    .requested_uin
    .is_some_and(|n| n == 0 || n > GG60_MAX_UIN)
  {
    return Err(RegistrationFailure::NumberInvalid.into());
  }
  let hash = Sha256::digest(invite.as_bytes()).to_vec();
  let mut tx = pool.begin().await?;
  // The first statement is a write: concurrent registrations serialize before any
  // reads. Failed attempts roll back the credit, CAPTCHA and sequence together.
  let changed = sqlx::query("UPDATE registration_invites SET used=used+1,last_used_at=CURRENT_TIMESTAMP WHERE code_hash=? AND active=1 AND used<max_uses")
    .bind(&hash).execute(&mut *tx).await?.rows_affected();
  if changed != 1 {
    return Err(RegistrationFailure::Invite.into());
  }
  let captcha = sqlx::query("UPDATE tokens SET used_at=CURRENT_TIMESTAMP WHERE token_id=? AND captcha_code=? AND used_at IS NULL AND created_at>datetime('now','-5 minutes')")
    .bind(form.token_id).bind(form.captcha.to_ascii_uppercase()).execute(&mut *tx).await?.rows_affected();
  if captcha != 1 {
    return Err(RegistrationFailure::Captcha.into());
  }
  let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
    .fetch_one(&mut *tx)
    .await?;
  if count >= 1000 {
    return Err(RegistrationFailure::Capacity.into());
  }
  let duplicate: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE email=?)")
    .bind(form.email)
    .fetch_one(&mut *tx)
    .await?;
  if duplicate {
    return Err(RegistrationFailure::EmailTaken.into());
  }
  let uin = if let Some(requested) = form.requested_uin {
    let occupied: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE uin=?)")
      .bind(requested)
      .fetch_one(&mut *tx)
      .await?;
    if occupied {
      return Err(RegistrationFailure::NumberTaken.into());
    }
    requested
  } else {
    let mut next: u32 = sqlx::query_scalar("SELECT next_uin FROM registration_state WHERE id=1")
      .fetch_one(&mut *tx)
      .await?;
    let occupied: Vec<u32> = sqlx::query_scalar("SELECT uin FROM users WHERE uin>=? ORDER BY uin")
      .bind(next)
      .fetch_all(&mut *tx)
      .await?;
    for number in occupied {
      if number != next {
        break;
      }
      next += 1;
    }
    if next > GG60_MAX_UIN {
      return Err(RegistrationFailure::Capacity.into());
    }
    sqlx::query("UPDATE registration_state SET next_uin=? WHERE id=1")
      .bind(next + 1)
      .execute(&mut *tx)
      .await?;
    next
  };
  let user = sqlx::query_as::<_, User>("INSERT INTO users(uin,name,email,password,registration_invite_hash) VALUES (?,?,?,?,?) RETURNING *")
    .bind(uin).bind(form.email.split('@').next().unwrap_or("user")).bind(form.email)
    .bind(vault.seal(form.password,uin)?).bind(hash).fetch_one(&mut *tx).await?;
  tx.commit().await?;
  tracing::info!(uin, "User registered");
  Ok(user)
}
