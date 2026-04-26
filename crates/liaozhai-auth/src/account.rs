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
    pub fn new(username: String) -> Self {
        Self {
            id: AccountId::new(),
            username,
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
        let acct = Account::new("alice".into());
        assert_eq!(acct.username(), "alice");
        let _ = acct.id(); // just verify it doesn't panic
    }
}
