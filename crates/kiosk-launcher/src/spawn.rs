//! Spawns the supervised child process and waits for its exit on a
//! detached thread, translating process I/O into `watchdog::Event`s.
//! Windows/P1 only — see the `not(windows)` stub at the bottom.
//!
//! # Dead code scope
//! `#[allow(dead_code)]` here is temporary: `main.rs` does not yet call
//! `spawn_main`. Remove this allow when Task 4 (`LauncherSink` + assembly)
//! wires it in.
#![allow(dead_code)]

use kiosk_core::watchdog::Event;
use std::io;
use std::path::Path;
use std::process::Child;
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_secs()
}

/// Raw kernel32 declarations for the one thing std doesn't expose: an
/// independent, waitable duplicate of a process HANDLE. `kernel32.lib` is
/// already linked into every Windows Rust binary, so no extra dependency
/// is needed for these four calls.
#[cfg(windows)]
#[allow(non_snake_case)]
mod win32 {
    use std::os::windows::io::RawHandle;

    pub const DUPLICATE_SAME_ACCESS: u32 = 0x0000_0002;
    pub const INFINITE: u32 = 0xFFFF_FFFF;

    extern "system" {
        pub fn GetCurrentProcess() -> RawHandle;
        pub fn DuplicateHandle(
            h_source_process: RawHandle,
            h_source: RawHandle,
            h_target_process: RawHandle,
            lp_target: *mut RawHandle,
            dw_desired_access: u32,
            b_inherit: i32,
            dw_options: u32,
        ) -> i32;
        pub fn WaitForSingleObject(h_handle: RawHandle, dw_milliseconds: u32) -> u32;
        pub fn GetExitCodeProcess(h_process: RawHandle, lp_exit_code: *mut u32) -> i32;
        pub fn CloseHandle(h_object: RawHandle) -> i32;
    }
}

/// Spawns the supervised child (`exe --config <config_dir> [--safe]`),
/// pushes `Event::Spawned` immediately, and starts a detached waiter
/// thread that sends `Event::ChildExited` once the child exits.
///
/// Returns the live `Child` handle to the caller. `Child::wait` takes
/// `&mut self`, and there is no safe way to share one `Child` between the
/// caller and a waiter thread, so the waiter is instead given an
/// independent duplicate of the underlying process HANDLE
/// (`DuplicateHandle`) that it alone waits on and closes; the caller's own
/// `Child`/handle is unaffected regardless of when the caller drops it.
///
/// On spawn failure, returns the `io::Error` without touching `tx` or
/// panicking; the caller is responsible for feeding a synthetic
/// `Event::ChildExited{code: -1, ..}` so the FSM's backoff governs retries.
#[cfg(windows)]
pub fn spawn_main(
    exe: &Path,
    config_dir: &Path,
    safe: bool,
    tx: Sender<Event>,
) -> io::Result<Child> {
    use std::os::windows::io::AsRawHandle;

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--config").arg(config_dir);
    if safe {
        cmd.arg("--safe");
    }
    let child = cmd.spawn()?;

    // send is best-effort: if the loop's receiver is gone, there is nothing
    // left to notify and no reason to panic the caller.
    let _ = tx.send(Event::Spawned { at: now() });

    let source = child.as_raw_handle();
    let mut dup = std::ptr::null_mut();
    // Safety: `source` is a valid open HANDLE owned by `child`, which
    // outlives this call. `dup` receives a brand-new, independently
    // closeable HANDLE to the same process object.
    let duplicated = unsafe {
        win32::DuplicateHandle(
            win32::GetCurrentProcess(),
            source,
            win32::GetCurrentProcess(),
            &mut dup,
            0,
            0,
            win32::DUPLICATE_SAME_ACCESS,
        )
    };

    if duplicated == 0 {
        // Duplication failing is exceedingly rare (e.g. handle-table
        // exhaustion). Report it as an immediate exit so backoff governs
        // rather than silently never observing this child's exit.
        let _ = tx.send(Event::ChildExited {
            code: -1,
            at: now(),
        });
        return Ok(child);
    }

    // Safety: `dup` is a plain HANDLE value (an integer-sized pointer with
    // no aliasing/thread-affinity requirements from the OS); wrapping it
    // lets it cross the `thread::spawn` boundary, which otherwise refuses
    // raw pointers.
    struct SendableHandle(std::os::windows::io::RawHandle);
    unsafe impl Send for SendableHandle {}
    let dup = SendableHandle(dup);

    std::thread::spawn(move || {
        // Force capture of the whole `SendableHandle` (not just its raw
        // pointer field) — 2021-edition disjoint closure captures would
        // otherwise capture `dup.0` directly and bypass its `unsafe impl
        // Send`.
        let dup = dup;
        let dup = dup.0;
        // Safety: `dup` is the independent HANDLE duplicated above; this
        // thread is its sole owner and closes it exactly once, below.
        let code = unsafe {
            win32::WaitForSingleObject(dup, win32::INFINITE);
            let mut code: u32 = 0;
            win32::GetExitCodeProcess(dup, &mut code);
            win32::CloseHandle(dup);
            code
        };
        let _ = tx.send(Event::ChildExited {
            code: code as i32,
            at: now(),
        });
    });

    Ok(child)
}

/// Non-Windows stub: kiosk-launcher's process-spawn model relies on
/// duplicating a Windows process HANDLE (see the `cfg(windows)` impl
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
    /// fast with code 2 — deterministic, no network, no display needed.
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
            matches!(exited, Event::ChildExited { code: 2, .. }),
            "where.exe with an invalid pattern exits 2, got {exited:?}"
        );
    }
}
