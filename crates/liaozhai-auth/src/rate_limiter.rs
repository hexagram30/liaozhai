//! Per-IP authentication rate limiter.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// In-memory per-IP authentication failure tracker using a sliding window.
///
/// Thread-safe via `Mutex<HashMap<...>>`. Shared across all connection
/// tasks via `Arc<AuthRateLimiter>`. The `HashMap` is bounded at
/// `max_entries`; when full, the entry with the oldest most-recent
/// failure is evicted before inserting a new IP.
#[derive(Debug)]
pub struct AuthRateLimiter {
    window: Duration,
    max_failures: usize,
    max_entries: usize,
    failures: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
}

impl AuthRateLimiter {
    pub fn new(window: Duration, max_failures: usize, max_entries: usize) -> Self {
        Self {
            window,
            max_failures,
            max_entries,
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
    /// If the map is at capacity and this IP is new, the entry with the
    /// oldest most-recent failure is evicted first.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn record_failure(&self, ip: IpAddr) {
        let mut map = self.failures.lock().expect("rate limiter mutex poisoned");

        if !map.contains_key(&ip) && map.len() >= self.max_entries {
            if let Some(oldest_ip) = find_oldest_entry(&map) {
                map.remove(&oldest_ip);
            }
        }

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

// O(n) walk — only runs when inserting past the cap, which only
// happens under high attack pressure with many distinct IPs.
fn find_oldest_entry(map: &HashMap<IpAddr, VecDeque<Instant>>) -> Option<IpAddr> {
    map.iter()
        .filter_map(|(ip, deque)| deque.back().map(|t| (*ip, *t)))
        .min_by_key(|(_, t)| *t)
        .map(|(ip, _)| ip)
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

    fn ip_n(n: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, n))
    }

    #[test]
    fn not_throttled_initially() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3, 10_000);
        assert!(!limiter.is_throttled(test_ip()));
    }

    #[test]
    fn throttled_after_max_failures() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3, 10_000);
        for _ in 0..3 {
            limiter.record_failure(test_ip());
        }
        assert!(limiter.is_throttled(test_ip()));
    }

    #[test]
    fn not_throttled_below_max() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3, 10_000);
        for _ in 0..2 {
            limiter.record_failure(test_ip());
        }
        assert!(!limiter.is_throttled(test_ip()));
    }

    #[test]
    fn reset_clears_history() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3, 10_000);
        for _ in 0..3 {
            limiter.record_failure(test_ip());
        }
        assert!(limiter.is_throttled(test_ip()));
        limiter.reset(test_ip());
        assert!(!limiter.is_throttled(test_ip()));
    }

    #[test]
    fn old_entries_pruned() {
        let limiter = AuthRateLimiter::new(Duration::from_millis(50), 3, 10_000);
        for _ in 0..3 {
            limiter.record_failure(test_ip());
        }
        assert!(limiter.is_throttled(test_ip()));
        std::thread::sleep(Duration::from_millis(100));
        assert!(!limiter.is_throttled(test_ip()));
    }

    #[test]
    fn different_ips_tracked_independently() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3, 10_000);
        for _ in 0..3 {
            limiter.record_failure(test_ip());
        }
        assert!(limiter.is_throttled(test_ip()));
        assert!(!limiter.is_throttled(other_ip()));
    }

    #[test]
    fn eviction_when_at_cap() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3, 3);
        // Fill to capacity with 3 IPs
        limiter.record_failure(ip_n(1));
        std::thread::sleep(Duration::from_millis(10));
        limiter.record_failure(ip_n(2));
        std::thread::sleep(Duration::from_millis(10));
        limiter.record_failure(ip_n(3));

        // Insert a 4th — should evict ip_n(1) (oldest most-recent failure)
        limiter.record_failure(ip_n(4));

        let map = limiter.failures.lock().unwrap();
        assert_eq!(map.len(), 3);
        assert!(!map.contains_key(&ip_n(1)));
        assert!(map.contains_key(&ip_n(4)));
    }

    #[test]
    fn no_eviction_below_cap() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3, 5);
        limiter.record_failure(ip_n(1));
        limiter.record_failure(ip_n(2));

        let map = limiter.failures.lock().unwrap();
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn existing_ip_not_evicted() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3, 2);
        limiter.record_failure(ip_n(1));
        limiter.record_failure(ip_n(2));

        // Record another failure for an existing IP — no eviction
        limiter.record_failure(ip_n(1));

        let map = limiter.failures.lock().unwrap();
        assert_eq!(map.len(), 2);
        assert!(map.contains_key(&ip_n(1)));
        assert!(map.contains_key(&ip_n(2)));
    }
}
