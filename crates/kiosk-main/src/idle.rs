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

use kiosk_core::app::state::Event as AppEvent;

/// Pure latch decision: fire iff idle timeout is enabled (`threshold != 0`), idle time
/// has reached/crossed it, and this episode hasn't already fired. The caller clears
/// `already_fired` once idle drops back below `threshold` (activity resumed), so the
/// next crossing fires again.
pub fn should_fire(idle_secs: u64, threshold: u64, already_fired: bool) -> bool {
    threshold != 0 && idle_secs >= threshold && !already_fired
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
    use windows::Win32::System::SystemInformation::GetTickCount64;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    use super::{should_fire, AppEvent};

    /// Seconds since the last system-wide keyboard/mouse input, via `GetLastInputInfo`
    /// (`dwTime` = tick count at last input) and `GetTickCount64` (current tick count).
    /// Both counters share the same base (system boot), so their difference is a plain
    /// elapsed-ms count regardless of `GetTickCount64`'s ~49.7-day wraparound-free 64-bit
    /// range. Falls back to "not idle" (0) if the Win32 call fails, rather than risking a
    /// false idle-fire off garbage data.
    fn idle_secs() -> u64 {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if unsafe { GetLastInputInfo(&mut info) }.as_bool() {
            (unsafe { GetTickCount64() }).saturating_sub(info.dwTime as u64) / 1000
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
    use super::should_fire;

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
}
