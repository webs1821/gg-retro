//! User model and repository.

use crate::models::RepositoryError;
use crate::security::PasswordVault;
use gg_protocol::GGNumber;
use rand::Rng;
use sqlx::{FromRow, Pool, Sqlite};
use tracing::{info, instrument};

/// Assigned numbers stay within the 24-bit GG6 address space.
const MIN_UIN: u32 = 1_000_000;
const MAX_UIN: u32 = 6_699_999;

/// User record from database.
#[derive(Clone, FromRow)]
pub struct User {
  /// Gadu-Gadu user number (UIN).
  pub uin: u32,
  /// Display name.
  pub name: String,
  /// Email address (unique).
  pub email: String,
  /// Authenticated ciphertext; the decryption key is stored separately.
  pub password: String,
  /// Account creation timestamp.
  pub created_at: Option<String>,
}

/// Repository for user database operations.
#[derive(Clone)]
pub struct UserRepository {
  #[allow(dead_code)]
  pool: Pool<Sqlite>,
}

impl std::fmt::Debug for UserRepository {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("UserRepository").finish()
  }
}

impl UserRepository {
  /// Create a new repository with the given database pool.
  pub fn new(pool: &Pool<Sqlite>) -> Self {
    Self { pool: pool.clone() }
  }

  /// Create a new user and return it with the assigned UIN.
  #[instrument(skip_all)]
  pub async fn create(
    &self,
    name: &str,
    email: &str,
    password: &str,
    vault: &PasswordVault,
  ) -> Result<User, RepositoryError> {
    // todo: add validation, min password length
    let uin = rand::rng().random_range(MIN_UIN..=MAX_UIN);
    let result = sqlx::query_as::<_, User>(
      "INSERT INTO users (uin, name, email, password) VALUES (?, ?, ?, ?) RETURNING *",
    )
    .bind(uin)
    .bind(name)
    .bind(email)
    .bind(vault.seal(password, uin)?)
    .fetch_one(&self.pool)
    .await?;

    info!(uin = result.uin, "User created");
    Ok(result)
  }

  /// Find a user by UIN.
  #[instrument(skip_all)]
  pub async fn find_by_uin(&self, uin: GGNumber) -> Result<Option<User>, RepositoryError> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE uin = ?")
      .bind(uin)
      .fetch_optional(&self.pool)
      .await?;
    Ok(user)
  }

  /// Check if a user exists with the given UIN.
  #[instrument(skip_all)]
  pub async fn exists(&self, uin: GGNumber) -> Result<bool, RepositoryError> {
    let result: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM users WHERE uin = ?")
      .bind(uin)
      .fetch_optional(&self.pool)
      .await?;
    Ok(result.is_some())
  }

  /// Find multiple users by their UIDs.
  #[instrument(skip_all)]
  pub async fn find_by_uins(&self, uins: &[GGNumber]) -> Result<Vec<User>, RepositoryError> {
    if uins.is_empty() {
      return Ok(Vec::new());
    }

    let placeholders = uins.iter().map(|_| "?").collect::<Vec<_>>().join(",");

    let query = format!("SELECT * FROM users WHERE uin IN ({})", placeholders);

    let mut query_builder = sqlx::query_as::<_, User>(&query);
    for &uin in uins {
      query_builder = query_builder.bind(uin);
    }

    let users = query_builder.fetch_all(&self.pool).await?;
    Ok(users)
  }

  /// Find a user by email.
  #[instrument(skip_all)]
  pub async fn find_by_email(&self, email: &str) -> Result<Option<User>, RepositoryError> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = ?")
      .bind(email)
      .fetch_optional(&self.pool)
      .await?;

    Ok(user)
  }

  /// Update user's password.
  #[instrument(skip_all)]
  pub async fn update_password(
    &self,
    uin: u32,
    password: &str,
    vault: &PasswordVault,
  ) -> Result<bool, RepositoryError> {
    let result = sqlx::query("UPDATE users SET password = ? WHERE uin = ?")
      .bind(vault.seal(password, uin)?)
      .bind(uin)
      .execute(&self.pool)
      .await?;

    let updated = result.rows_affected() > 0;
    if updated {
      info!("User password updated");
    }
    Ok(updated)
  }

  /// Delete a user by UIN.
  #[instrument(skip_all)]
  pub async fn delete(&self, uin: u32) -> Result<bool, RepositoryError> {
    let result = sqlx::query("DELETE FROM users WHERE uin = ?")
      .bind(uin)
      .execute(&self.pool)
      .await?;

    let deleted = result.rows_affected() > 0;
    if deleted {
      info!("User deleted");
    }
    Ok(deleted)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::RepositoryError;
  use crate::security::PasswordVault;
  use sqlx::sqlite::SqlitePoolOptions;

  async fn setup_test_db() -> Result<Pool<Sqlite>, RepositoryError> {
    let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await?;

    sqlx::migrate!("./migrations")
      .run(&pool)
      .await
      .expect("Migration failed");

    Ok(pool)
  }

  #[tokio::test]
  async fn test_user_crud() {
    let pool = setup_test_db().await.unwrap();
    let repo = UserRepository::new(&pool);
    let vault = PasswordVault::new(&[7; 32]).unwrap();

    // Create
    let user = repo
      .create("Test", "test@gg.pl", "hash123", &vault)
      .await
      .unwrap();
    assert!(user.uin > 0);

    // Find
    let found = repo.find_by_uin(user.uin).await.unwrap().unwrap();
    assert_eq!(found.email, "test@gg.pl");

    // Update
    repo
      .update_password(user.uin, "newhash", &vault)
      .await
      .unwrap();
    let updated = repo.find_by_uin(user.uin).await.unwrap().unwrap();
    assert_ne!(updated.password, "newhash");
    assert_eq!(
      &**vault.open(&updated.password, user.uin).unwrap(),
      "newhash"
    );

    // Delete
    repo.delete(user.uin).await.unwrap();
    assert!(repo.find_by_uin(user.uin).await.unwrap().is_none());
  }
}
