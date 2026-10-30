//! Captcha token model and repository.

use crate::models::RepositoryError;
use rand::Rng;
use sqlx::{FromRow, Pool, Sqlite};
use tracing::instrument;

/// Token expiration time in minutes.
const TOKEN_EXPIRY_MINUTES: i32 = 5;

/// Captcha token record from database.
#[derive(Debug, Clone, FromRow)]
pub struct Token {
  /// Database ID.
  pub id: i64,
  /// Unique token identifier (32 chars).
  pub token_id: String,
  /// Captcha code (4 alphanumeric chars).
  pub captcha_code: String,
  /// Token creation timestamp.
  pub created_at: Option<String>,
  /// When the token was used (NULL if unused).
  pub used_at: Option<String>,
}

/// Repository for token database operations.
#[derive(Clone)]
pub struct TokenRepository {
  pool: Pool<Sqlite>,
}

impl std::fmt::Debug for TokenRepository {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("TokenRepository").finish()
  }
}

impl TokenRepository {
  /// Create a new repository with the given database pool.
  pub fn new(pool: &Pool<Sqlite>) -> Self {
    Self { pool: pool.clone() }
  }

  /// Create a new captcha token.
  #[instrument(skip_all)]
  pub async fn create(&self) -> Result<Token, RepositoryError> {
    sqlx::query(
      "DELETE FROM tokens WHERE used_at IS NOT NULL OR created_at < datetime('now', '-5 minutes')",
    )
    .execute(&self.pool)
    .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tokens")
      .fetch_one(&self.pool)
      .await?;
    if count >= 1000 {
      return Err(RepositoryError::MailboxFull);
    }
    let token_id: String = rand::rng()
      .sample_iter(rand::distr::Alphanumeric)
      .take(32)
      .map(char::from)
      .collect();

    let captcha_code: String = rand::rng()
      .sample_iter(rand::distr::Alphanumeric)
      .take(4)
      .map(char::from)
      .map(|c: char| c.to_ascii_uppercase())
      .collect();

    let result = sqlx::query_as::<_, Token>(
      "INSERT INTO tokens (token_id, captcha_code) VALUES (?, ?) RETURNING *",
    )
    .bind(token_id)
    .bind(captcha_code)
    .fetch_one(&self.pool)
    .await?;

    Ok(result)
  }

  /// Find a token by ID. Returns None if token is expired (>5 min) or already used.
  #[instrument(skip_all)]
  pub async fn find_by_token_id(&self, token_id: &str) -> Result<Option<Token>, RepositoryError> {
    let token = sqlx::query_as::<_, Token>(
      "SELECT * FROM tokens WHERE token_id = ? AND used_at IS NULL AND datetime(created_at, '+' || ? || ' minutes') > datetime('now')"
    )
      .bind(token_id)
      .bind(TOKEN_EXPIRY_MINUTES)
      .fetch_optional(&self.pool)
      .await?;
    Ok(token)
  }

  /// Validate a token and mark it as used if valid.
  /// Returns true if the token was valid and has been consumed.
  #[instrument(skip_all)]
  pub async fn validate(
    &self,
    token_id: &str,
    captcha_code: &str,
  ) -> Result<bool, RepositoryError> {
    let token = self.find_by_token_id(token_id).await?;

    match token {
      Some(t) if t.captcha_code == captcha_code => {
        let result = sqlx::query(
          "UPDATE tokens SET used_at = CURRENT_TIMESTAMP WHERE token_id = ? AND used_at IS NULL",
        )
        .bind(token_id)
        .execute(&self.pool)
        .await?;

        let validated = result.rows_affected() > 0;

        Ok(validated)
      }
      _ => Ok(false),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use sqlx::sqlite::SqlitePoolOptions;

  async fn setup_test_db() -> Result<Pool<Sqlite>, RepositoryError> {
    let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await?;

    sqlx::migrate!("./migrations")
      .run(&pool)
      .await
      .expect("migrations failed");

    Ok(pool)
  }
  // TODO: add specs

  #[tokio::test]
  async fn test_token_not_found() {
    let pool = setup_test_db().await.unwrap();
    let repo = TokenRepository::new(&pool);

    let found = repo.find_by_token_id("nonexistent").await.unwrap();
    assert!(found.is_none());
  }
}
