//! Exit-gesture PIN gate (spec P1-D2c): argon2id PIN verify + persisted
//! lockout/backoff. Pure logic, host-tested — never let PIN checking live in
//! the untestable Tauri layer.

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use serde::{Deserialize, Serialize};

/// Verify `pin` against a PHC-string argon2id hash. Any parse/verify failure → false
/// (never panics on attacker-influenced input); the crate's verify is constant-time.
pub fn verify_pin(pin: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(hash) => Argon2::default()
            .verify_password(pin.as_bytes(), &hash)
            .is_ok(),
        Err(_) => false,
    }
}

pub const FREE_ATTEMPTS: u32 = 3; // first N failures are free (fat-finger tolerance)
pub const BACKOFF_BASE_S: i64 = 5; // then 5s, 10s, 20s, ...
pub const BACKOFF_CAP_S: i64 = 3600; // capped at 1h

/// Persisted PIN-attempt lockout state (survives restart via serde).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Lockout {
    consecutive_failures: u32,
    blocked_until: Option<i64>, // unix seconds
}

pub enum Gate {
    Allowed,
    Blocked { until: i64 },
}

impl Lockout {
    /// Whether a PIN attempt is allowed right now (`now` = unix seconds, injected for
    /// deterministic tests).
    pub fn check(&self, now: i64) -> Gate {
        match self.blocked_until {
            Some(until) if now < until => Gate::Blocked { until },
            _ => Gate::Allowed,
        }
    }

    /// Record a failed attempt; exponential backoff kicks in after `FREE_ATTEMPTS`.
    pub fn record_failure(&mut self, now: i64) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= FREE_ATTEMPTS {
            // ponytail: off-by-one vs. the brief's literal reference (`> FREE_ATTEMPTS`,
            // `over - 1`) — that version never blocks after exactly FREE_ATTEMPTS
            // failures, contradicting `allowed_until_free_attempts_exhausted` below.
            // `>=` + no `-1` makes the block start the instant the free attempts run out.
            let over = self.consecutive_failures - FREE_ATTEMPTS; // 0,1,2,...
            let shift = over.min(20); // guard the shl from overflowing
            let wait = BACKOFF_BASE_S
                .saturating_mul(1i64 << shift)
                .min(BACKOFF_CAP_S);
            self.blocked_until = Some(now.saturating_add(wait));
        }
    }

    /// A correct PIN clears the lockout entirely.
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.blocked_until = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // A real argon2id PHC hash of the PIN "1234" (generated once with the argon2 crate;
    // pasted as a literal so the test needs no RNG at run time).
    // Generated via `cargo run -p kiosk-core --example gen_phc` (scratch binary, deleted
    // after use) so no RNG runs at test time.
    const PHC_1234: &str =
        "$argon2id$v=19$m=19456,t=2,p=1$fsrz9R8cQjyfR0fI1Unrsg$yP77kmD/LziluK9uN0QEZSSJDOlEYGSeb2X9qkT1dDI";

    #[test]
    fn correct_pin_verifies() {
        assert!(verify_pin("1234", PHC_1234));
    }
    #[test]
    fn wrong_pin_rejected() {
        assert!(!verify_pin("9999", PHC_1234));
    }
    #[test]
    fn malformed_phc_is_false_not_panic() {
        assert!(!verify_pin("1234", "not-a-phc"));
    }
    #[test]
    fn empty_pin_rejected() {
        assert!(!verify_pin("", PHC_1234));
    }

    #[test]
    fn allowed_until_free_attempts_exhausted() {
        let mut l = Lockout::default();
        for _ in 0..FREE_ATTEMPTS {
            assert!(matches!(l.check(0), Gate::Allowed));
            l.record_failure(0);
        }
        assert!(
            matches!(l.check(0), Gate::Blocked { .. }),
            "blocks after FREE_ATTEMPTS failures"
        );
    }
    #[test]
    fn backoff_is_monotonic_and_capped() {
        let mut l = Lockout::default();
        let mut prev = 0i64;
        for n in 0..12 {
            l.record_failure(0);
            if let Gate::Blocked { until } = l.check(0) {
                assert!(until >= prev, "backoff never shrinks (attempt {n})");
                assert!(until <= BACKOFF_CAP_S, "capped");
                prev = until;
            }
        }
    }
    #[test]
    fn block_expires_then_allows() {
        let mut l = Lockout::default();
        for _ in 0..=FREE_ATTEMPTS {
            l.record_failure(100);
        }
        let until = match l.check(100) {
            Gate::Blocked { until } => until,
            _ => panic!("blocked"),
        };
        assert!(
            matches!(l.check(until + 1), Gate::Allowed),
            "past the block window → allowed"
        );
    }
    #[test]
    fn success_resets_the_counter() {
        let mut l = Lockout::default();
        for _ in 0..=FREE_ATTEMPTS {
            l.record_failure(0);
        }
        l.record_success();
        assert!(
            matches!(l.check(0), Gate::Allowed),
            "success clears the lockout"
        );
    }
    #[test]
    fn survives_a_restart_via_serde() {
        let mut l = Lockout::default();
        for _ in 0..=FREE_ATTEMPTS {
            l.record_failure(500);
        }
        let json = serde_json::to_string(&l).unwrap();
        let reloaded: Lockout = serde_json::from_str(&json).unwrap();
        assert!(
            matches!(reloaded.check(500), Gate::Blocked { .. }),
            "reload mid-backoff still blocks"
        );
    }
}
