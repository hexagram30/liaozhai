//! Argon2 hashing parameter configuration.

use liaozhai_core::constants;

/// `Argon2id` hashing parameters.
///
/// Defaults follow OWASP 2024 recommendations:
/// - `m_cost` = 19,456 KiB (19 MiB)
/// - `t_cost` = 2 iterations
/// - `p_cost` = 1 lane
#[derive(Debug, Clone, PartialEq)]
pub struct Argon2Params {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Argon2Params {
    pub fn new(m_cost: u32, t_cost: u32, p_cost: u32) -> Self {
        Self {
            m_cost,
            t_cost,
            p_cost,
        }
    }

    /// Fast parameters for tests only (~1 ms per hash).
    pub fn test_fast() -> Self {
        Self {
            m_cost: 256,
            t_cost: 1,
            p_cost: 1,
        }
    }

    /// Build an `argon2::Params` from these values.
    ///
    /// # Errors
    ///
    /// Returns an error if the parameter values are out of argon2's valid range.
    pub fn to_argon2_params(&self) -> Result<argon2::Params, argon2::Error> {
        argon2::Params::new(self.m_cost, self.t_cost, self.p_cost, None)
    }
}

impl Default for Argon2Params {
    fn default() -> Self {
        Self {
            m_cost: constants::DEFAULT_ARGON2_MEMORY_COST,
            t_cost: constants::DEFAULT_ARGON2_TIME_COST,
            p_cost: constants::DEFAULT_ARGON2_PARALLELISM,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_params_match_owasp() {
        let p = Argon2Params::default();
        assert_eq!(p.m_cost, 19_456);
        assert_eq!(p.t_cost, 2);
        assert_eq!(p.p_cost, 1);
    }

    #[test]
    fn to_argon2_params_succeeds() {
        let p = Argon2Params::test_fast();
        assert!(p.to_argon2_params().is_ok());
    }

    #[test]
    fn to_argon2_params_with_zero_rejects() {
        let p = Argon2Params::new(0, 0, 0);
        assert!(p.to_argon2_params().is_err());
    }
}
