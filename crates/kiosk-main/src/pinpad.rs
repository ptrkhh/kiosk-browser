//! Technician PIN pad IPC + exit (P1-D2c Task 5, spec §3.5/§5.2): the Tauri command
//! the bundled `pinpad.html` calls. Adjudication itself ([`adjudicate`]) is pure and
//! host-tested; the security core (argon2id verify + the persisted backoff curve)
//! is Task 1's `kiosk_core::exit` — this module only wires it to an in-memory
//! `Lockout` (loaded from disk once at startup) and, on a correct PIN, the
//! sanctioned technician exit (`std::process::exit(86)`, spec exit-code-86).
//!
//! **In-memory `Lockout` is authoritative within a run; disk is only cross-restart
//! durability.** The command locks the managed `Mutex<Lockout>`, adjudicates against
//! THAT (never a fresh disk read), then persists best-effort. So a failing disk
//! write (full/locked `%ProgramData%` volume) still increments the in-memory
//! failure counter and engages backoff — SEC-05's brute-force protection cannot be
//! eroded by re-reading a stale pre-failure state off disk on the next attempt. The
//! `Mutex` also serializes Tauri's worker-pool-dispatched invokes, so two
//! near-simultaneous attempts can't both slip past the same counter value.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use kiosk_core::exit::{Gate, Lockout};
use serde::Serialize;

pub const LOCKOUT_FILE: &str = "exit-lockout.json";

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "result")]
pub enum PinResult {
    Ok,
    Blocked { until: i64 },
    Rejected,
}

/// State the `verify_pin` command needs, `manage`d once at Tauri setup (main.rs).
pub struct PinPadState {
    /// The effective exit-gesture `pin_hash` (Task 4's `gesture::effective_gesture`).
    /// `None` when the gesture is disabled (cfg-12) — Task 4 never navigates to the
    /// pad in that case, but the command below does not trust that and refuses on
    /// its own (no-fail-open: a missing/empty hash must never grant a no-PIN exit).
    pub pin_hash: Option<String>,
    pub data_dir: PathBuf,
    /// The AUTHORITATIVE lockout for this process run, seeded from disk once at
    /// construction. Every invoke adjudicates against this in-memory value (not a
    /// fresh disk read), so a failed persist can't roll the failure counter back.
    pub lockout: Mutex<Lockout>,
}

impl PinPadState {
    /// Build the managed state, seeding the in-memory lockout from disk ONCE
    /// (missing/corrupt → default, per [`load_lockout`]).
    pub fn new(pin_hash: Option<String>, data_dir: PathBuf) -> Self {
        let lockout = load_lockout(&data_dir);
        Self {
            pin_hash,
            data_dir,
            lockout: Mutex::new(lockout),
        }
    }
}

/// Pure adjudication (host-tested, TDD Step 1): the lockout gate is checked BEFORE
/// the hash is even looked at, so a correct PIN during an active lockout is still
/// `Blocked` — there is no path here that verifies a PIN while blocked.
pub fn adjudicate(lockout: &mut Lockout, pin: &str, phc: &str, now: i64) -> PinResult {
    match lockout.check(now) {
        Gate::Blocked { until } => PinResult::Blocked { until },
        Gate::Allowed => {
            if kiosk_core::exit::verify_pin(pin, phc) {
                lockout.record_success();
                PinResult::Ok
            } else {
                lockout.record_failure(now);
                PinResult::Rejected
            }
        }
    }
}

fn lockout_path(data_dir: &Path) -> PathBuf {
    data_dir.join(LOCKOUT_FILE)
}

/// Missing or corrupt → a fresh (unblocked) `Lockout`; a damaged lockout file must
/// never brick the technician exit path (same "absent/corrupt degrades to default"
/// posture as `kiosk_core::config::store::ConfigStore::load_last_good`).
fn load_lockout(data_dir: &Path) -> Lockout {
    std::fs::read_to_string(lockout_path(data_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Atomic persist: temp file + fsync-the-file + rename, the SAME idiom
/// `kiosk_core::config::store::ConfigStore::save_last_good` uses (SEC-05) — reused
/// verbatim, not hand-rolled, because a power cut mid-write must never leave a
/// half-written lockout file that either bricks the pad or silently drops the
/// backoff a technician has already earned by getting the PIN wrong.
fn save_lockout(data_dir: &Path, lockout: &Lockout) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let text = serde_json::to_string(lockout)?;
    let tmp = data_dir.join(format!("{LOCKOUT_FILE}.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, lockout_path(data_dir))?;
    // Best-effort directory fsync, same as the config store (ignored on platforms
    // where it errors, e.g. Windows directory handles).
    let _ = std::fs::File::open(data_dir).and_then(|d| d.sync_all());
    Ok(())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The Tauri command `pinpad.html` invokes (`invoke('verify_pin', {pin})`).
///
/// Namespacing note: this IS `pinpad::verify_pin`, the IPC command. It calls
/// `kiosk_core::exit::verify_pin` (the argon2id check) only indirectly, via
/// `adjudicate` — the two `verify_pin`s are never called interchangeably.
#[tauri::command]
pub fn verify_pin(pin: String, state: tauri::State<PinPadState>) -> PinResult {
    // No-fail-open (cfg-12): a missing effective pin_hash must never grant a
    // no-PIN exit, defensively, even though Task 4 never opens the pad in that case.
    let Some(phc) = state.pin_hash.as_deref() else {
        return PinResult::Rejected;
    };

    // Lock the AUTHORITATIVE in-memory lockout for the whole adjudicate+persist —
    // this both serializes concurrent invokes and keeps disk out of the decision.
    // A poisoned mutex (a prior invoke panicked mid-critical-section) must never
    // fail-open into unlimited attempts, so recover the inner value and keep going.
    let mut lockout = state
        .lockout
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let now = now_unix();
    let result = adjudicate(&mut lockout, &pin, phc, now);

    // Best-effort persist for cross-restart durability only; in-memory `lockout` is
    // authoritative within this run, so a failed write does NOT roll back the
    // counter we just advanced (that's the SEC-05 erosion this design closes).
    if let Err(e) = save_lockout(&state.data_dir, &lockout) {
        eprintln!("pinpad: failed to persist lockout: {e}");
    }

    // Exit AFTER the persist above: the success-reset lockout should be durable on
    // disk before the process that reset it goes away (spec exit-code-86 + SEC-05).
    if matches!(result, PinResult::Ok) {
        std::process::exit(86);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // The SAME real argon2id PHC hash of "1234" as Task 1's `kiosk_core::exit`
    // tests (copied verbatim, not regenerated) — both suites must agree on what
    // "the correct PIN" verifies against.
    const PHC_1234: &str =
        "$argon2id$v=19$m=19456,t=2,p=1$fsrz9R8cQjyfR0fI1Unrsg$yP77kmD/LziluK9uN0QEZSSJDOlEYGSeb2X9qkT1dDI";

    #[test]
    fn correct_pin_when_allowed_is_ok() {
        let mut l = Lockout::default();
        assert!(matches!(
            adjudicate(&mut l, "1234", PHC_1234, 0),
            PinResult::Ok
        ));
    }
    #[test]
    fn wrong_pin_records_failure_and_rejects() {
        let mut l = Lockout::default();
        assert!(matches!(
            adjudicate(&mut l, "0000", PHC_1234, 0),
            PinResult::Rejected
        ));
    }
    #[test]
    fn blocked_pin_does_not_even_check_hash() {
        let mut l = Lockout::default();
        for _ in 0..=kiosk_core::exit::FREE_ATTEMPTS {
            l.record_failure(0);
        }
        assert!(
            matches!(
                adjudicate(&mut l, "1234", PHC_1234, 0),
                PinResult::Blocked { .. }
            ),
            "a correct PIN during lockout is still blocked"
        );
    }

    #[test]
    fn lockout_round_trips_through_the_atomic_persist() {
        let dir = tempfile::tempdir().unwrap();
        let mut l = Lockout::default();
        for _ in 0..=kiosk_core::exit::FREE_ATTEMPTS {
            l.record_failure(0);
        }
        save_lockout(dir.path(), &l).unwrap();
        let reloaded = load_lockout(dir.path());
        assert!(
            matches!(reloaded.check(0), Gate::Blocked { .. }),
            "a persisted lockout must still be blocked after reload"
        );
    }

    #[test]
    fn missing_lockout_file_loads_as_default_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let l = load_lockout(dir.path());
        assert!(matches!(l.check(0), Gate::Allowed));
    }

    // The core of the Important review fix: one Lockout held ACROSS attempts (as
    // PinPadState's Mutex<Lockout> does per run) engages the lockout with NO disk
    // involved. If the command instead reloaded a stale state from disk each time
    // (the eroded design), the counter would never climb and this would stay
    // Rejected forever — unlimited brute force. The command's `state.lockout.lock()`
    // hands `adjudicate` this same persistent `&mut Lockout`; asserting it at the
    // state level is the cleanest host-testable proof without a full Tauri harness
    // (the `#[tauri::command]` wrapper + `tauri::State` extraction are not
    // unit-testable off a live app — see the report's manual-check note).
    #[test]
    fn in_memory_lockout_held_across_attempts_engages_backoff_without_disk() {
        let mut lockout = Lockout::default();
        // Wrong PIN, repeated, against the SAME in-memory Lockout — no persist, no reload.
        for _ in 0..kiosk_core::exit::FREE_ATTEMPTS {
            assert!(matches!(
                adjudicate(&mut lockout, "0000", PHC_1234, 0),
                PinResult::Rejected
            ));
        }
        // Once the free attempts are spent, the same held Lockout now blocks — even a
        // CORRECT PIN — purely from in-memory state.
        assert!(
            matches!(
                adjudicate(&mut lockout, "1234", PHC_1234, 0),
                PinResult::Blocked { .. }
            ),
            "an in-memory lockout held across attempts must engage backoff with no disk read"
        );
    }
}
