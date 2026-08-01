//! kiosk-launcher — the supervisor (spec §3.1). It spawns and watches
//! `kiosk-main` over a named-pipe heartbeat and executes whatever the pure E1
//! `watchdog` FSM decides; it reimplements no supervise logic of its own.
//!
//! Assembly only: three source threads (pipe reader, 1 s timer, child waiter)
//! feed ONE `mpsc::Sender<watchdog::Event>`, and a single loop owns the
//! `Watchdog` and dispatches each returned `Action` to the [`sink::LauncherSink`].

//!
//! The modules themselves live in this crate's lib target (`lib.rs`) rather than
//! being declared here, so RT-13 can link them; this file remains the only
//! production entry point and the only place the assembly below exists.

use kiosk_launcher::{job, loop_, pipe, sink, timer};

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
///
/// Both degraded paths also drop a `startup-degraded.txt` breadcrumb in the data
/// dir (see [`sink::breadcrumb`]): under a Scheduled Task these `eprintln!`s have
/// no console to reach, and a device that supervises perfectly while reporting
/// nothing must not look healthy to the fleet.
fn load_bootstrap(config_dir: &Path, data_dir: &Path) -> Option<BootstrapConfig> {
    let path = config_dir.join("kiosk.ini");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            sink::breadcrumb(
                data_dir,
                "config",
                &format!("cannot read {}: {e}", path.display()),
            );
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
            sink::breadcrumb(
                data_dir,
                "config",
                &format!("{} is not a valid kiosk.ini: {e}", path.display()),
            );
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
    // Degraded-start warnings raised before `LauncherSink::new` exists, replayed
    // as breadcrumbs once it does — see where they're drained below.
    let mut degraded: Vec<(&str, String)> = Vec::new();

    // FIRST, before ANY side effect: nothing above this point touches a file,
    // creates the heartbeat pipe or spawns a child, so a second launcher exits
    // without disturbing a single byte of the running one's state.
    //
    // The token is bound for the whole of `main` and never dropped (this
    // function ends in `process::exit`). It must NEVER become `let _ = ...`:
    // that closes the handle immediately and frees the name, disarming the
    // check for the next launcher that starts.
    let _single_instance = match job::acquire_single_instance() {
        Ok(Some(token)) => Some(token),
        // A peer supervises. Deliberate clean exit — a success, not a failure.
        Ok(None) => {
            eprintln!("kiosk-launcher: another kiosk-launcher is running; exiting");
            std::process::exit(0);
        }
        // The mutex could not be CREATED (e.g. no SeCreateGlobalPrivilege for
        // the `Global\` namespace). Distinct from already-held on purpose:
        // conflating them would make the launcher silently refuse to start,
        // which is the black screen this binary exists to prevent. Never block
        // boot on a hardening failure.
        Err(e) => {
            eprintln!(
                "kiosk-launcher: single-instance mutex unavailable ({e}); \
                 continuing without double-start protection"
            );
            degraded.push(("mutex", e.to_string()));
            None
        }
    };

    let config_dir = resolve_config_dir(std::env::args());
    let data_dir = resolve_data_dir();
    let bootstrap = load_bootstrap(&config_dir, &data_dir);
    let exe = resolve_main_exe(&config_dir);

    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    // 0 == "no live child". The sink is this atomic's only writer; `pipe::serve`
    // reads it to authenticate the heartbeat client and to decide whether a
    // broken pipe is a channel fault or just a dead child's corpse.
    let child_pid = Arc::new(AtomicU32::new(0));
    let pipe_name = pipe::instance_name();

    timer::spawn_timer(tx.clone(), cancel.clone());
    // Clones for the pipe thread, captured now because `LauncherSink::new`
    // below moves `tx`, `child_pid`, `pipe_name`, and `data_dir` by value; the
    // thread itself isn't spawned until after that call returns (see there).
    let pipe_thread_args = (
        pipe_name.clone(),
        data_dir.clone(),
        tx.clone(),
        cancel.clone(),
        child_pid.clone(),
    );

    // The kill-on-close job the sink assigns every child to. A failure here is
    // WARNING + continue: the launcher supervises exactly as it did before
    // P1-F1, it just can't take the child with it when it dies unexpectedly.
    let job = match job::Job::create() {
        Ok(job) => Some(job),
        Err(e) => {
            eprintln!(
                "kiosk-launcher: job object unavailable ({e}); a supervised \
                 kiosk-main will survive an unexpected launcher death"
            );
            degraded.push(("job", e.to_string()));
            None
        }
    };

    let (wd, initial) = Watchdog::new(watchdog_config(bootstrap.as_ref()));
    let mut sink = sink::LauncherSink::new(
        exe,
        config_dir,
        data_dir.clone(),
        pipe_name,
        tx,
        child_pid,
        bootstrap.as_ref(),
        // Moved into the sink, which lives for the rest of `main`: the job must
        // outlive every child it kills, and dropping it early would kill the
        // kiosk rather than merely disable the feature.
        job,
    );

    // Replayed only NOW, after `LauncherSink::new`: its healthy-telemetry arm
    // deletes `startup-degraded.txt`, so a breadcrumb written earlier would be
    // silently erased — the same ordering trap that put the pipe thread below
    // this call. And before the pipe thread starts, so `serve`'s squatted-pipe
    // breadcrumb (a permanent heartbeat outage) still wins the file.
    //
    // `_if_absent`: a `config`/`telemetry` failure this boot is strictly more
    // severe than losing kill-on-close, and the file only holds one line.
    for (reason, detail) in &degraded {
        sink::breadcrumb_if_absent(&data_dir, reason, detail);
    }

    // Must start after `LauncherSink::new`: its healthy-telemetry arm clears a
    // stale `startup-degraded.txt` breadcrumb left over from a previous boot.
    // A squatted-pipe breadcrumb written by `serve` before that clear runs
    // would be deleted along with it, silently losing the one diagnostic for
    // a permanent heartbeat outage. `serve` blocks its caller for its whole
    // lifetime (unlike `spawn_timer`, which spawns its own thread), so it
    // gets a thread here.
    {
        let (name, data, tx, cancel, pid) = pipe_thread_args;
        if let Err(e) = std::thread::Builder::new()
            .name("kiosk-launcher-pipe".into())
            .spawn(move || pipe::serve(&name, &data, tx, cancel, pid))
        {
            // Without the pipe there are no Ready/heartbeat events, so the FSM
            // restarts the child at every startup-grace expiry. Supervising that
            // way is still better than not supervising at all.
            eprintln!("kiosk-launcher: heartbeat pipe thread failed to start ({e})");
        }
    }

    let code = loop_::run(rx, wd, initial, &mut sink);
    // No `cancel.store(true)` here: `process::exit` tears every thread down
    // regardless and nothing observes the flag on this path, so setting it
    // would only read like a shutdown handshake that does not exist. `cancel`
    // is for the threads' own between-blocking-calls checks.
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
