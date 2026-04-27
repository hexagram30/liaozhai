//! `SQLite`-backed account storage with argon2id password hashing.
//!
//! ## Schema
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS accounts (
//!     id            TEXT PRIMARY KEY,
//!     username      TEXT UNIQUE NOT NULL COLLATE NOCASE,
//!     password_hash TEXT NOT NULL,
//!     created_at    INTEGER NOT NULL,
//!     last_login_at INTEGER
//! );
//! ```
//!
//! - `id`: UUID v4 as canonical hyphenated text.
//! - `username`: case-insensitive uniqueness via `COLLATE NOCASE`.
//!   Original case is preserved.
//! - `password_hash`: argon2 PHC string (embeds algorithm, version,
//!   params, salt, and hash).
//! - `created_at`: Unix epoch seconds.
//! - `last_login_at`: Unix epoch seconds; NULL until first login.
//!
//! ## Timing-attack resistance
//!
//! `verify_credentials` always performs an argon2 verification — even
//! when the username doesn't exist — by checking against a dummy hash
//! generated at startup. This ensures that "user doesn't exist" and
//! "wrong password" take approximately the same time.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::{Algorithm, Argon2, Version};
use liaozhai_core::error::{Error, Result};
use liaozhai_core::id::AccountId;
use password_hash::rand_core::OsRng;
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use uuid::Uuid;

use crate::account::Account;
use crate::params::Argon2Params;

const CREATE_TABLE_SQL: &str = "
    CREATE TABLE IF NOT EXISTS accounts (
        id            TEXT PRIMARY KEY,
        username      TEXT UNIQUE NOT NULL COLLATE NOCASE,
        password_hash TEXT NOT NULL,
        created_at    INTEGER NOT NULL,
        last_login_at INTEGER
    )
";

/// `SQLite`-backed account store.
///
/// Thread-safe via `Arc<Mutex<Connection>>`. All public async
/// methods use `spawn_blocking` to avoid blocking the tokio runtime.
#[derive(Debug, Clone)]
pub struct AccountStore {
    conn: Arc<Mutex<rusqlite::Connection>>,
    params: Argon2Params,
    dummy_hash: String,
}

impl AccountStore {
    /// Open (or create) the `SQLite` database at `path`, ensuring the schema exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or the schema cannot be created.
    ///
    /// # Panics
    ///
    /// Panics if argon2 dummy hash generation fails (should not happen with valid params).
    pub fn open(path: &Path, params: &Argon2Params) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| Error::Auth(format!("creating database directory: {e}")))?;
            }
        }

        let conn = rusqlite::Connection::open(path)
            .map_err(|e| Error::Auth(format!("opening database: {e}")))?;

        conn.execute_batch(CREATE_TABLE_SQL)
            .map_err(|e| Error::Auth(format!("creating schema: {e}")))?;

        let dummy_hash = generate_hash("dummy_password_for_timing", params)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            params: params.clone(),
            dummy_hash,
        })
    }

    /// Insert a new account. Username uniqueness is enforced by `SQLite`.
    ///
    /// # Errors
    ///
    /// Returns `Error::Auth` if the username already exists or if a
    /// database error occurs.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub async fn create_account(&self, username: &str, password: &str) -> Result<Account> {
        let conn = Arc::clone(&self.conn);
        let username = username.to_owned();
        let password = password.to_owned();
        let params = self.params.clone();

        tokio::task::spawn_blocking(move || {
            let password_hash = generate_hash(&password, &params)?;
            let id = AccountId::new();
            let now = unix_now();

            let conn = conn.lock().expect("AccountStore mutex poisoned");
            conn.execute(
                "INSERT INTO accounts (id, username, password_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id.to_string(), username, password_hash, now],
            )
            .map_err(|e| {
                if let rusqlite::Error::SqliteFailure(ref err, _) = e {
                    if err.code == rusqlite::ErrorCode::ConstraintViolation {
                        return Error::Auth(format!("Account '{username}' already exists"));
                    }
                }
                Error::Auth(format!("creating account: {e}"))
            })?;

            Ok(Account::from_row(id, username, now, None))
        })
        .await
        .map_err(|e| Error::Auth(format!("spawn_blocking failed: {e}")))?
    }

    /// Verify credentials. Returns `Ok(Some(Account))` on success,
    /// `Ok(None)` on wrong password or unknown user. Always performs
    /// argon2 verification for timing-attack resistance.
    ///
    /// # Errors
    ///
    /// Returns `Error::Auth` only for database/system errors, not for
    /// wrong credentials.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub async fn verify_credentials(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<Account>> {
        let conn = Arc::clone(&self.conn);
        let username = username.to_owned();
        let password = password.to_owned();
        let dummy_hash = self.dummy_hash.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().expect("AccountStore mutex poisoned");

            let row = conn.query_row(
                "SELECT id, username, password_hash, created_at, last_login_at \
                 FROM accounts WHERE username = ?1",
                rusqlite::params![username],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                },
            );

            match row {
                Ok((id_str, db_username, stored_hash, created_at, last_login_at)) => {
                    let parsed = PasswordHash::new(&stored_hash)
                        .map_err(|e| Error::Auth(format!("parsing stored hash: {e}")))?;
                    let argon2 = Argon2::default();
                    if argon2.verify_password(password.as_bytes(), &parsed).is_ok() {
                        let uuid = Uuid::parse_str(&id_str)
                            .map_err(|e| Error::Auth(format!("parsing account ID: {e}")))?;
                        Ok(Some(Account::from_row(
                            AccountId::from_uuid(uuid),
                            db_username,
                            created_at,
                            last_login_at,
                        )))
                    } else {
                        Ok(None)
                    }
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    // Timing-attack resistance: verify against dummy hash
                    let parsed = PasswordHash::new(&dummy_hash)
                        .map_err(|e| Error::Auth(format!("parsing dummy hash: {e}")))?;
                    let argon2 = Argon2::default();
                    let _ = argon2.verify_password(password.as_bytes(), &parsed);
                    Ok(None)
                }
                Err(e) => Err(Error::Auth(format!("querying account: {e}"))),
            }
        })
        .await
        .map_err(|e| Error::Auth(format!("spawn_blocking failed: {e}")))?
    }

    /// List all accounts (no password hashes returned).
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub async fn list_accounts(&self) -> Result<Vec<Account>> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().expect("AccountStore mutex poisoned");
            let mut stmt = conn
                .prepare(
                    "SELECT id, username, created_at, last_login_at \
                     FROM accounts ORDER BY username",
                )
                .map_err(|e| Error::Auth(format!("preparing statement: {e}")))?;

            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                })
                .map_err(|e| Error::Auth(format!("querying accounts: {e}")))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Auth(format!("collecting accounts: {e}")))?;

            rows.into_iter()
                .map(|(id_str, username, created_at, last_login_at)| {
                    let uuid = Uuid::parse_str(&id_str)
                        .map_err(|e| Error::Auth(format!("parsing account ID: {e}")))?;
                    Ok(Account::from_row(
                        AccountId::from_uuid(uuid),
                        username,
                        created_at,
                        last_login_at,
                    ))
                })
                .collect()
        })
        .await
        .map_err(|e| Error::Auth(format!("spawn_blocking failed: {e}")))?
    }

    /// Update `last_login_at` to the current Unix timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error if the database update fails.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub async fn record_login(&self, account_id: AccountId) -> Result<()> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().expect("AccountStore mutex poisoned");
            let now = unix_now();
            conn.execute(
                "UPDATE accounts SET last_login_at = ?1 WHERE id = ?2",
                rusqlite::params![now, account_id.to_string()],
            )
            .map_err(|e| Error::Auth(format!("recording login: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| Error::Auth(format!("spawn_blocking failed: {e}")))?
    }
}

fn generate_hash(password: &str, params: &Argon2Params) -> Result<String> {
    let argon2_params = params
        .to_argon2_params()
        .map_err(|e| Error::Auth(format!("invalid argon2 params: {e}")))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon2_params);
    let salt = SaltString::generate(&mut OsRng);
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| Error::Auth(format!("hashing password: {e}")))?;
    Ok(hash.to_string())
}

#[expect(clippy::cast_possible_wrap)]
fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_params() -> Argon2Params {
        Argon2Params::test_fast()
    }

    fn open_test_store() -> (AccountStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let store = AccountStore::open(&db_path, &test_params()).unwrap();
        (store, dir)
    }

    #[test]
    fn open_creates_file_and_table() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        assert!(!db_path.exists());
        let _store = AccountStore::open(&db_path, &test_params()).unwrap();
        assert!(db_path.exists());
    }

    #[test]
    fn open_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let _store1 = AccountStore::open(&db_path, &test_params()).unwrap();
        let _store2 = AccountStore::open(&db_path, &test_params()).unwrap();
    }

    #[tokio::test]
    async fn create_account_succeeds() {
        let (store, _dir) = open_test_store();
        let acct = store.create_account("alice", "secret").await.unwrap();
        assert_eq!(acct.username(), "alice");
        assert!(acct.created_at() > 0);
        assert_eq!(acct.last_login_at(), None);
    }

    #[tokio::test]
    async fn create_account_duplicate_rejected() {
        let (store, _dir) = open_test_store();
        store.create_account("alice", "secret").await.unwrap();
        let err = store.create_account("alice", "other").await.unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn create_account_case_insensitive_duplicate() {
        let (store, _dir) = open_test_store();
        store.create_account("Alice", "secret").await.unwrap();
        let err = store.create_account("alice", "other").await.unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn verify_credentials_correct_password() {
        let (store, _dir) = open_test_store();
        store.create_account("alice", "secret").await.unwrap();
        let result = store.verify_credentials("alice", "secret").await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().username(), "alice");
    }

    #[tokio::test]
    async fn verify_credentials_wrong_password() {
        let (store, _dir) = open_test_store();
        store.create_account("alice", "secret").await.unwrap();
        let result = store.verify_credentials("alice", "wrong").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn verify_credentials_unknown_user() {
        let (store, _dir) = open_test_store();
        let result = store
            .verify_credentials("nobody", "whatever")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn verify_credentials_case_insensitive_username() {
        let (store, _dir) = open_test_store();
        store.create_account("Alice", "secret").await.unwrap();
        let result = store.verify_credentials("alice", "secret").await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn list_accounts_empty() {
        let (store, _dir) = open_test_store();
        let accounts = store.list_accounts().await.unwrap();
        assert!(accounts.is_empty());
    }

    #[tokio::test]
    async fn list_accounts_after_create() {
        let (store, _dir) = open_test_store();
        store.create_account("alice", "secret").await.unwrap();
        store.create_account("bob", "pass").await.unwrap();
        let accounts = store.list_accounts().await.unwrap();
        assert_eq!(accounts.len(), 2);
        let names: Vec<&str> = accounts.iter().map(|a| a.username()).collect();
        assert!(names.contains(&"alice"));
        assert!(names.contains(&"bob"));
    }

    #[tokio::test]
    async fn record_login_updates_timestamp() {
        let (store, _dir) = open_test_store();
        let acct = store.create_account("alice", "secret").await.unwrap();
        assert_eq!(acct.last_login_at(), None);

        store.record_login(acct.id()).await.unwrap();

        let accounts = store.list_accounts().await.unwrap();
        let alice = accounts.iter().find(|a| a.username() == "alice").unwrap();
        assert!(alice.last_login_at().is_some());
    }
}
