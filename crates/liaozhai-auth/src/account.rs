//! Account types and authentication API.
//!
//! M1: Placeholder types only. Implementation lands in M4.

use liaozhai_core::id::AccountId;

/// An account in the system.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    id: AccountId,
    username: String,
}

impl Account {
    pub fn new(username: impl Into<String>) -> Self {
        Self {
            id: AccountId::new(),
            username: username.into(),
        }
    }

    pub fn id(&self) -> AccountId {
        self.id
    }

    pub fn username(&self) -> &str {
        &self.username
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_getters() {
        let acct = Account::new("alice");
        assert_eq!(acct.username(), "alice");
        // id() is stable across calls (idempotent getter, not a fresh random per call).
        assert_eq!(acct.id(), acct.id());
    }
}
