//! RT-13's scriptable stand-in for `kiosk-main`. No webview, no config, no
//! telemetry: it speaks the real `kiosk_core::ipc` protocol over the real
//! named pipe and follows a one-line script.
//!
//! # Contracts it must honour (they are what RT-13 is proving)
//! * The pipe name comes from the `KIOSK_HEARTBEAT_PIPE` environment variable
//!   that `spawn::spawn_main` sets on the child — never recomputed, never
//!   hardcoded (the real client does the same, `kiosk-main/src/heartbeat.rs`).
//! * The open is retried: between reconnects the launcher's pipe name
//!   transiently does not exist (`ERROR_FILE_NOT_FOUND`) or is momentarily
//!   taken (`ERROR_PIPE_BUSY`).
//! * It connects from the spawned process itself, because the launcher
//!   authenticates the heartbeat client by PID.
//!
//! # Script
//! Read from `script.txt` in the `--config <dir>` the launcher passes. The
//! script is NOT an environment variable: `spawn_main` builds the child's env
//! itself, so a test could only set one process-globally, which races the other
//! RT-13 scenarios running concurrently in the same test binary. The config dir
//! is per-scenario and already plumbed through.
//!
//! * `healthy`   — `Ready`, then `Ping` every `PING_INTERVAL_S`, forever.
//! * `hang`      — `Ready`, then silence forever (connection held open).
//! * `exit:<n>`  — `Ready`, then exit with code `<n>`.

fn main() {
    #[cfg(windows)]
    windows_main();
    #[cfg(unix)]
    unix_main();
}

#[cfg(windows)]
fn windows_main() {
    use kiosk_core::ipc::{encode, Frame, PING_INTERVAL_S};
    use std::io::Write;
    use std::time::{Duration, Instant};

    let mut args = std::env::args().skip(1);
    let mut config_dir = None;
    while let Some(a) = args.next() {
        if a == "--config" {
            config_dir = args.next();
        }
    }
    let config_dir = config_dir.expect("rt13-mock-main: the launcher always passes --config <dir>");
    let script = std::fs::read_to_string(std::path::Path::new(&config_dir).join("script.txt"))
        .expect("rt13-mock-main: script.txt in the config dir");
    let script = script.trim().to_string();

    let pipe_name = std::env::var("KIOSK_HEARTBEAT_PIPE")
        .expect("rt13-mock-main: KIOSK_HEARTBEAT_PIPE is set by spawn_main");

    // Connect-and-send, retried exactly like the real client
    // (`kiosk-main/src/heartbeat.rs`): any open or write failure is a backoff
    // and another attempt, never a fatal error. The open is retried because the
    // launcher may not have called `CreateNamedPipeW` yet, because of the
    // reconnect gap (ERROR_FILE_NOT_FOUND) and because the sole instance may be
    // momentarily taken (ERROR_PIPE_BUSY). The *write* is retried because the
    // server disconnects a client whose PID does not match the child it is
    // currently supervising, and right after a restart the server can still be
    // holding the previous child's PID — the write then fails with
    // ERROR_NO_DATA (233) and the next attempt succeeds.
    // Bounds one *failing streak*, not the process: re-armed after every
    // successful `Ready`, so a healthy mock can run as long as the test needs.
    let mut deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if Instant::now() >= deadline {
            panic!("rt13-mock-main: could not deliver Ready on {pipe_name} within 30s");
        }
        let Ok(mut pipe) = std::fs::OpenOptions::new().write(true).open(&pipe_name) else {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        };
        if pipe.write_all(encode(&Frame::Ready).as_bytes()).is_err() {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        deadline = Instant::now() + Duration::from_secs(30);
        // Give the server a scheduling turn to consume Ready before an
        // exit:<n> script terminates. Without this, a fast crash can race the
        // server's frame read and make the integration test depend on OS
        // scheduling rather than the protocol.
        std::thread::sleep(Duration::from_millis(100));

        match script.as_str() {
            "healthy" => loop {
                std::thread::sleep(Duration::from_secs(PING_INTERVAL_S));
                if pipe.write_all(encode(&Frame::Ping).as_bytes()).is_err() {
                    break; // channel gone: reconnect, as the real client does
                }
            },
            // Alive, connected, and saying nothing — the shape the FSM must
            // catch as a hang. The launcher kills this process; it never exits
            // on its own.
            "hang" => loop {
                std::thread::sleep(Duration::from_secs(1));
            },
            other => {
                let code: i32 = other
                    .strip_prefix("exit:")
                    .and_then(|n| n.parse().ok())
                    .unwrap_or_else(|| panic!("rt13-mock-main: unknown script {other:?}"));
                std::process::exit(code);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
fn unix_main() {
    use kiosk_core::ipc::{encode, Frame, PING_INTERVAL_S};
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    let mut args = std::env::args().skip(1);
    let mut config_dir = None;
    while let Some(a) = args.next() {
        if a == "--config" {
            config_dir = args.next();
        }
    }
    let config_dir = config_dir.expect("rt13-mock-main: the launcher always passes --config <dir>");
    let script = std::fs::read_to_string(std::path::Path::new(&config_dir).join("script.txt"))
        .expect("rt13-mock-main: script.txt in the config dir");
    let script = script.trim().to_string();
    let socket_name = std::env::var("KIOSK_HEARTBEAT_PIPE")
        .expect("rt13-mock-main: KIOSK_HEARTBEAT_PIPE is set by spawn_main");

    let mut deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if Instant::now() >= deadline {
            panic!("rt13-mock-main: could not deliver Ready on {socket_name} within 30s");
        }
        let Ok(mut socket) = UnixStream::connect(&socket_name) else {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        };
        if socket.write_all(encode(&Frame::Ready).as_bytes()).is_err() {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        deadline = Instant::now() + Duration::from_secs(30);
        std::thread::sleep(Duration::from_millis(100));

        match script.as_str() {
            "healthy" => loop {
                std::thread::sleep(Duration::from_secs(PING_INTERVAL_S));
                if socket.write_all(encode(&Frame::Ping).as_bytes()).is_err() {
                    break;
                }
            },
            "hang" => loop {
                std::thread::sleep(Duration::from_secs(1));
            },
            other => {
                let code: i32 = other
                    .strip_prefix("exit:")
                    .and_then(|n| n.parse().ok())
                    .unwrap_or_else(|| panic!("rt13-mock-main: unknown script {other:?}"));
                std::process::exit(code);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
