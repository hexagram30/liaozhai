//! Account types.

use std::time::{SystemTime, UNIX_EPOCH};

use liaozhai_core::id::AccountId;

/// An account in the system.
///
/// Never carries the password hash — that exists only within `AccountStore`
/// internals and is dropped after verification.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    id: AccountId,
    username: String,
    created_at: i64,
    last_login_at: Option<i64>,
}

impl Account {
    /// Create an in-memory account with the current timestamp.
    ///
    /// Used for tests and non-DB construction sites.
    ///
    /// # Panics
    ///
    /// Panics if the system clock is before the Unix epoch.
    #[expect(clippy::cast_possible_wrap)]
    pub fn new(username: impl Into<String>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_secs() as i64;
        Self {
            id: AccountId::new(),
            username: username.into(),
            created_at: now,
            last_login_at: None,
        }
    }

    /// Construct an Account from database row fields.
    pub fn from_row(
        id: AccountId,
        username: String,
        created_at: i64,
        last_login_at: Option<i64>,
    ) -> Self {
        Self {
            id,
            username,
            created_at,
            last_login_at,
        }
    }

    pub fn id(&self) -> AccountId {
        self.id
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn created_at(&self) -> i64 {
        self.created_at
    }

    pub fn last_login_at(&self) -> Option<i64> {
        self.last_login_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_new_getters() {
        let acct = Account::new("alice");
        assert_eq!(acct.username(), "alice");
        assert_eq!(acct.id(), acct.id());
        assert!(acct.created_at() > 0);
        assert_eq!(acct.last_login_at(), None);
    }

    #[test]
    fn account_from_row_getters() {
        let id = AccountId::new();
        let acct = Account::from_row(id, "bob".into(), 1000, Some(2000));
        assert_eq!(acct.id(), id);
        assert_eq!(acct.username(), "bob");
        assert_eq!(acct.created_at(), 1000);
        assert_eq!(acct.last_login_at(), Some(2000));
    }
}
