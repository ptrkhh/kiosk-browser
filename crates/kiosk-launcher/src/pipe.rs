//! Named-pipe heartbeat server: accepts the connection from the spawned
//! `kiosk-main` child and turns each `'\n'`-delimited `ipc::Frame` line into
//! a `watchdog::Event`. Windows/P1 only — see the `not(windows)` stub at the
//! bottom. `serve` itself does not spawn its own thread (unlike
//! `spawn_timer`/`spawn_main`'s waiter): it blocks the calling thread for its
//! whole lifetime, so the caller (Task 4) runs it via
//! `thread::spawn(move || pipe::serve(..))`.
//!
//! # Pipe name propagation (cross-task contract)
//! `PIPE_NAME` is only the base. The concrete per-boot name
//! (`instance_name()`, `PIPE_NAME` + the launcher's PID) is what's actually
//! served. Task 4 must set it as the `KIOSK_HEARTBEAT_PIPE` environment
//! variable on the spawned child (`Command::env`) when it calls
//! `spawn::spawn_main`; Task 5's client in kiosk-main reads that env var
//! rather than recomputing the PID-based name itself.
//!
//! # Dead code scope
//! `#[allow(dead_code)]` here is temporary: `main.rs` does not yet call
//! `serve`. Remove this allow when Task 4 (`LauncherSink` + assembly) wires
//! it in.
#![allow(dead_code)]

use crate::clock::now;
use kiosk_core::ipc::{decode, Frame};
use kiosk_core::watchdog::Event;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

/// Base pipe name; `instance_name()` appends a per-boot suffix so a stale
/// instance from a previous boot can never collide with the current one.
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

#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;
    use std::os::windows::io::RawHandle;

    pub const PIPE_ACCESS_INBOUND: u32 = 0x0000_0001;
    pub const PIPE_TYPE_BYTE: u32 = 0x0000_0000;
    pub const PIPE_READMODE_BYTE: u32 = 0x0000_0000;
    pub const PIPE_WAIT: u32 = 0x0000_0000;
    pub const PIPE_UNLIMITED_INSTANCES: u32 = 255;
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
                win32::PIPE_ACCESS_INBOUND,
                win32::PIPE_TYPE_BYTE | win32::PIPE_READMODE_BYTE | win32::PIPE_WAIT,
                win32::PIPE_UNLIMITED_INSTANCES,
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
#[cfg(windows)]
pub fn serve(pipe_name: &str, tx: Sender<Event>, cancel: Arc<AtomicBool>) {
    use imp::{close_pipe, connect_pipe, create_pipe, disconnect_pipe, LineReader};

    // Set once a ChannelFault has been reported and we're waiting to accept
    // the reconnect; cleared the moment the first valid post-reconnect frame
    // arrives, at which point ChannelReconnected is sent before that frame's
    // own Event (the FSM's channel-grace state depends on this order).
    let mut awaiting_reconnect_event = false;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let handle = match create_pipe(pipe_name) {
            Ok(h) => h,
            Err(_) => return, // can't even create the pipe; nothing to serve
        };
        if connect_pipe(handle).is_err() {
            close_pipe(handle);
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            continue;
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
                    if tx.send(Event::ChannelFault { at: now() }).is_err() {
                        close_pipe(handle);
                        return;
                    }
                    disconnect_pipe(handle);
                    awaiting_reconnect_event = true;
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

/// Non-Windows stub: named pipes are a Windows-only IPC mechanism here (the
/// kiosk target is Windows x64; other host platforms are dev-only).
#[cfg(not(windows))]
pub fn serve(_pipe_name: &str, _tx: Sender<Event>, _cancel: Arc<AtomicBool>) {
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
        let server = std::thread::spawn(move || serve(&serve_name, tx, serve_cancel));

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
        // Best-effort join with a bound: per the shutdown caveat, the server
        // thread may be parked in a blocking Windows call with nothing to
        // unblock it, so this must not hang the test suite indefinitely.
        let _ = server; // detach; joining is not required for the test to pass
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
        let _server = std::thread::spawn(move || serve(&serve_name, tx, cancel));

        let mut client = open_client(&pipe_name);
        client.write_all(b"not json at all\n").unwrap();
        client
            .write_all(kiosk_core::ipc::encode(&Frame::Ready).as_bytes())
            .unwrap();
        // The garbage line must be dropped silently; the next real event
        // must still arrive.
        assert_eq!(recv_event(&rx), Event::Ready);
    }
}
