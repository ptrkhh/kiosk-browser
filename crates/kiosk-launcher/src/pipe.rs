//! Named-pipe heartbeat server: accepts the connection from the spawned
//! `kiosk-main` child and turns each `'\n'`-delimited `ipc::Frame` line into
//! a `watchdog::Event`. Windows/P1 only — see the `not(windows)` stub at the
//! bottom. `serve` itself does not spawn its own thread (unlike
//! `spawn_timer`/`spawn_main`'s waiter): it blocks the calling thread for its
//! whole lifetime, so the caller (Task 4) runs it via
//! `thread::spawn(move || pipe::serve(..))`.
//!
//! # Pipe name propagation (cross-task contract)
//! `PIPE_NAME` is only the base. The concrete per-launcher name
//! (`instance_name()`, `PIPE_NAME` + the launcher's PID) is what's actually
//! served. `spawn::spawn_main` takes that name as a parameter and sets it as
//! the `KIOSK_HEARTBEAT_PIPE` environment variable on the child it spawns;
//! Task 5's client in kiosk-main reads that env var rather than recomputing
//! the PID-based name itself. Task 4 passes `pipe::instance_name()` to both
//! `serve` and `spawn_main`.
//!
//! # Supervised-child PID (cross-task contract)
//! `serve` takes `child_pid: Arc<AtomicU32>`, shared with Task 4's sink,
//! where **0 means "no live child"**. Task 4 must:
//! * store `child.id()` immediately after a successful `spawn_main`, and
//! * store `0` as soon as it observes `Event::ChildExited` (including the
//!   synthetic `ChildExited{-1}` it feeds on a `spawn_main` `Err`).
//!
//! `serve` uses it for two things: it only accepts pipe clients whose
//! process ID matches (so no other local process can forge heartbeats for a
//! hung kiosk), and it only reports `ChannelFault` while a child is actually
//! alive (a pipe breaking because the child died is a `ChildExited`, not a
//! channel fault — reporting both races the FSM's restart and can leave the
//! *next* child with an inherited channel-grace window).

use crate::clock::now;
use kiosk_core::ipc::{decode, Frame};
use kiosk_core::watchdog::Event;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

/// Base pipe name; `instance_name()` appends the launcher's PID so two
/// launcher processes (or a launcher and a still-exiting predecessor) can
/// never serve the same name. Note the suffix is per-*process*, not per-boot:
/// PIDs are reused across boots, but only one process can hold a given PID at
/// a time, and the pipe dies with its process — keep that property if the
/// suffix ever changes.
///
/// # Reconnect gap (cross-task contract for Task 5's client)
/// Between reconnects, `serve` closes the old pipe handle before creating the
/// next instance (and sleeps first on a rejection path), so the pipe name
/// transiently does not exist at all. A client that tries to open it during
/// that window gets `ERROR_FILE_NOT_FOUND`, not `ERROR_PIPE_BUSY` — Task 5's
/// client must retry the open on `ERROR_FILE_NOT_FOUND` as well as on
/// `ERROR_PIPE_BUSY`, not only the latter.
pub const PIPE_NAME: &str = r"\\.\pipe\kiosk-heartbeat";

/// The concrete, per-process pipe name. Uses the launcher's own PID as the
/// per-boot suffix: unique per launcher run, and trivially reproducible by
/// whichever code needs to pass it to the child (see module docs).
pub fn instance_name() -> String {
    format!("{PIPE_NAME}-{}", std::process::id())
}

/// Pure line -> Event mapping: the seam this module exists to make
/// host-testable. `decode` errors (garbage, unknown frame type, partial
/// JSON) are dropped silently — `None`, never a panic, never an `Event`.
pub fn frame_to_event(line: &str, now: u64) -> Option<Event> {
    match decode(line) {
        Ok(Frame::Ready) => Some(Event::Ready),
        Ok(Frame::Ping) => Some(Event::Heartbeat { at: now }),
        Err(_) => None,
    }
}

/// Fail-closed client authentication, as a pure seam.
///
/// `expected` is the PID snapshot taken before the accept; `current` is the
/// shared child PID re-read at accept time. Either may legitimately identify
/// the child: during a restart with `backoff_s > 2` the snapshot is taken while
/// the shared PID is still 0 (`await_child_pid` gives up after ~2s), so a
/// snapshot-only check rejects the *legitimate* new child's first connect and
/// logs it as an impostor on every normal restart.
///
/// Fail-closed in every other case: `None` (Windows won't name the client), a
/// zero client PID, and any PID matching neither value are all rejected.
pub fn accept_client(client: Option<u32>, expected: u32, current: u32) -> bool {
    match client {
        Some(p) if p != 0 => p == expected || p == current,
        _ => false,
    }
}

#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;
    use std::os::windows::io::RawHandle;

    pub const PIPE_ACCESS_INBOUND: u32 = 0x0000_0001;
    pub const PIPE_TYPE_BYTE: u32 = 0x0000_0000;
    pub const PIPE_READMODE_BYTE: u32 = 0x0000_0000;
    pub const PIPE_WAIT: u32 = 0x0000_0000;
    /// Creation fails with ERROR_ACCESS_DENIED if another process already
    /// owns this pipe name — the loud outcome we want, rather than silently
    /// becoming an extra instance of a squatter's pipe (whose DACL and pipe
    /// mode we would then inherit, and whose instance the child might reach).
    pub const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
    /// Refuse clients arriving over SMB; the only legitimate client is a
    /// child process on this machine.
    pub const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x0000_0008;
    pub const ERROR_PIPE_CONNECTED: u32 = 535;

    /// `-1` reinterpreted as a pointer-sized handle: the documented
    /// `INVALID_HANDLE_VALUE` sentinel `CreateNamedPipeW` returns on failure.
    pub const INVALID_HANDLE_VALUE: RawHandle = -1isize as RawHandle;

    // kernel32.lib is already linked into every Windows Rust binary (see
    // spawn.rs's `win32` module for the same reasoning), so these raw
    // declarations need no extra dependency and no `#[link(...)]`.
    extern "system" {
        pub fn CreateNamedPipeW(
            lp_name: *const u16,
            dw_open_mode: u32,
            dw_pipe_mode: u32,
            n_max_instances: u32,
            n_out_buffer_size: u32,
            n_in_buffer_size: u32,
            n_default_time_out: u32,
            lp_security_attributes: *mut c_void,
        ) -> RawHandle;
        pub fn ConnectNamedPipe(h_named_pipe: RawHandle, lp_overlapped: *mut c_void) -> i32;
        pub fn GetNamedPipeClientProcessId(h_pipe: RawHandle, client_pid: *mut u32) -> i32;
        pub fn DisconnectNamedPipe(h_named_pipe: RawHandle) -> i32;
        pub fn ReadFile(
            h_file: RawHandle,
            lp_buffer: *mut u8,
            n_number_of_bytes_to_read: u32,
            lp_number_of_bytes_read: *mut u32,
            lp_overlapped: *mut c_void,
        ) -> i32;
        pub fn CloseHandle(h_object: RawHandle) -> i32;
        pub fn GetLastError() -> u32;
    }
}

#[cfg(windows)]
mod imp {
    use super::win32;
    use std::ffi::OsStr;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::RawHandle;

    /// Longest line the reader accumulates before giving up on ever seeing a
    /// `'\n'` and resetting. A misbehaving/malicious writer that never
    /// terminates a line must not grow this buffer forever.
    ///
    /// ponytail: fixed cap + full-buffer reset (not a ring buffer or
    /// incremental resync); raise or make configurable if a legitimate frame
    /// ever needs to exceed 64 KiB.
    const MAX_LINE_BYTES: usize = 64 * 1024;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn create_pipe(name: &str) -> io::Result<RawHandle> {
        let wide = to_wide(name);
        // Safety: `wide` is a valid null-terminated UTF-16 buffer alive for
        // the call; all other args are plain values/null, matching the
        // documented `CreateNamedPipeW` signature.
        let handle = unsafe {
            win32::CreateNamedPipeW(
                wide.as_ptr(),
                win32::PIPE_ACCESS_INBOUND | win32::FILE_FLAG_FIRST_PIPE_INSTANCE,
                win32::PIPE_TYPE_BYTE
                    | win32::PIPE_READMODE_BYTE
                    | win32::PIPE_WAIT
                    | win32::PIPE_REJECT_REMOTE_CLIENTS,
                // Exactly one client (the supervised child) is ever expected.
                1,
                0,
                0,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == win32::INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(handle)
        }
    }

    /// Blocks until a client connects (or is already connected). Blocking
    /// mode: nothing on this thread can unblock a pending `ConnectNamedPipe`
    /// other than a client actually connecting or the pipe handle being
    /// closed from elsewhere — `cancel` is only checked between calls, never
    /// during one (see module-level shutdown notes in `serve`).
    pub fn connect_pipe(handle: RawHandle) -> io::Result<()> {
        // Safety: `handle` is a live named-pipe server handle owned by the
        // caller for the duration of this blocking call.
        let ok = unsafe { win32::ConnectNamedPipe(handle, std::ptr::null_mut()) };
        if ok != 0 {
            return Ok(());
        }
        // A client raced us and connected between CreateNamedPipeW and this
        // call — Windows reports that as ERROR_PIPE_CONNECTED, which is a
        // success case for us, not an error.
        let err = unsafe { win32::GetLastError() };
        if err == win32::ERROR_PIPE_CONNECTED {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(err as i32))
        }
    }

    /// PID of the process on the other end of a connected pipe instance.
    /// `None` if Windows won't tell us — treated by the caller as "not the
    /// supervised child", i.e. rejected.
    pub fn client_pid(handle: RawHandle) -> Option<u32> {
        let mut pid: u32 = 0;
        // Safety: `handle` is a live, connected named-pipe server handle
        // owned by the caller; `pid` is a valid u32 the call writes into.
        let ok = unsafe { win32::GetNamedPipeClientProcessId(handle, &mut pid) };
        if ok != 0 {
            Some(pid)
        } else {
            None
        }
    }

    pub fn disconnect_pipe(handle: RawHandle) {
        // Safety: `handle` is a live named-pipe server handle; failure is
        // ignored (best-effort, matches spawn.rs's kill_orphan pattern).
        unsafe {
            win32::DisconnectNamedPipe(handle);
        }
    }

    pub fn close_pipe(handle: RawHandle) {
        // Safety: `handle` is a live handle owned by the caller, closed
        // exactly once here.
        unsafe {
            win32::CloseHandle(handle);
        }
    }

    /// Buffers raw bytes off a connected pipe handle and yields complete
    /// `'\n'`-delimited lines (without the trailing `'\n'`). A partial line
    /// left in the buffer when the connection breaks is simply dropped when
    /// the caller discards this reader and reconnects — never handed to
    /// `frame_to_event` as if it were complete.
    pub struct LineReader {
        handle: RawHandle,
        buf: Vec<u8>,
    }

    impl LineReader {
        pub fn new(handle: RawHandle) -> Self {
            LineReader {
                handle,
                buf: Vec::new(),
            }
        }

        /// Returns the next complete line, reading more off the pipe as
        /// needed. `Err` means the pipe broke (client gone, reset, etc.) —
        /// the caller is expected to disconnect/reconnect, not retry.
        pub fn next_line(&mut self) -> io::Result<String> {
            loop {
                if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = self.buf.drain(..=pos).collect();
                    // Trim the trailing '\n' (and a possible '\r').
                    let line = &line[..line.len() - 1];
                    return Ok(String::from_utf8_lossy(line)
                        .trim_end_matches('\r')
                        .to_string());
                }
                if self.buf.len() > MAX_LINE_BYTES {
                    // Oversized garbage with no newline in sight: drop it and
                    // resync on whatever arrives next rather than growing
                    // forever or panicking.
                    self.buf.clear();
                }
                let mut chunk = [0u8; 4096];
                let mut read: u32 = 0;
                // Safety: `chunk` is a valid, appropriately-sized stack
                // buffer for the duration of this blocking call; `read`
                // receives the byte count `ReadFile` writes back.
                let ok = unsafe {
                    win32::ReadFile(
                        self.handle,
                        chunk.as_mut_ptr(),
                        chunk.len() as u32,
                        &mut read,
                        std::ptr::null_mut(),
                    )
                };
                if ok == 0 {
                    return Err(io::Error::last_os_error());
                }
                if read == 0 {
                    // Blocking byte-mode reads returning 0 bytes without an
                    // error is not an expected case, but treat it as a
                    // broken connection rather than spinning forever.
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "pipe read 0 bytes",
                    ));
                }
                self.buf.extend_from_slice(&chunk[..read as usize]);
            }
        }
    }
}

/// Creates the named-pipe server at `pipe_name`, accepts the connection,
/// and turns each line into an `Event` on `tx`. Runs until `tx`'s receiver
/// is dropped (the supervisor loop exited) or `cancel` is observed true
/// between blocking calls.
///
/// # Shutdown caveat
/// `cancel` is only checked between blocking Windows calls (after a
/// disconnect, before the next accept). If this thread is parked inside a
/// pending `ConnectNamedPipe` (no client has ever connected) or mid-`ReadFile`
/// when `cancel` is set, nothing unblocks it — it lingers until a client
/// connects/disconnects or the process exits. This is the same shape as
/// `spawn_main`'s child-waiter thread: a detached thread that outlives a
/// courtesy cancel flag.
///
/// # Client authentication
/// Only frames from the process whose PID is in `child_pid` are honoured;
/// any other local process that opens the (trivially derivable) pipe name is
/// disconnected without a single event, so it cannot keep the watchdog happy
/// on behalf of a hung kiosk. See the module docs for `child_pid`'s contract.
#[cfg(windows)]
pub fn serve(
    pipe_name: &str,
    data_dir: &std::path::Path,
    tx: Sender<Event>,
    cancel: Arc<AtomicBool>,
    child_pid: Arc<AtomicU32>,
) {
    use imp::{client_pid, close_pipe, connect_pipe, create_pipe, disconnect_pipe, LineReader};

    // Set once a ChannelFault has been reported and we're waiting to accept
    // the reconnect; cleared the moment the first valid post-reconnect frame
    // arrives, at which point ChannelReconnected is sent before that frame's
    // own Event (the FSM's channel-grace state depends on this order).
    let mut awaiting_reconnect_event = false;
    // PID of the child whose fault we're waiting to see reconnect. A *new*
    // child is not a reconnect of the old channel, so the latch is cleared
    // when the supervised child changes (no spurious ChannelReconnected /
    // ChannelReset log on every restart).
    let mut faulted_pid: u32 = 0;
    // Suppresses repeat logging while create/connect keeps failing the same
    // way; re-armed by any success.
    let mut logged_failure = false;
    // Same once-per-streak latch pattern as `logged_failure`, but for
    // consecutive wrong-PID rejections: an operator should see "someone is
    // repeatedly trying to forge heartbeats" once per streak, not once per
    // rejection.
    let mut logged_impostor = false;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let handle = match create_pipe(pipe_name) {
            Ok(h) => h,
            Err(e) => {
                // ponytail: `serve` has no Logger (the sink owns that stack,
                // and `Logger` is neither `Clone` nor `Sync`), so the operator
                // signal here is the same `startup-degraded.txt` breadcrumb the
                // config/telemetry startup failures leave. Without it a squatted
                // pipe name — a permanent heartbeat outage, since
                // FILE_FLAG_FIRST_PIPE_INSTANCE keeps failing forever — is the
                // only silent-forever degraded path on the device. Upgrade to a
                // real log entry if `serve` ever gets a logger.
                if !logged_failure {
                    logged_failure = true;
                    eprintln!("kiosk-launcher: cannot create heartbeat pipe {pipe_name}: {e}");
                    crate::sink::breadcrumb(data_dir, "pipe", &e.to_string());
                }
                sleep_retry();
                continue;
            }
        };
        // Wait for the supervised child's PID *before* handing out the sole
        // pipe instance via `connect_pipe` (Finding 1): if this wait ran
        // after connect, whoever connected first — attacker or child — would
        // hold the only instance for up to ~2s, and a looping attacker could
        // keep winning that race forever, starving the real child of a slot
        // to ever deliver `Ready`.
        let mut expected = await_child_pid(&child_pid, &cancel);
        if cancel.load(Ordering::Relaxed) {
            close_pipe(handle);
            return;
        }

        if let Err(e) = connect_pipe(handle) {
            if !logged_failure {
                logged_failure = true;
                eprintln!("kiosk-launcher: heartbeat pipe accept failed: {e}");
            }
            close_pipe(handle);
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            sleep_retry();
            continue;
        }
        logged_failure = false;

        // Authenticate the client against the supervised child: the snapshot
        // taken above OR the shared PID as it stands right now. During a
        // restart with `backoff_s > 2` the snapshot is 0 (the child did not
        // exist yet when `await_child_pid` gave up), so snapshot-only would
        // reject the legitimate new child and cry impostor on every restart.
        // Still fail-closed — see `accept_client`. One extra relaxed load at
        // accept time, so no starvation window reopens, and this still runs
        // before any `LineReader` exists: a rejected client emits no event.
        let client = client_pid(handle);
        if !accept_client(client, expected, child_pid.load(Ordering::Relaxed)) {
            if !logged_impostor {
                logged_impostor = true;
                eprintln!(
                    "kiosk-launcher: heartbeat pipe rejected a client with an unexpected PID"
                );
            }
            disconnect_pipe(handle);
            close_pipe(handle);
            sleep_retry();
            continue;
        }
        logged_impostor = false;
        // The accepted client's own PID is what the ChannelFault liveness
        // check and the reconnect latch must compare against from here on —
        // it may be the current value rather than the stale snapshot.
        expected = client.unwrap_or(expected);
        if expected != faulted_pid {
            // Different child than the one that faulted (or a first
            // connection): nothing to "reconnect" to.
            awaiting_reconnect_event = false;
        }

        let mut reader = LineReader::new(handle);
        loop {
            match reader.next_line() {
                Ok(line) => {
                    if let Some(ev) = frame_to_event(&line, now()) {
                        if awaiting_reconnect_event {
                            awaiting_reconnect_event = false;
                            if tx.send(Event::ChannelReconnected).is_err() {
                                close_pipe(handle);
                                return;
                            }
                        }
                        if tx.send(ev).is_err() {
                            close_pipe(handle);
                            return;
                        }
                    }
                    // decode error: dropped silently, keep reading.
                }
                Err(_) => {
                    // Only a fault *while the child is alive* is a channel
                    // fault. If the child is gone (PID reset to 0 by Task 4
                    // on ChildExited), the broken pipe is just the corpse of
                    // that child: ChildExited already tells the FSM, and a
                    // racing ChannelFault landing after restart() would hand
                    // the *next* child a stale 30s channel-grace window.
                    if child_pid.load(Ordering::Relaxed) == expected {
                        if tx.send(Event::ChannelFault { at: now() }).is_err() {
                            close_pipe(handle);
                            return;
                        }
                        awaiting_reconnect_event = true;
                        faulted_pid = expected;
                    }
                    disconnect_pipe(handle);
                    break;
                }
            }
        }
        close_pipe(handle);
        if cancel.load(Ordering::Relaxed) {
            return;
        }
    }
}

/// Pause before retrying a failed create/accept, so a persistent failure is
/// a slow retry loop rather than 100% CPU for weeks.
#[cfg(windows)]
fn sleep_retry() {
    std::thread::sleep(std::time::Duration::from_millis(100));
}

/// Reads the supervised child's PID, tolerating the startup window where
/// Task 4 has spawned the child but not yet published its PID: waits (up to
/// ~2s, bounded, cancellable) for a nonzero value before giving up and
/// returning 0, which the caller treats as "reject this client".
#[cfg(windows)]
fn await_child_pid(child_pid: &AtomicU32, cancel: &AtomicBool) -> u32 {
    for _ in 0..100 {
        let pid = child_pid.load(Ordering::Relaxed);
        if pid != 0 || cancel.load(Ordering::Relaxed) {
            return pid;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    child_pid.load(Ordering::Relaxed)
}

/// Non-Windows stub: named pipes are a Windows-only IPC mechanism here (the
/// kiosk target is Windows x64; other host platforms are dev-only).
#[cfg(not(windows))]
pub fn serve(
    _pipe_name: &str,
    _data_dir: &std::path::Path,
    _tx: Sender<Event>,
    _cancel: Arc<AtomicBool>,
    _child_pid: Arc<AtomicU32>,
) {
    eprintln!("kiosk-launcher pipe::serve is Windows-only; not serving on this platform");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_line_maps_to_ready_event() {
        assert_eq!(
            frame_to_event("{\"type\":\"ready\"}", 42),
            Some(Event::Ready)
        );
    }

    #[test]
    fn ping_line_maps_to_heartbeat_with_now() {
        assert_eq!(
            frame_to_event("{\"type\":\"ping\"}", 42),
            Some(Event::Heartbeat { at: 42 })
        );
    }

    /// Finding 4: during a restart with `backoff_s > 2` the pre-accept snapshot
    /// is 0 (no child existed when `await_child_pid` gave up), and the
    /// legitimate new child must still be accepted on the CURRENT value —
    /// otherwise every normal restart logs a security-shaped impostor rejection
    /// and costs the child a ~1s retry.
    #[test]
    fn accept_client_takes_either_the_snapshot_or_the_current_pid() {
        assert!(accept_client(Some(1234), 1234, 1234), "steady state");
        assert!(
            accept_client(Some(1234), 1234, 0),
            "the child that connected has since exited: its own frames are still its own"
        );
        assert!(
            accept_client(Some(5678), 0, 5678),
            "post-backoff restart: stale snapshot 0, the new child is current"
        );
    }

    /// ...and the fail-closed property survives it: an unknown, absent or
    /// zero-matching PID is still rejected before any frame is read.
    #[test]
    fn accept_client_is_fail_closed() {
        assert!(!accept_client(None, 1234, 1234), "Windows won't name it");
        assert!(!accept_client(Some(9999), 1234, 5678), "matches neither");
        assert!(!accept_client(Some(1234), 0, 0), "no live child at all");
        assert!(
            !accept_client(Some(0), 0, 0),
            "a zero client PID must never match the zero sentinel"
        );
    }

    #[test]
    fn garbage_maps_to_none() {
        assert_eq!(frame_to_event("not json", 42), None);
        assert_eq!(frame_to_event("", 42), None);
        assert_eq!(frame_to_event("{\"type\":\"unknown\"}", 42), None);
    }
}

#[cfg(all(test, windows))]
mod windows_smoke {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Opens the client end of `path`, retrying briefly: the server may not
    /// have called `ConnectNamedPipe` (or even `CreateNamedPipeW`) yet when
    /// the test thread races it.
    /// The server pipe is `PIPE_ACCESS_INBOUND` (server reads only), so the
    /// client end only ever has write access — requesting read access here
    /// gets `ERROR_ACCESS_DENIED`.
    fn open_client(path: &str) -> std::fs::File {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match OpenOptions::new().write(true).open(path) {
                Ok(f) => return f,
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => panic!("failed to open client end of {path}: {e}"),
            }
        }
    }

    /// Data dir for the tests that never fail `create_pipe` and so never write
    /// a breadcrumb; the squatter test below uses a real tempdir instead.
    fn test_data_dir() -> std::path::PathBuf {
        std::env::temp_dir().join("kiosk-launcher-pipe-tests")
    }

    fn recv_event(rx: &mpsc::Receiver<Event>) -> Event {
        rx.recv_timeout(Duration::from_secs(5))
            .expect("expected an Event within 5s")
    }

    /// Real Windows smoke (brief Step 3): drives connect -> Ready -> Ping ->
    /// client death -> ChannelFault -> reconnect -> ChannelReconnected,
    /// against this module's real named-pipe server, using a plain
    /// `OpenOptions` open as the client end (standing in for Task 5's
    /// not-yet-built kiosk-main client).
    #[test]
    fn connect_ready_ping_then_client_death_then_reconnect() {
        let pipe_name = format!(r"\\.\pipe\kiosk-heartbeat-test-{}", std::process::id());
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));

        let serve_name = pipe_name.clone();
        let serve_cancel = cancel.clone();
        // This test process is the pipe client, so it is the "supervised
        // child" as far as the PID check is concerned.
        let child_pid = Arc::new(AtomicU32::new(std::process::id()));
        let serve_pid = child_pid.clone();
        let server = std::thread::spawn(move || {
            serve(&serve_name, &test_data_dir(), tx, serve_cancel, serve_pid)
        });

        // First client: Ready, then Ping.
        let mut client = open_client(&pipe_name);
        client
            .write_all(kiosk_core::ipc::encode(&Frame::Ready).as_bytes())
            .unwrap();
        assert_eq!(recv_event(&rx), Event::Ready);

        client
            .write_all(kiosk_core::ipc::encode(&Frame::Ping).as_bytes())
            .unwrap();
        assert!(matches!(recv_event(&rx), Event::Heartbeat { .. }));

        // Kill the client mid-run: server must see a broken pipe.
        drop(client);
        assert!(matches!(recv_event(&rx), Event::ChannelFault { .. }));

        // Reconnect: the first frame after reconnect must be preceded by
        // ChannelReconnected, before its own event.
        let mut client2 = open_client(&pipe_name);
        client2
            .write_all(kiosk_core::ipc::encode(&Frame::Ready).as_bytes())
            .unwrap();
        assert_eq!(recv_event(&rx), Event::ChannelReconnected);
        assert_eq!(recv_event(&rx), Event::Ready);

        cancel.store(true, Ordering::Relaxed);
        drop(client2);
        // Deliberately not joined: per the shutdown caveat, the server thread
        // may be parked in a blocking Windows call that `cancel` cannot
        // interrupt, so joining could hang the suite. It exits with the
        // process.
        drop(server);
    }

    /// Finding 1: a read error while no child is alive (shared PID back to 0,
    /// as Task 4 sets it on `ChildExited`) must NOT produce `ChannelFault` —
    /// the dead child's broken pipe is already reported as `ChildExited`.
    #[test]
    fn read_error_with_no_live_child_produces_no_channel_fault() {
        let pipe_name = format!(
            r"\\.\pipe\kiosk-heartbeat-test-nofault-{}",
            std::process::id()
        );
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let child_pid = Arc::new(AtomicU32::new(std::process::id()));

        let serve_name = pipe_name.clone();
        let serve_cancel = cancel.clone();
        let serve_pid = child_pid.clone();
        let _server = std::thread::spawn(move || {
            serve(&serve_name, &test_data_dir(), tx, serve_cancel, serve_pid)
        });

        let mut client = open_client(&pipe_name);
        client
            .write_all(kiosk_core::ipc::encode(&Frame::Ready).as_bytes())
            .unwrap();
        assert_eq!(recv_event(&rx), Event::Ready);

        // The "child" exits: Task 4 zeroes the PID, then the pipe breaks.
        child_pid.store(0, Ordering::Relaxed);
        drop(client);

        assert!(
            matches!(
                rx.recv_timeout(Duration::from_millis(500)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "no event at all should follow a read error with no live child"
        );
        cancel.store(true, Ordering::Relaxed);
    }

    /// Finding 3a: a client that is not the supervised child is disconnected
    /// and its frames produce no events — otherwise any local process could
    /// keep the watchdog happy on behalf of a hung kiosk.
    #[test]
    fn client_with_wrong_pid_is_rejected() {
        let pipe_name = format!(
            r"\\.\pipe\kiosk-heartbeat-test-badpid-{}",
            std::process::id()
        );
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        // Some PID that is definitely not this test process.
        let child_pid = Arc::new(AtomicU32::new(u32::MAX));

        let serve_name = pipe_name.clone();
        let serve_cancel = cancel.clone();
        let serve_pid = child_pid.clone();
        let _server = std::thread::spawn(move || {
            serve(&serve_name, &test_data_dir(), tx, serve_cancel, serve_pid)
        });

        let mut client = open_client(&pipe_name);
        // Writes may or may not succeed depending on when the server
        // disconnects; either way no event may be produced.
        let _ = client.write_all(kiosk_core::ipc::encode(&Frame::Ready).as_bytes());
        let _ = client.write_all(kiosk_core::ipc::encode(&Frame::Ping).as_bytes());

        assert!(
            matches!(
                rx.recv_timeout(Duration::from_millis(500)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "an impostor client must produce no events"
        );
        cancel.store(true, Ordering::Relaxed);
    }

    /// Finding 5: a squatted pipe name is a permanent heartbeat outage
    /// (`FILE_FLAG_FIRST_PIPE_INSTANCE` keeps failing forever) and there is no
    /// console on a deployed device — so it must leave the same
    /// `startup-degraded.txt` breadcrumb the config/telemetry failures do.
    #[test]
    fn a_squatted_pipe_name_leaves_a_breadcrumb() {
        let pipe_name = format!(
            r"\\.\pipe\kiosk-heartbeat-test-squat-{}",
            std::process::id()
        );
        // Squat the name first: `serve`'s own create then fails forever.
        let squatter = imp::create_pipe(&pipe_name).expect("squat the name");

        let data = tempfile::tempdir().unwrap();
        let dir = data.path().to_path_buf();
        let (tx, _rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let serve_cancel = cancel.clone();
        let serve_name = pipe_name.clone();
        let _server = std::thread::spawn(move || {
            serve(
                &serve_name,
                &dir,
                tx,
                serve_cancel,
                Arc::new(AtomicU32::new(0)),
            )
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let file = data.path().join(crate::sink::DEGRADED_FILE);
        while !file.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let text = std::fs::read_to_string(&file).expect("breadcrumb written");
        assert!(
            text.contains("pipe:"),
            "reason token must be `pipe`, got {text:?}"
        );

        cancel.store(true, Ordering::Relaxed);
        imp::close_pipe(squatter);
    }

    #[test]
    fn garbage_line_does_not_panic_or_produce_an_event() {
        let pipe_name = format!(
            r"\\.\pipe\kiosk-heartbeat-test-garbage-{}",
            std::process::id()
        );
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));

        let serve_name = pipe_name.clone();
        let serve_cancel = cancel.clone();
        let child_pid = Arc::new(AtomicU32::new(std::process::id()));
        let _server = std::thread::spawn(move || {
            serve(&serve_name, &test_data_dir(), tx, serve_cancel, child_pid)
        });

        let mut client = open_client(&pipe_name);
        client.write_all(b"not json at all\n").unwrap();
        client
            .write_all(kiosk_core::ipc::encode(&Frame::Ready).as_bytes())
            .unwrap();
        // The garbage line must be dropped silently; the next real event
        // must still arrive.
        assert_eq!(recv_event(&rx), Event::Ready);
        cancel.store(true, Ordering::Relaxed);
    }
}
