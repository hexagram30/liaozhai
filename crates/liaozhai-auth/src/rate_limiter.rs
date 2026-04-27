//! Per-IP authentication rate limiter.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// In-memory per-IP authentication failure tracker using a sliding window.
///
/// Thread-safe via `Mutex<HashMap<...>>`. Shared across all connection
/// tasks via `Arc<AuthRateLimiter>`.
// TODO(M6): cap HashMap size with LRU eviction at ~10,000 entries.
#[derive(Debug)]
pub struct AuthRateLimiter {
    window: Duration,
    max_failures: usize,
    failures: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
}

impl AuthRateLimiter {
    pub fn new(window: Duration, max_failures: usize) -> Self {
        Self {
            window,
            max_failures,
            failures: Mutex::new(HashMap::new()),
        }
    }

    /// Returns `true` if this IP has exceeded the failure threshold
    /// within the current window.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn is_throttled(&self, ip: IpAddr) -> bool {
        let mut map = self.failures.lock().expect("rate limiter mutex poisoned");
        if let Some(deque) = map.get_mut(&ip) {
            prune(deque, self.window);
            deque.len() >= self.max_failures
        } else {
            false
        }
    }

    /// Record a failed authentication attempt from this IP.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn record_failure(&self, ip: IpAddr) {
        let mut map = self.failures.lock().expect("rate limiter mutex poisoned");
        let deque = map.entry(ip).or_default();
        prune(deque, self.window);
        deque.push_back(Instant::now());
    }

    /// Clear the failure history for this IP (called on successful auth).
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn reset(&self, ip: IpAddr) {
        let mut map = self.failures.lock().expect("rate limiter mutex poisoned");
        map.remove(&ip);
    }
}

fn prune(deque: &mut VecDeque<Instant>, window: Duration) {
    let cutoff = Instant::now().checked_sub(window).unwrap_or(Instant::now());
    while deque.front().is_some_and(|&t| t < cutoff) {
        deque.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn test_ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))
    }

    fn other_ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))
    }

    #[test]
    fn not_throttled_initially() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3);
        assert!(!limiter.is_throttled(test_ip()));
    }

    #[test]
    fn throttled_after_max_failures() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3);
        for _ in 0..3 {
            limiter.record_failure(test_ip());
        }
        assert!(limiter.is_throttled(test_ip()));
    }

    #[test]
    fn not_throttled_below_max() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3);
        for _ in 0..2 {
            limiter.record_failure(test_ip());
        }
        assert!(!limiter.is_throttled(test_ip()));
    }

    #[test]
    fn reset_clears_history() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3);
        for _ in 0..3 {
            limiter.record_failure(test_ip());
        }
        assert!(limiter.is_throttled(test_ip()));
        limiter.reset(test_ip());
        assert!(!limiter.is_throttled(test_ip()));
    }

    #[test]
    fn old_entries_pruned() {
        let limiter = AuthRateLimiter::new(Duration::from_millis(50), 3);
        for _ in 0..3 {
            limiter.record_failure(test_ip());
        }
        assert!(limiter.is_throttled(test_ip()));
        std::thread::sleep(Duration::from_millis(100));
        assert!(!limiter.is_throttled(test_ip()));
    }

    #[test]
    fn different_ips_tracked_independently() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3);
        for _ in 0..3 {
            limiter.record_failure(test_ip());
        }
        assert!(limiter.is_throttled(test_ip()));
        assert!(!limiter.is_throttled(other_ip()));
    }
}
