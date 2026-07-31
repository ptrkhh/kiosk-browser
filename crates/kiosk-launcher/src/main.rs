//! kiosk-launcher — the supervisor (spec §3.1). It spawns and watches
//! `kiosk-main` over a named-pipe heartbeat and executes whatever the pure E1
//! `watchdog` FSM decides; it reimplements no supervise logic of its own.
//!
//! Assembly only: three source threads (pipe reader, 1 s timer, child waiter)
//! feed ONE `mpsc::Sender<watchdog::Event>`, and a single loop owns the
//! `Watchdog` and dispatches each returned `Action` to the [`sink::LauncherSink`].

mod clock;
mod loop_;
mod pipe;
mod sink;
mod spawn;
mod timer;

use kiosk_core::config::bootstrap::BootstrapConfig;
use kiosk_core::watchdog::{Watchdog, WatchdogConfig};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{mpsc, Arc};

/// The install dir `kiosk.ini` and the credential live in (spec §4): next to the
/// running exe, unless `--config <dir>` overrides it. Same convention and same
/// flag as kiosk-main's `cli`/`resolve_config_dir`, so both processes read the
/// same files — and `spawn::spawn_main` passes this directory to the child as
/// its own `--config`.
fn resolve_config_dir(args: impl Iterator<Item = String>) -> PathBuf {
    let mut args = args.skip(1);
    let mut override_dir = None;
    while let Some(a) = args.next() {
        if a == "--config" {
            override_dir = args.next();
        }
    }
    match override_dir {
        Some(dir) => PathBuf::from(dir),
        None => std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from(".")),
    }
}

/// The data dir (spool, cache, last-good) — `%ProgramData%\kiosk\` (spec §4),
/// never operator-overridden. The same rule as kiosk-main's `resolve_data_dir`:
/// the launcher's `spool/launcher` partition and the `spool/main` partition it
/// drains have to land in the same place.
fn resolve_data_dir() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("kiosk")
}

/// The supervised binary: `kiosk-main` next to this exe.
fn resolve_main_exe(config_dir: &Path) -> PathBuf {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| config_dir.to_path_buf());
    dir.join(format!("kiosk-main{}", std::env::consts::EXE_SUFFIX))
}

/// Read and parse `kiosk.ini`, or `None` with a stderr note.
///
/// A missing/invalid ini is NOT fatal here, deliberately: the launcher's job is
/// to keep a screen lit, and a supervisor that refuses to start guarantees a
/// black screen, while one that starts with the spec's default watchdog timings
/// still supervises. The cost is that this run has no telemetry — the same trade
/// kiosk-main makes when its own credential is unreadable.
fn load_bootstrap(config_dir: &Path) -> Option<BootstrapConfig> {
    let path = config_dir.join("kiosk.ini");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "kiosk-launcher: cannot read {} ({e}); supervising with default timings and no telemetry",
                path.display()
            );
            return None;
        }
    };
    match BootstrapConfig::parse(&text) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!(
                "kiosk-launcher: {} is not a valid kiosk.ini ({e}); supervising with default timings and no telemetry",
                path.display()
            );
            None
        }
    }
}

fn watchdog_config(bootstrap: Option<&BootstrapConfig>) -> WatchdogConfig {
    match bootstrap {
        Some(b) => WatchdogConfig {
            startup_grace_s: b.startup_grace_s,
            healthy_run_s: b.healthy_run_s,
            channel_grace_s: b.channel_grace_s,
        },
        // `BootstrapConfig::parse`'s own defaults (spec §5.1).
        None => WatchdogConfig {
            startup_grace_s: 90,
            healthy_run_s: 120,
            channel_grace_s: 30,
        },
    }
}

fn main() {
    let config_dir = resolve_config_dir(std::env::args());
    let data_dir = resolve_data_dir();
    let bootstrap = load_bootstrap(&config_dir);
    let exe = resolve_main_exe(&config_dir);

    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    // 0 == "no live child". The sink is this atomic's only writer; `pipe::serve`
    // reads it to authenticate the heartbeat client and to decide whether a
    // broken pipe is a channel fault or just a dead child's corpse.
    let child_pid = Arc::new(AtomicU32::new(0));
    let pipe_name = pipe::instance_name();

    timer::spawn_timer(tx.clone(), cancel.clone());
    // `serve` blocks its caller for its whole lifetime (unlike `spawn_timer`,
    // which spawns its own thread), so it gets a thread here.
    {
        let (name, tx, cancel, pid) = (
            pipe_name.clone(),
            tx.clone(),
            cancel.clone(),
            child_pid.clone(),
        );
        if let Err(e) = std::thread::Builder::new()
            .name("kiosk-launcher-pipe".into())
            .spawn(move || pipe::serve(&name, tx, cancel, pid))
        {
            // Without the pipe there are no Ready/heartbeat events, so the FSM
            // restarts the child at every startup-grace expiry. Supervising that
            // way is still better than not supervising at all.
            eprintln!("kiosk-launcher: heartbeat pipe thread failed to start ({e})");
        }
    }

    let (wd, initial) = Watchdog::new(watchdog_config(bootstrap.as_ref()));
    let mut sink = sink::LauncherSink::new(
        exe,
        config_dir,
        data_dir,
        pipe_name,
        tx,
        child_pid,
        bootstrap.as_ref(),
    );

    let code = loop_::run(rx, wd, initial, &mut sink);
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> std::vec::IntoIter<String> {
        items
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn config_flag_overrides_the_install_dir() {
        assert_eq!(
            resolve_config_dir(args(&["kiosk-launcher", "--config", r"D:\kiosk"])),
            PathBuf::from(r"D:\kiosk")
        );
    }

    #[test]
    fn without_the_flag_the_install_dir_is_the_exes_own_directory() {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .expect("the test binary has a parent directory");
        assert_eq!(resolve_config_dir(args(&["kiosk-launcher"])), exe_dir);
    }

    #[test]
    fn a_missing_ini_falls_back_to_the_spec_default_timings() {
        let cfg = watchdog_config(None);
        assert_eq!(cfg.startup_grace_s, 90);
        assert_eq!(cfg.healthy_run_s, 120);
        assert_eq!(cfg.channel_grace_s, 30);
    }

    #[test]
    fn the_ini_drives_the_watchdog_timings_when_it_parses() {
        let ini = "[kiosk]\nconfig_url = https://e/c.json\nsite = s\nproject_id = p\n\
                   credential = c.json\nstartup_grace_s = 45\nhealthy_run_s = 300\n\
                   channel_grace_s = 5\n\n[bootstrap]\nurl = https://app.example.com/\n";
        let b = BootstrapConfig::parse(ini).expect("valid ini");
        let cfg = watchdog_config(Some(&b));
        assert_eq!(cfg.startup_grace_s, 45);
        assert_eq!(cfg.healthy_run_s, 300);
        assert_eq!(cfg.channel_grace_s, 5);
    }
}
