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

/// Kills `child` so no orphaned process is left holding the single-instance
/// mutex/IPC pipe. Does not send anything on `tx`: the caller of
/// `spawn_main` is solely responsible for reporting the `Err` result as a
/// synthetic `ChildExited{-1}`, and this helper must not race it with a
/// second one.
#[cfg(windows)]
fn kill_orphan(mut child: Child) {
    let _ = child.kill();
}

/// Spawns the supervised child (`exe --config <config_dir> [--safe]`),
/// pushes `Event::Spawned` immediately, and starts a detached waiter
/// thread that sends `Event::ChildExited` once the child exits.
///
/// `pipe_name` is passed to the child as the `KIOSK_HEARTBEAT_PIPE`
/// environment variable (the child cannot derive the launcher's PID-suffixed
/// heartbeat pipe name itself — see `pipe`'s module docs). It is set on the
/// `Command` this function builds, so it affects only the child; the
/// launcher's own environment is never mutated.
///
/// Returns the live `Child` handle to the caller. `Child::wait` takes
/// `&mut self`, so the waiter thread is instead given an independent, owned
/// duplicate of the underlying process handle (`BorrowedHandle::
/// try_clone_to_owned`, stable std, equivalent to `DuplicateHandle` with
/// `DUPLICATE_SAME_ACCESS`) that it alone waits on; the `OwnedHandle` closes
/// itself on drop. The caller's own `Child` is unaffected regardless of when
/// the caller drops it.
///
/// # `Err` contract
/// Whenever this function returns `Err` — whether `cmd.spawn()` itself
/// failed, the process handle could not be duplicated, or the waiter thread
/// could not be created — it has sent nothing on `tx` that represents an
/// exit, and no supervised child exists. The caller (Task 4) is solely
/// responsible for feeding a synthetic `Event::ChildExited{code: -1, ..}`
/// in every `Err` case so the FSM's backoff governs retries; that keeps
/// "one spawn attempt, one exit event" true regardless of which stage
/// failed. A `Event::Spawned` may still have been sent before the failure
/// (handle-duplication and thread-creation failures happen after spawn
/// succeeded, so the child is killed to avoid orphaning it) — that is
/// harmless, since a `Spawned` followed by the caller's `ChildExited{-1}`
/// is exactly the sequence a fast crash produces.
#[cfg(windows)]
pub fn spawn_main(
    exe: &Path,
    config_dir: &Path,
    safe: bool,
    pipe_name: &str,
    tx: Sender<Event>,
) -> io::Result<Child> {
    use std::os::windows::io::AsHandle;

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--config").arg(config_dir);
    cmd.env("KIOSK_HEARTBEAT_PIPE", pipe_name);
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
            // exhaustion). Kill the child since nothing will ever observe
            // its real exit, and return `Err` with no traffic on `tx`; the
            // caller supplies the one `ChildExited{-1}`.
            kill_orphan(child);
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
            // Thread creation failing is equally rare and equally fatal to
            // ever observing this child's exit. Kill the child and return
            // `Err` with no traffic on `tx`; the caller supplies the one
            // `ChildExited{-1}`.
            kill_orphan(child);
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
    _pipe_name: &str,
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

        let child = spawn_main(exe, &config_dir, true, r"\\.\pipe\kiosk-heartbeat-test", tx)
            .expect("where.exe should spawn");
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

        let result = spawn_main(
            exe,
            &config_dir,
            false,
            r"\\.\pipe\kiosk-heartbeat-test",
            tx,
        );
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
