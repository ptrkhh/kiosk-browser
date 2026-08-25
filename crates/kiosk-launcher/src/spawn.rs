//! Spawns the supervised child process and waits for its exit on a
//! detached thread, translating process I/O into `watchdog::Event`s.
//! Windows/P1 only — see the `not(windows)` stub at the bottom.

use crate::clock::now;
use kiosk_core::watchdog::Event;
use std::io;
use std::path::Path;
#[cfg(windows)]
use std::process::Child;
use std::sync::mpsc::Sender;

#[cfg(unix)]
pub(crate) fn exit_code_of(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => code,
        (None, Some(signo)) => 128 + signo,
        (None, None) => -2,
    }
}

#[cfg(windows)]
pub type ChildHandle = Child;

#[cfg(unix)]
pub struct ChildHandle {
    pub(crate) pidfd: Option<std::os::fd::OwnedFd>,
    pub(crate) exited: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) pid: u32,
}

impl ChildHandle {
    pub(crate) fn id(&self) -> u32 {
        #[cfg(windows)]
        {
            Child::id(self)
        }
        #[cfg(unix)]
        {
            self.pid
        }
    }
}

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

/// How long [`kill_and_wait`] waits for a killed child to actually die.
///
/// ponytail: a bounded wait, not `Child::wait`'s `INFINITE`. The ceiling is
/// that a process wedged in an uninterruptible kernel wait (a hung GPU/display
/// driver is the realistic case on a kiosk running WebView2) is given up on
/// after this long, and the caller proceeds with its handles possibly still
/// open. That degrades to today's behaviour — a racy spool rename — instead of
/// parking the single supervise thread forever, which would stop the kiosk
/// being supervised at all. Raise it if a real device is ever seen needing more.
#[cfg(windows)]
const KILL_WAIT_MS: u32 = 5_000;

/// Kills `child` and waits (bounded, [`KILL_WAIT_MS`]) for it to actually go
/// away.
///
/// `Child::kill` is `TerminateProcess`, which returns BEFORE the kernel has
/// torn the process down and closed its handles. Callers depend on the child's
/// files really being closed — `sink`'s orphan-spool rename hits
/// `ERROR_SHARING_VIOLATION` on kiosk-main's still-open spool segment
/// otherwise, and silently loses that child's pre-death telemetry — so the
/// wait is part of the kill, not an optional extra.
#[cfg(windows)]
pub(crate) fn kill_and_wait(child: &mut Child) {
    use std::os::windows::io::AsRawHandle;
    let _ = child.kill();
    let handle = child.as_raw_handle();
    // Safety: `handle` is the live process handle owned by `child`, which
    // outlives this call; the wait is bounded and has no out-parameters.
    unsafe {
        win32::WaitForSingleObject(handle, KILL_WAIT_MS);
    }
}

/// Non-Windows stub (dev hosts only; the kiosk target is Windows x64).
#[cfg(unix)]
pub(crate) fn kill_and_wait(child: &mut ChildHandle) {
    use std::os::fd::AsRawFd;
    use std::os::raw::{c_int, c_long, c_void};
    use std::ptr;
    use std::time::{Duration, Instant};

    const SIGKILL: c_int = 9;
    const SYS_PIDFD_SEND_SIGNAL: c_long = 424;

    extern "C" {
        fn syscall(number: c_long, ...) -> c_long;
        fn kill(pid: c_int, signal: c_int) -> c_int;
    }

    if !child.exited.load(std::sync::atomic::Ordering::Acquire) {
        let used_pidfd = if let Some(pidfd) = child.pidfd.as_ref() {
            // Safety: syscall arguments match pidfd_send_signal(2); the
            // pidfd is an owned live descriptor and the siginfo pointer is
            // explicitly NULL for a plain SIGKILL.
            let _ = unsafe {
                syscall(
                    SYS_PIDFD_SEND_SIGNAL,
                    pidfd.as_raw_fd(),
                    SIGKILL,
                    ptr::null::<c_void>(),
                    0u32,
                )
            };
            true
        } else {
            false
        };
        if !used_pidfd {
            // Degraded path when pidfd_open was unavailable or the syscall is
            // rejected by the sandbox. The exited gate avoids killing a
            // recycled PID after the sole waiter has reaped the child.
            if !child.exited.load(std::sync::atomic::Ordering::Acquire) {
                // Safety: `child.pid` is the PID returned by Command::spawn;
                // this is the documented fallback and is guarded by the
                // post-spawn exited flag check above.
                unsafe {
                    let _ = kill(child.pid as c_int, SIGKILL);
                }
            }
        }
    }

    let deadline = Instant::now() + Duration::from_millis(5_000);
    while !child.exited.load(std::sync::atomic::Ordering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Kills `child` so no orphaned process is left holding the IPC pipe (its
/// `nMaxInstances` is 1, so a live orphan keeps the real child from ever
/// connecting). Does not send anything on `tx`: the caller of `spawn_main` is
/// solely responsible for reporting the `Err` result as a synthetic
/// `ChildExited{-1}`, and this helper must not race it with a second one.
#[cfg(windows)]
fn kill_orphan(mut child: Child) {
    kill_and_wait(&mut child);
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
        // P1-F1 Task 2: kiosk-main consumes `--safe` — it renders the bundled
        // `safe.html` (device id + last crash breadcrumb), skips all remote
        // I/O and the FSM driver, and still heartbeats. So a `watchdog.
        // safe_mode` entry means safe mode was actually ENTERED, and
        // `watchdog.safe_mode_failed` means even the safe child kept dying.
        // Known gap (P1-F2): the safe path still panics on an unreadable or
        // invalid `kiosk.ini`, so config faults never reach `safe.html`.
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
#[cfg(unix)]
fn pidfd_open(pid: u32) -> io::Result<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;
    use std::os::raw::{c_long, c_uint};

    const SYS_PIDFD_OPEN: c_long = 434;
    extern "C" {
        fn syscall(number: c_long, ...) -> c_long;
    }
    // Safety: syscall(434) is pidfd_open(pid, flags), with flags zero.
    let fd = unsafe { syscall(SYS_PIDFD_OPEN, pid as c_uint, 0u32) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // Safety: the kernel returned a fresh owned file descriptor.
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd as i32) })
}

#[cfg(unix)]
pub fn spawn_main(
    exe: &Path,
    config_dir: &Path,
    safe: bool,
    pipe_name: &str,
    tx: Sender<Event>,
) -> io::Result<ChildHandle> {
    let mut command = std::process::Command::new(exe);
    command
        .arg("--config")
        .arg(config_dir)
        .env("KIOSK_HEARTBEAT_PIPE", pipe_name);
    if safe {
        command.arg("--safe");
    }
    let child = command.spawn()?;
    let pid = child.id();
    let pidfd = match pidfd_open(pid) {
        Ok(fd) => Some(fd),
        Err(error) => {
            eprintln!(
                "kiosk-launcher: pidfd_open({pid}) unavailable ({error}); using guarded kill fallback"
            );
            None
        }
    };
    let exited = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let exited_waiter = exited.clone();
    let waiter_tx = tx.clone();
    let _ = tx.send(Event::Spawned { at: now() });
    std::thread::Builder::new()
        .name("kiosk-launcher-child-waiter".into())
        .spawn(move || {
            let mut child = child;
            let code = child.wait().map(exit_code_of).unwrap_or(-2);
            exited_waiter.store(true, std::sync::atomic::Ordering::Release);
            let _ = waiter_tx.send(Event::ChildExited { code, at: now() });
        })?;
    Ok(ChildHandle { pidfd, exited, pid })
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

#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[test]
    fn exit_mapping_preserves_signal_identity_and_never_uses_minus_one() {
        assert_eq!(exit_code_of(std::process::ExitStatus::from_raw(0)), 0);
        assert_eq!(exit_code_of(std::process::ExitStatus::from_raw(9)), 137);
        for signo in 1..=64 {
            let code = exit_code_of(std::process::ExitStatus::from_raw(signo));
            assert_ne!(code, 86);
            assert_ne!(code, -1);
        }
    }

    fn supervised_sleep(use_pidfd: bool) -> (ChildHandle, mpsc::Receiver<Event>) {
        let mut child = Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("sleep must spawn");
        let pid = child.id();
        let pidfd = if use_pidfd {
            Some(pidfd_open(pid).expect("pidfd_open must be available for the pidfd arm"))
        } else {
            None
        };
        let exited = Arc::new(AtomicBool::new(false));
        let exited_waiter = exited.clone();
        let (tx, rx) = mpsc::channel();
        let waiter_tx = tx.clone();
        std::thread::spawn(move || {
            let code = child.wait().map(exit_code_of).unwrap_or(-2);
            exited_waiter.store(true, Ordering::Release);
            let _ = waiter_tx.send(Event::ChildExited { code, at: now() });
        });
        let _ = tx.send(Event::Spawned { at: now() });
        (ChildHandle { pidfd, exited, pid }, rx)
    }

    fn run_kill_gate(use_pidfd: bool) {
        for _ in 0..200 {
            let (mut child, rx) = supervised_sleep(use_pidfd);
            kill_and_wait(&mut child);
            let mut exits = 0;
            let mut spawned = 0;
            loop {
                match rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(Event::Spawned { .. }) => spawned += 1,
                    Ok(Event::ChildExited { .. }) => exits += 1,
                    Ok(_) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        panic!("child waiter did not report within the bounded gate")
                    }
                }
            }
            assert_eq!(spawned, 1);
            assert_eq!(exits, 1, "exactly one exit event per killed child");
        }
    }

    #[test]
    fn exactly_one_exit_event_with_pidfd() {
        run_kill_gate(true);
    }

    #[test]
    fn exactly_one_exit_event_with_guarded_pid_fallback() {
        run_kill_gate(false);
    }
}
