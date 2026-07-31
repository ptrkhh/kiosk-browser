//! Shared "read the clock once" helper for the edges (spawn/timer). The
//! FSM itself never reads a clock; only these edge modules do, and both
//! need the same panic-free behavior on a pre-1970 clock.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current unix time in seconds. Never panics: a dead/misconfigured RTC
/// that reports a pre-epoch time yields `0` rather than crashing the
/// launcher.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
