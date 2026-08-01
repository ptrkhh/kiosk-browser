//! Heartbeat client (P1-E2 Task 5): the kiosk-main end of the launcher's
//! named-pipe watchdog channel. Sends `Frame::Ready` once readiness is reached
//! (webview up + first navigation committed, arch-03) and `Frame::Ping` every
//! `PING_INTERVAL_S` thereafter, so `kiosk-launcher`'s supervise FSM stays armed.
//!
//! This module only *sends* frames. Every restart/timeout decision belongs to
//! the launcher's FSM (`kiosk_core::watchdog`) and is deliberately not mirrored
//! here.
//!
//! # Standalone runs
//! The pipe name is never recomputed here: it embeds the launcher's PID, and
//! `spawn::spawn_main` publishes it as `KIOSK_HEARTBEAT_PIPE` on the child.
//! No variable → nobody is supervising us → no heartbeat at all
//! (`pipe_name_from_env` returns `None`). kiosk-main must run standalone.
//!
//! # Degradation
//! A pipe that never appears, disappears forever, or rejects us can only ever
//! cost heartbeats, never the browser: every failure path here is a log + a
//! backoff + a retry, and the whole task folds on the app's `CancellationToken`.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// Environment variable the launcher sets on the child it spawns; see
/// `kiosk-launcher`'s `pipe.rs` module docs for the cross-task contract.
pub const PIPE_ENV: &str = "KIOSK_HEARTBEAT_PIPE";

/// Pause before retrying a failed open or resuming after a broken pipe. Must
/// stay well under the FSM's 15 s miss window so the launcher sees a
/// `ChannelReconnected`, not a heartbeat-miss restart.
const RECONNECT_BACKOFF: Duration = Duration::from_secs(1);

/// The launcher-provided pipe name, or `None` when running unsupervised.
pub fn pipe_name_from_env() -> Option<String> {
    std::env::var(PIPE_ENV).ok().filter(|s| !s.is_empty())
}

#[cfg(windows)]
pub async fn run(pipe_name: String, ready: Arc<Notify>, cancel: CancellationToken) {
    use tokio::io::AsyncWriteExt;
    use tokio::net::windows::named_pipe::ClientOptions;

    use kiosk_core::ipc::{encode, Frame, PING_INTERVAL_S};

    // Latched once the first navigation has committed. After that, EVERY
    // connection (including a reconnect) opens with `Ready` — see the module
    // note in the task report: `watchdog::on(Event::Ready)` is a no-op outside
    // `Phase::Spawning`, so a repeat is free, while a fault that happens
    // between readiness and the FSM arming would otherwise never be recovered.
    let mut ready_reached = false;
    // Log the "cannot reach the launcher" line once per failure streak, not
    // once per second (same latch pattern as the server's `logged_failure`).
    let mut logged_failure = false;

    loop {
        if cancel.is_cancelled() {
            return;
        }
        // Any open error is retried: the two expected ones are
        // ERROR_PIPE_BUSY (231, the instance is momentarily taken) and
        // ERROR_FILE_NOT_FOUND (2, the reconnect gap where the server has
        // closed the old handle and not yet created the next instance).
        // Anything else (ACCESS_DENIED from a squatter, say) is equally
        // transient from our side and equally not worth crashing the kiosk.
        let mut client = match ClientOptions::new()
            .read(false)
            .write(true)
            .open(&pipe_name)
        {
            Ok(c) => c,
            Err(e) => {
                if !logged_failure {
                    logged_failure = true;
                    eprintln!("kiosk-main: heartbeat pipe {pipe_name} unavailable ({e}); retrying");
                }
                if !sleep_or_cancel(&cancel, RECONNECT_BACKOFF).await {
                    return;
                }
                continue;
            }
        };
        logged_failure = false;

        if !ready_reached {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = ready.notified() => ready_reached = true,
            }
        }

        // Cancel-aware: a server that stops draining leaves `write_all` parked
        // on a full pipe buffer forever, which would otherwise be the one await
        // in this module unresponsive to shutdown (the launcher waits for this
        // process to exit).
        let ready_frame = encode(&Frame::Ready);
        let write_result = tokio::select! {
            _ = cancel.cancelled() => return,
            r = client.write_all(ready_frame.as_bytes()) => r,
        };
        if write_result.is_err() {
            if !sleep_or_cancel(&cancel, RECONNECT_BACKOFF).await {
                return;
            }
            continue;
        }

        let mut tick = tokio::time::interval(Duration::from_secs(PING_INTERVAL_S));
        // Steady cadence, not catch-up: after a suspend/resume or runtime
        // stall, fire one ping and resync rather than bursting every missed
        // one (same reasoning as `health::run`).
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tick.tick().await; // the immediate first tick; Ready was just sent
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tick.tick() => {
                    // Same cancel-aware wrap as the `Ready` write above: a stalled
                    // pipe must not park this task past shutdown.
                    let ping_frame = encode(&Frame::Ping);
                    let write_result = tokio::select! {
                        _ = cancel.cancelled() => return,
                        r = client.write_all(ping_frame.as_bytes()) => r,
                    };
                    if write_result.is_err() {
                        break; // pipe gone: reconnect
                    }
                }
            }
        }
        if !sleep_or_cancel(&cancel, RECONNECT_BACKOFF).await {
            return;
        }
    }
}

/// Sleeps `d`, returning `false` if the token was cancelled first (the caller
/// must then return promptly — the launcher waits for this process to exit).
#[cfg(windows)]
async fn sleep_or_cancel(cancel: &CancellationToken, d: Duration) -> bool {
    tokio::select! {
        _ = cancel.cancelled() => false,
        _ = tokio::time::sleep(d) => true,
    }
}

/// Non-Windows stub: the watchdog channel is a Windows named pipe (the kiosk
/// target is Windows x64; other hosts are dev-only). Mirrors the stubs in
/// `kiosk-launcher`'s `pipe.rs` / `spawn.rs`.
#[cfg(not(windows))]
pub async fn run(_pipe_name: String, _ready: Arc<Notify>, _cancel: CancellationToken) {
    eprintln!("kiosk-main heartbeat is Windows-only; not sending heartbeats on this platform");
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use kiosk_core::ipc::{decode, Frame};
    use tokio::io::AsyncReadExt;
    use tokio::net::windows::named_pipe::ServerOptions;

    /// Creates the server end, retrying while the *previous* instance of the
    /// same name is still alive (a client that has not yet noticed the drop
    /// keeps it around, and `first_pipe_instance` then fails with
    /// ERROR_ACCESS_DENIED). Bounded.
    async fn serve(name: &str) -> tokio::net::windows::named_pipe::NamedPipeServer {
        for _ in 0..200 {
            match ServerOptions::new().first_pipe_instance(true).create(name) {
                Ok(s) => return s,
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
        panic!("could not create test pipe server at {name}");
    }

    fn unique_pipe(tag: &str) -> String {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        format!(
            r"\\.\pipe\kiosk-main-hb-test-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// Reads until `want` complete lines have arrived, or fails the bounded wait.
    async fn read_lines(
        server: &mut tokio::net::windows::named_pipe::NamedPipeServer,
        want: usize,
    ) -> Vec<String> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 256];
        loop {
            let lines = buf.iter().filter(|&&b| b == b'\n').count();
            if lines >= want {
                return String::from_utf8_lossy(&buf)
                    .lines()
                    .map(|s| s.to_string())
                    .collect();
            }
            // Generous vs. the real 5 s ping cadence: the first `Ping` lands a
            // full interval after `Ready`, so a 5 s bound would race itself.
            let n = tokio::time::timeout(Duration::from_secs(10), server.read(&mut chunk))
                .await
                .expect("heartbeat frames should arrive within 10s")
                .expect("pipe read failed");
            assert!(n > 0, "client closed the pipe before sending {want} lines");
            buf.extend_from_slice(&chunk[..n]);
        }
    }

    /// The core contract: nothing before readiness, then `Ready`, then `Ping`.
    #[tokio::test]
    async fn sends_ready_after_the_signal_then_pings() {
        let name = unique_pipe("ready");
        let mut server = serve(&name).await;

        let ready = Arc::new(Notify::new());
        let cancel = CancellationToken::new();
        let task = tokio::spawn(run(name.clone(), ready.clone(), cancel.clone()));

        tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("client should connect within 5s")
            .expect("pipe connect failed");

        // Nothing may be sent before the first navigation commits. `AsyncReadExt::read`
        // is cancel-safe, so a timed-out read is a sound way to assert "nothing yet".
        let mut probe = [0u8; 64];
        assert!(
            tokio::time::timeout(Duration::from_millis(300), server.read(&mut probe))
                .await
                .is_err(),
            "no frame may be sent before the readiness signal"
        );
        ready.notify_one();
        let lines = read_lines(&mut server, 2).await;
        assert_eq!(decode(&lines[0]).unwrap(), Frame::Ready);
        assert_eq!(decode(&lines[1]).unwrap(), Frame::Ping);

        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("heartbeat::run must exit promptly on cancel")
            .expect("task panicked");
    }

    /// The reconnect gap (launcher contract): the pipe name transiently does
    /// not exist at all (ERROR_FILE_NOT_FOUND). The client must keep retrying
    /// the open, and must lead the new connection with `Ready` because
    /// readiness was already reached.
    #[tokio::test]
    async fn retries_when_the_pipe_does_not_exist_yet_and_reconnects() {
        let name = unique_pipe("gap");
        let ready = Arc::new(Notify::new());
        ready.notify_one(); // readiness already reached before we ever connect
        let cancel = CancellationToken::new();
        let task = tokio::spawn(run(name.clone(), ready, cancel.clone()));

        // No server exists yet — the client is looping on ERROR_FILE_NOT_FOUND.
        tokio::time::sleep(Duration::from_millis(150)).await;
        let mut server = serve(&name).await;
        tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("client should connect once the pipe appears")
            .expect("pipe connect failed");
        let lines = read_lines(&mut server, 1).await;
        assert_eq!(decode(&lines[0]).unwrap(), Frame::Ready);

        // Server dies (the launcher's reconnect gap): drop it, then serve again.
        drop(server);
        let mut server2 = serve(&name).await;
        tokio::time::timeout(Duration::from_secs(15), server2.connect())
            .await
            .expect("client should reconnect well inside the 15s miss window")
            .expect("pipe connect failed");
        let lines = read_lines(&mut server2, 1).await;
        assert_eq!(
            decode(&lines[0]).unwrap(),
            Frame::Ready,
            "a post-readiness reconnect leads with Ready (a no-op for an Armed FSM)"
        );

        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("heartbeat::run must exit promptly on cancel")
            .expect("task panicked");
    }

    /// A pipe that never appears must not hang or crash the kiosk, and cancel
    /// must still be honoured promptly from inside the retry loop.
    #[tokio::test]
    async fn exits_promptly_on_cancel_with_no_server_at_all() {
        let cancel = CancellationToken::new();
        let task = tokio::spawn(run(
            unique_pipe("nosrv"),
            Arc::new(Notify::new()),
            cancel.clone(),
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
        // Tight enough that only a genuinely cancel-aware sleep (not a plain
        // `tokio::time::sleep` standing in for `sleep_or_cancel`) can meet it:
        // the retry loop is sitting in `RECONNECT_BACKOFF` (1s) when cancelled,
        // so a mutant that ignores `cancel` inside that sleep would blow this
        // bound while still passing a laxer one.
        tokio::time::timeout(Duration::from_millis(300), task)
            .await
            .expect("heartbeat::run must exit promptly on cancel")
            .expect("task panicked");
    }
}

#[cfg(test)]
mod env_tests {
    use super::*;

    /// Standalone (developer / direct launch): no variable, no heartbeat.
    #[test]
    fn absent_or_empty_env_means_no_heartbeat() {
        // `set_var`/`remove_var` are process-global; this is the only test
        // touching this variable, and it restores nothing because nothing else
        // reads it in-process.
        std::env::remove_var(PIPE_ENV);
        assert_eq!(pipe_name_from_env(), None);
        std::env::set_var(PIPE_ENV, "");
        assert_eq!(pipe_name_from_env(), None);
        std::env::set_var(PIPE_ENV, r"\\.\pipe\x");
        assert_eq!(pipe_name_from_env(), Some(r"\\.\pipe\x".to_string()));
        std::env::remove_var(PIPE_ENV);
    }
}
