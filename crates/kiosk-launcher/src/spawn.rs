//! Spawns the supervised child process and waits for its exit on a
//! detached thread, translating process I/O into `watchdog::Event`s.
//! Windows/P1 only — see the `not(windows)` stub at the bottom.
//!
//! # Dead code scope
//! `#[allow(dead_code)]` here is temporary: `main.rs` does not yet call
//! `spawn_main`. Remove this allow when Task 4 (`LauncherSink` + assembly)
//! wires it in.
#![allow(dead_code)]

use crate::clock::now;
use kiosk_core::watchdog::Event;
use std::io;
use std::path::Path;
use std::process::Child;
use std::sync::mpsc::Sender;

/// Raw kernel32 declarations for the two calls std doesn't expose: waiting
/// on a HANDLE and reading its exit code. `kernel32.lib` is already linked
/// into every Windows Rust binary, so no extra dependency is needed.
#[cfg(windows)]
#[allow(non_snake_case)]
mod win32 {
    use std::os::windows::io::RawHandle;

    pub const INFINITE: u32 = 0xFFFF_FFFF;
    pub const WAIT_FAILED: u32 = 0xFFFF_FFFF;

    extern "system" {
        pub fn WaitForSingleObject(h_handle: RawHandle, dw_milliseconds: u32) -> u32;
        pub fn GetExitCodeProcess(h_process: RawHandle, lp_exit_code: *mut u32) -> i32;
    }
}

/// Sends a synthetic `ChildExited{code: -1, ..}` and kills `child` so no
/// orphaned process is left holding the single-instance mutex/IPC pipe
/// while the FSM's backoff spawns a replacement.
#[cfg(windows)]
fn report_dead_and_kill(tx: &Sender<Event>, mut child: Child) {
    let _ = child.kill();
    let _ = tx.send(Event::ChildExited {
        code: -1,
        at: now(),
    });
}

/// Spawns the supervised child (`exe --config <config_dir> [--safe]`),
/// pushes `Event::Spawned` immediately, and starts a detached waiter
/// thread that sends `Event::ChildExited` once the child exits.
///
/// Returns the live `Child` handle to the caller. `Child::wait` takes
/// `&mut self`, and there is no safe way to share one `Child` between the
/// caller and a waiter thread, so the waiter is instead given an
/// independent, owned duplicate of the underlying process handle
/// (`BorrowedHandle::try_clone_to_owned`, stable std, equivalent to
/// `DuplicateHandle` with `DUPLICATE_SAME_ACCESS`) that it alone waits on;
/// the `OwnedHandle` closes itself on drop. The caller's own `Child` is
/// unaffected regardless of when the caller drops it.
///
/// On spawn failure, returns the `io::Error` without touching `tx` or
/// panicking; the caller is responsible for feeding a synthetic
/// `Event::ChildExited{code: -1, ..}` so the FSM's backoff governs retries.
///
/// If the process handle cannot be duplicated, or the waiter thread cannot
/// be created, the child is killed immediately and a synthetic
/// `Event::ChildExited{code: -1, ..}` is sent instead: a `Child` this
/// function can no longer observe must not be handed back as if it were
/// healthy, since the caller (and Task 4's supervise loop) will treat the
/// returned handle as disposable and may drop it, which does not kill a
/// Windows process.
#[cfg(windows)]
pub fn spawn_main(
    exe: &Path,
    config_dir: &Path,
    safe: bool,
    tx: Sender<Event>,
) -> io::Result<Child> {
    use std::os::windows::io::AsHandle;

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--config").arg(config_dir);
    if safe {
        cmd.arg("--safe");
    }
    let child = cmd.spawn()?;

    // send is best-effort: if the loop's receiver is gone, there is nothing
    // left to notify and no reason to panic the caller.
    let _ = tx.send(Event::Spawned { at: now() });

    let dup = match child.as_handle().try_clone_to_owned() {
        Ok(dup) => dup,
        Err(_) => {
            // Duplication failing is exceedingly rare (e.g. handle-table
            // exhaustion). Report it as an immediate exit so backoff
            // governs, and kill the child since nothing will ever observe
            // its real exit.
            report_dead_and_kill(&tx, child);
            return Err(io::Error::other(
                "spawn_main: failed to duplicate child process handle",
            ));
        }
    };

    let waiter_tx = tx.clone();
    let spawned = std::thread::Builder::new()
        .name("kiosk-launcher-child-waiter".into())
        .spawn(move || {
            use std::os::windows::io::AsRawHandle;
            let handle = dup.as_raw_handle();
            // Safety: `dup` is an `OwnedHandle` this thread exclusively
            // owns for its lifetime (moved in, not shared); `handle` stays
            // valid for the duration of these two calls because `dup` is
            // not dropped until after they return.
            let code = unsafe {
                let wait = win32::WaitForSingleObject(handle, win32::INFINITE);
                if wait == win32::WAIT_FAILED {
                    -1
                } else {
                    let mut code: u32 = 0;
                    if win32::GetExitCodeProcess(handle, &mut code) == 0 {
                        -1
                    } else {
                        code as i32
                    }
                }
            };
            let _ = waiter_tx.send(Event::ChildExited { code, at: now() });
        });

    match spawned {
        Ok(_) => Ok(child),
        Err(_) => {
            // Thread creation failing is as rare as handle duplication
            // failing, and equally fatal to ever observing this child's
            // exit: report it the same way.
            report_dead_and_kill(&tx, child);
            Err(io::Error::other(
                "spawn_main: failed to create child-waiter thread",
            ))
        }
    }
}

/// Non-Windows stub: kiosk-launcher's process-spawn model relies on
/// duplicating a Windows process handle (see the `cfg(windows)` impl
/// above), so on other host platforms (dev-only; the kiosk target is
/// Windows x64) this simply reports "unsupported" rather than spawning.
#[cfg(not(windows))]
pub fn spawn_main(
    _exe: &Path,
    _config_dir: &Path,
    _safe: bool,
    _tx: Sender<Event>,
) -> io::Result<Child> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "kiosk-launcher spawn_main is Windows-only",
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// Real Windows smoke test (brief Step 3): spawns `where.exe` with the
    /// exact argument shape `spawn_main` always appends. `where.exe`
    /// rejects `--config <dir> --safe` as an invalid pattern and exits
    /// non-zero fast — deterministic, no network, no display needed. The
    /// exact nonzero code is host/path-dependent (varies with how the
    /// TEMP path tokenizes as a search pattern), so only nonzero is
    /// asserted; the behavior under test is the waiter thread, not
    /// `where.exe`'s argument parser.
    #[test]
    fn spawn_main_reports_spawned_then_child_exited() {
        let (tx, rx) = mpsc::channel();
        let exe = Path::new("where.exe");
        let config_dir = std::env::temp_dir();

        let child = spawn_main(exe, &config_dir, true, tx).expect("where.exe should spawn");
        drop(child); // caller's copy; dropping closes its own HANDLE, not the process

        let spawned = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("expected Event::Spawned");
        assert!(matches!(spawned, Event::Spawned { .. }));

        let exited = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("expected Event::ChildExited");
        assert!(
            matches!(exited, Event::ChildExited { code, .. } if code != 0),
            "where.exe with an invalid pattern exits nonzero, got {exited:?}"
        );
    }

    /// Contract Task 4 depends on: a nonexistent exe produces `Err`, sends
    /// nothing on `tx`, and does not panic.
    #[test]
    fn spawn_main_nonexistent_exe_is_err_with_no_traffic() {
        let (tx, rx) = mpsc::channel();
        let exe = Path::new("this-exe-does-not-exist-kiosk-launcher-test.exe");
        let config_dir = std::env::temp_dir();

        let result = spawn_main(exe, &config_dir, false, tx);
        assert!(result.is_err());
        // No event was sent; either the receive times out, or (since `tx`
        // was dropped along with the failed attempt) the channel reports
        // disconnected. Either way, no traffic was ever delivered.
        assert!(matches!(
            rx.recv_timeout(std::time::Duration::from_millis(200)),
            Err(mpsc::RecvTimeoutError::Timeout) | Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }
}
