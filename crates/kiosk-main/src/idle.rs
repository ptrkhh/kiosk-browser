//! Native idle timer → `AppEvent::IdleExpired` (P1-D2c Task 3, spec §3.5).
//!
//! Windows has no per-window "idle" event, so this polls system-wide last-input time
//! (`GetLastInputInfo`) once a second and emits `IdleExpired` when idle time crosses
//! the operator's `idle_reset_seconds` threshold — UNCONDITIONALLY, with no FSM-state
//! check here: `kiosk_core::app::state::Machine` already no-ops `IdleExpired` outside
//! `Online` (P1-D1 rule 9), so re-checking state here would just be a second, drifting
//! copy of that gate.
//!
//! The one thing this module owns is the LATCH ([`should_fire`]): fire once when idle
//! crosses the threshold, then stay quiet until activity resumes (idle drops back below
//! threshold), so a kiosk left untouched for hours doesn't re-fire every second.
//! `threshold == 0` means the feature is off — never fires (spec: `idle_reset_seconds:
//! 0` disables idle reset).
//!
//! SEC-09 final review: this task is never cancelled by a credential-DACL violation
//! (its `cancel` is the top-level shutdown token, not `main::fetch_probe_cancel`), so
//! it keeps `tx.send`ing `IdleExpired` into `driver::run`'s channel after a violation
//! exactly as before one. That's intentional, not a leak: `driver::run` also stays
//! alive post-violation (see `driver::SafeLatchedSink`'s doc comment), so these events
//! are still drained and handled by the FSM — `Online + idle_clear` still emits
//! `Effect::ClearProfile`, which the latch deliberately lets through, and any
//! `Navigate`/`ShowVideo` the FSM might otherwise emit is blocked at the sink, not by
//! starving this producer.

use kiosk_core::app::state::Event as AppEvent;

/// Pure latch decision: fire iff idle timeout is enabled (`threshold != 0`), idle time
/// has reached/crossed it, and this episode hasn't already fired. The caller clears
/// `already_fired` once idle drops back below `threshold` (activity resumed), so the
/// next crossing fires again.
pub fn should_fire(idle_secs: u64, threshold: u64, already_fired: bool) -> bool {
    threshold != 0 && idle_secs >= threshold && !already_fired
}

/// Idle seconds from two 32-bit tick samples (ms since boot), wrap-safe. Both
/// `GetLastInputInfo`'s `dwTime` AND `GetTickCount` are 32-bit `DWORD`s that wrap
/// every ~49.7 days, so `now.wrapping_sub(last)` cancels the wrap correctly as long as
/// the true idle interval is under 49.7 days (always true for any real reset
/// threshold). MUST stay same-width: subtracting the 32-bit `dwTime` from 64-bit
/// `GetTickCount64` inflates the result by k*2^32 ms permanently after the first
/// wraparound (~50 days uptime), wedging idle_secs at billions so the latch never
/// re-arms and idle reset silently dies for the rest of uptime.
pub fn idle_secs_from_ticks(now_tick_ms32: u32, last_input_tick_ms32: u32) -> u64 {
    (now_tick_ms32.wrapping_sub(last_input_tick_ms32) / 1000) as u64
}

#[cfg(windows)]
pub async fn run(
    threshold: u64,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
    cancel: tokio_util::sync::CancellationToken,
) {
    windows_impl::run(threshold, tx, cancel).await;
}

#[cfg(not(windows))]
pub async fn run(
    _threshold: u64,
    _tx: tokio::sync::mpsc::Sender<AppEvent>,
    _cancel: tokio_util::sync::CancellationToken,
) {
    eprintln!("idle: only implemented on Windows; idle reset will never fire");
}

#[cfg(windows)]
mod windows_impl {
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    use super::{idle_secs_from_ticks, should_fire, AppEvent};

    /// Seconds since the last system-wide keyboard/mouse input, via `GetLastInputInfo`
    /// (`dwTime` = 32-bit tick count at last input) and `GetTickCount` (32-bit current
    /// tick count) — same-width so [`idle_secs_from_ticks`]'s wrapping subtraction is
    /// correct across the ~49.7-day `DWORD` wraparound. Falls back to "not idle" (0) if
    /// the Win32 call fails, rather than risking a false idle-fire off garbage data.
    fn idle_secs() -> u64 {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if unsafe { GetLastInputInfo(&mut info) }.as_bool() {
            idle_secs_from_ticks(unsafe { GetTickCount() }, info.dwTime)
        } else {
            0
        }
    }

    /// Polls once a second; on a fresh crossing sends `IdleExpired` and latches, and
    /// clears the latch once activity brings idle time back under `threshold` — cancel-
    /// aware so it never outlives shutdown.
    pub async fn run(threshold: u64, tx: mpsc::Sender<AppEvent>, cancel: CancellationToken) {
        let mut already_fired = false;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
            }
            let secs = idle_secs();
            if should_fire(secs, threshold, already_fired) {
                let _ = tx.try_send(AppEvent::IdleExpired);
                already_fired = true;
            } else if secs < threshold {
                already_fired = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{idle_secs_from_ticks, should_fire};

    #[test]
    fn fires_once_when_idle_exceeds_threshold() {
        assert!(should_fire(200, 180, false)); // crossed, not yet fired -> fire
        assert!(!should_fire(200, 180, true)); // already fired this episode -> no repeat
        assert!(!should_fire(10, 180, false)); // below threshold -> no
    }

    #[test]
    fn threshold_zero_never_fires() {
        assert!(!should_fire(9999, 0, false)); // 0 = off
    }

    #[test]
    fn idle_secs_is_wrap_safe_across_the_32bit_tick_boundary() {
        // No wrap, ordinary case.
        assert_eq!(idle_secs_from_ticks(200_000, 20_000), 180); // 180 s idle

        // The wraparound the old 64-vs-32 subtraction got wrong: last input just
        // before the u32 boundary, `now` just past it. True idle is ~1 s (5 ms +
        // 995 ms), NOT billions. Fails against `GetTickCount64() - dwTime`, passes
        // with same-width wrapping arithmetic.
        assert_eq!(idle_secs_from_ticks(5, u32::MAX - 995), 1);
    }
}
