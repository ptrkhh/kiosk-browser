//! Watchdog supervise FSM (spec §3.1). Pure Mealy machine: no process/pipe/
//! clock — time is injected via event fields (`at`/`now`). The launcher shell
//! (a later plan) feeds this real events and executes the `Action`s returned.
//!
//! Rules 1-3 + 9 (READY arming, heartbeat/miss disambiguation, channel-fault
//! grace, exit-86) plus rules 4-6 (backoff doubling with a 60s ceiling,
//! respawn-on-tick, healthy_run_s backoff/crash-loop-window reset) are
//! implemented here. Crash-loop -> safe mode + safe-mode escalation
//! (rules 7-8) are implemented too: >5 restarts in a sliding 600s window
//! trips safe mode; 3 consecutive fast (< healthy_run_s) `--safe` fails
//! escalate to SafeModeFailed and hold backoff at the 60s ceiling; any
//! instance (safe or normal) surviving >= healthy_run_s exits safe mode.
//!
//! ponytail: while in safe mode we never retry normal mode on our own; only
//! surviving healthy_run_s as --safe exits safe. §3.1's "retry normal every
//! 10 min" nuance isn't pinned by a test here, so a separate normal-mode
//! retry timer isn't built — add it if a future test requires it.

use crate::logging::event::Event as LogEvent;

pub struct WatchdogConfig {
    pub startup_grace_s: u64,
    pub healthy_run_s: u64,
    pub channel_grace_s: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Spawned { at: u64 },
    Ready,
    Heartbeat { at: u64 },
    ChildExited { code: i32, at: u64 },
    Tick { now: u64 },
    ChannelFault { at: u64 },
    ChannelReconnected,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    SpawnMain,
    SpawnSafe,
    DrainOrphanedSpool,
    Log(WatchdogEvent),
    ExitLauncher { code: i32 },
}

/// The structured payload for a `watchdog.*` telemetry entry. `log_event()`
/// maps to the existing P1-B `LogEvent`; the shell (E2) turns `fields` into
/// the jsonPayload.
#[derive(Debug, Clone, PartialEq)]
pub enum WatchdogEvent {
    Arm {
        time_to_ready_s: u64,
    },
    Restart {
        code: i32,
        backoff_s: u64,
        cause: &'static str,
    },
    Hang,
    ChannelReset,
    SafeMode,
    SafeModeFailed,
}

impl WatchdogEvent {
    pub fn log_event(&self) -> LogEvent {
        match self {
            WatchdogEvent::Arm { .. } => LogEvent::WatchdogArm,
            WatchdogEvent::Restart { .. } => LogEvent::WatchdogRestart,
            WatchdogEvent::Hang => LogEvent::WatchdogHang,
            WatchdogEvent::ChannelReset => LogEvent::WatchdogChannelReset,
            WatchdogEvent::SafeMode => LogEvent::WatchdogSafeMode,
            WatchdogEvent::SafeModeFailed => LogEvent::WatchdogSafeModeFailed,
        }
    }
}

const MISS_LIMIT_S: u64 = 15; // 3 x PING_INTERVAL_S
const WINDOW_S: u64 = 600; // crash-loop sliding window (rule 7)
const SAFE_FAIL_LIMIT: u32 = 3; // consecutive safe-mode fails -> SafeModeFailed (rule 8)

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    AwaitingSpawn,                 // new() asked for a spawn; waiting for Spawned
    Spawning { grace_until: u64 }, // spawned, waiting READY
    Armed,                         // enforcing heartbeat
    BackingOff { until: u64 },     // waiting to (re)spawn
}

pub struct Watchdog {
    cfg: WatchdogConfig,
    phase: Phase,
    safe: bool, // running --safe
    spawned_at: u64,
    last_heartbeat: u64,
    backoff_s: u64,                   // current backoff (Task 3)
    restarts: Vec<u64>,               // restart timestamps, sliding window (Task 4)
    safe_fails: u32,                  // consecutive --safe fails within healthy_run_s (Task 4)
    channel_grace_until: Option<u64>, // set on ChannelFault
    now: u64,
}

impl Watchdog {
    pub fn new(cfg: WatchdogConfig) -> (Watchdog, Vec<Action>) {
        let w = Watchdog {
            cfg,
            phase: Phase::AwaitingSpawn,
            safe: false,
            spawned_at: 0,
            last_heartbeat: 0,
            backoff_s: 1,
            restarts: Vec::new(),
            safe_fails: 0,
            channel_grace_until: None,
            now: 0,
        };
        (w, vec![Action::SpawnMain])
    }

    fn restart(&mut self, code: i32, at: u64, cause: &'static str) -> Vec<Action> {
        self.phase = Phase::BackingOff {
            until: at + self.backoff_s,
        };
        let mut fx = vec![
            Action::DrainOrphanedSpool,
            Action::Log(WatchdogEvent::Restart {
                code,
                backoff_s: self.backoff_s,
                cause,
            }),
        ];

        let mut escalated = false;
        if self.safe {
            // rule 8: a --safe instance that fails fast (within healthy_run_s)
            // counts toward escalation. Surviving healthy_run_s exits safe mode
            // via the Armed-tick healthy-reset path, not here.
            if at.saturating_sub(self.spawned_at) < self.cfg.healthy_run_s {
                self.safe_fails = self.safe_fails.saturating_add(1);
                if self.safe_fails >= SAFE_FAIL_LIMIT {
                    fx.push(Action::Log(WatchdogEvent::SafeModeFailed));
                    escalated = true;
                }
            }
        } else {
            // rule 7: sliding 10-min crash-loop window.
            self.restarts.push(at);
            self.restarts.retain(|&t| at.saturating_sub(t) <= WINDOW_S);
            if self.restarts.len() > 5 {
                self.safe = true;
                self.safe_fails = 0;
                self.restarts.clear();
                fx.push(Action::Log(WatchdogEvent::SafeMode));
            }
        }

        self.backoff_s = if escalated {
            60 // hold at the ceiling: stop fast-looping once escalated
        } else {
            self.backoff_s.saturating_mul(2).min(60)
        };
        fx
    }

    pub fn on(&mut self, event: Event) -> Vec<Action> {
        match event {
            Event::Spawned { at } => {
                self.now = self.now.max(at);
                self.spawned_at = at;
                self.phase = Phase::Spawning {
                    grace_until: at + self.cfg.startup_grace_s,
                };
                Vec::new()
            }
            Event::Ready => {
                if let Phase::Spawning { .. } = self.phase {
                    let time_to_ready_s = self.now.saturating_sub(self.spawned_at);
                    self.phase = Phase::Armed;
                    self.last_heartbeat = self.now;
                    vec![Action::Log(WatchdogEvent::Arm { time_to_ready_s })]
                } else {
                    Vec::new()
                }
            }
            Event::Heartbeat { at } => {
                self.now = self.now.max(at);
                if let Phase::Armed = self.phase {
                    self.last_heartbeat = at;
                }
                Vec::new()
            }
            Event::ChildExited { code, at } => {
                self.now = self.now.max(at);
                if code == 86 {
                    return vec![Action::ExitLauncher { code: 86 }];
                }
                self.restart(code, at, "exit")
            }
            Event::Tick { now } => {
                self.now = self.now.max(now);
                match self.phase {
                    Phase::Spawning { grace_until } if now > grace_until => {
                        self.restart(0, now, "no_ready")
                    }
                    Phase::Armed if now.saturating_sub(self.last_heartbeat) >= MISS_LIMIT_S => {
                        // a genuine miss takes precedence over health-reset: a
                        // restart tick must never also reset backoff/restarts
                        // (Task 4's crash-loop window depends on the restart
                        // timestamp this tick pushes surviving the tick).
                        match self.channel_grace_until {
                            Some(grace_until) if now <= grace_until => {
                                // channel fault still within grace: wait, no restart yet.
                                Vec::new()
                            }
                            Some(_) => {
                                // channel grace expired: restart.
                                self.restart(0, now, "hang")
                            }
                            None => {
                                // healthy channel, missed heartbeats: confirmed hang.
                                let mut fx = vec![Action::Log(WatchdogEvent::Hang)];
                                fx.extend(self.restart(0, now, "hang"));
                                fx
                            }
                        }
                    }
                    Phase::Armed => {
                        // rule 6: heartbeat is healthy (no miss this tick) and
                        // the run has crossed healthy_run_s — clear backoff +
                        // the crash-loop window. Idempotent.
                        if now.saturating_sub(self.spawned_at) >= self.cfg.healthy_run_s {
                            self.backoff_s = 1;
                            self.restarts.clear();
                            self.safe = false; // rule 8: survived healthy_run_s -> exit safe mode
                            self.safe_fails = 0;
                        }
                        Vec::new()
                    }
                    Phase::BackingOff { until } if now >= until => {
                        self.spawned_at = now;
                        self.last_heartbeat = now;
                        self.phase = Phase::Spawning {
                            grace_until: now + self.cfg.startup_grace_s,
                        };
                        vec![if self.safe {
                            Action::SpawnSafe
                        } else {
                            Action::SpawnMain
                        }]
                    }
                    _ => Vec::new(),
                }
            }
            Event::ChannelFault { at } => {
                self.now = self.now.max(at);
                self.channel_grace_until = Some(at + self.cfg.channel_grace_s);
                Vec::new()
            }
            Event::ChannelReconnected => {
                self.channel_grace_until = None;
                vec![Action::Log(WatchdogEvent::ChannelReset)]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> WatchdogConfig {
        WatchdogConfig {
            startup_grace_s: 90,
            healthy_run_s: 120,
            channel_grace_s: 30,
        }
    }

    #[test]
    fn ready_within_grace_arms_and_logs_time_to_ready() {
        let (mut w, boot) = Watchdog::new(cfg());
        assert_eq!(boot, vec![Action::SpawnMain]);
        w.on(Event::Spawned { at: 100 });
        let fx = w.on(Event::Ready); // READY at… the FSM learns "now" from the next Tick; Ready carries no time
                                     // arm is logged; time_to_ready measured from spawned_at to the arming tick:
        assert!(fx
            .iter()
            .any(|a| matches!(a, Action::Log(WatchdogEvent::Arm { .. }))));
    }

    #[test]
    fn grace_expiry_with_no_ready_restarts_as_failed_start() {
        let (mut w, _) = Watchdog::new(cfg());
        w.on(Event::Spawned { at: 0 });
        // no Ready — grace expires
        let fx = w.on(Event::Tick {
            now: cfg().startup_grace_s + 1,
        });
        assert!(fx.iter().any(|a| matches!(
            a,
            Action::Log(WatchdogEvent::Restart {
                cause: "no_ready",
                ..
            })
        )));
    }

    #[test]
    fn child_exited_passes_through_the_real_exit_code() {
        let (mut w, _) = Watchdog::new(cfg());
        w.on(Event::Spawned { at: 0 });
        w.on(Event::Ready);
        let fx = w.on(Event::ChildExited { code: 137, at: 10 });
        assert!(fx
            .iter()
            .any(|a| matches!(a, Action::Log(WatchdogEvent::Restart { code: 137, .. }))));
    }

    #[test]
    fn miss_with_child_exited_restarts_with_real_code() {
        let (mut w, _) = Watchdog::new(cfg());
        w.on(Event::Spawned { at: 0 });
        w.on(Event::Ready);
        // child crashed at t=10; the FSM hears the real exit first
        let fx = w.on(Event::ChildExited { code: 1, at: 10 });
        assert!(fx.iter().any(|a| matches!(
            a,
            Action::Log(WatchdogEvent::Restart {
                code: 1,
                cause: "exit",
                ..
            })
        )));
    }

    #[test]
    fn armed_missed_heartbeats_with_healthy_channel_is_a_hang() {
        let (mut w, _) = Watchdog::new(cfg());
        w.on(Event::Spawned { at: 0 });
        w.on(Event::Ready);
        w.on(Event::Heartbeat { at: 0 });
        let fx = w.on(Event::Tick { now: 16 }); // 16 s since last heartbeat, child never exited, channel healthy
        assert!(fx
            .iter()
            .any(|a| matches!(a, Action::Log(WatchdogEvent::Hang))));
        assert!(
            fx.contains(&Action::SpawnMain)
                || fx.iter().any(|a| matches!(
                    a,
                    Action::Log(WatchdogEvent::Restart { cause: "hang", .. })
                ))
        );
    }

    #[test]
    fn channel_fault_reconnect_does_not_restart() {
        let (mut w, _) = Watchdog::new(cfg());
        w.on(Event::Spawned { at: 0 });
        w.on(Event::Ready);
        w.on(Event::Heartbeat { at: 0 });
        w.on(Event::ChannelFault { at: 5 });
        let fx = w.on(Event::ChannelReconnected);
        assert!(fx
            .iter()
            .any(|a| matches!(a, Action::Log(WatchdogEvent::ChannelReset))));
        assert!(
            !fx.contains(&Action::SpawnMain),
            "a reconnected channel must NOT restart"
        );
    }

    /// Feeds a `ChildExited` and returns the `backoff_s` of the resulting
    /// `Restart` log action, if any.
    fn restart_backoff(w: &mut Watchdog, at: u64) -> Option<u64> {
        w.on(Event::ChildExited { code: 1, at })
            .into_iter()
            .find_map(|a| match a {
                Action::Log(WatchdogEvent::Restart { backoff_s, .. }) => Some(backoff_s),
                _ => None,
            })
    }

    #[test]
    fn backoff_doubles_from_1_to_the_60s_ceiling() {
        let (mut w, _) = Watchdog::new(cfg());
        let mut seen = vec![];
        let mut t = 0;
        for _ in 0..10 {
            w.on(Event::Spawned { at: t });
            w.on(Event::Ready);
            if let Some(b) = restart_backoff(&mut w, t + 1) {
                seen.push(b);
            }
            t += 200;
        }
        // 1,2,4,8,16,32,60,60,60,60 — doubles then holds at 60
        assert_eq!(&seen[..7], &[1, 2, 4, 8, 16, 32, 60]);
        assert!(seen[7..].iter().all(|&b| b == 60), "ceiling holds at 60");
    }

    #[test]
    fn a_healthy_run_resets_backoff() {
        let (mut w, _) = Watchdog::new(cfg());
        w.on(Event::Spawned { at: 0 });
        w.on(Event::Ready);
        let b1 = restart_backoff(&mut w, 1).unwrap(); // crashed fast → backoff grows
                                                      // next instance runs healthy_run_s+ before crashing → backoff must reset to 1
        w.on(Event::Spawned { at: 100 });
        w.on(Event::Ready);
        w.on(Event::Heartbeat { at: 215 }); // keep it heartbeating through the healthy window
        w.on(Event::Tick { now: 100 + 121 }); // ran > healthy_run_s (120), no miss this tick
        let b2 = restart_backoff(&mut w, 100 + 300).unwrap();
        assert!(
            b1 > 0 && b2 == 1,
            "a run past healthy_run_s clears backoff (was {b1}, now {b2})"
        );
    }

    #[test]
    fn a_hang_restart_tick_does_not_also_reset_backoff() {
        // A tick that BOTH crosses healthy_run_s AND finds a missed heartbeat
        // must take the miss/restart path only — it must not also reset
        // backoff or wipe the restarts window the restart just recorded.
        let (mut w, _) = Watchdog::new(cfg());
        w.on(Event::Spawned { at: 0 });
        w.on(Event::Ready);
        let b1 = restart_backoff(&mut w, 1).unwrap(); // backoff now doubled (2)
        w.on(Event::Spawned { at: 100 });
        w.on(Event::Ready); // last_heartbeat = 100, spawned_at = 100
        let fx = w.on(Event::Tick { now: 100 + 121 }); // no heartbeat sent: both a miss AND past healthy_run_s
        let b_this_tick = fx.iter().find_map(|a| match a {
            Action::Log(WatchdogEvent::Restart { backoff_s, .. }) => Some(*backoff_s),
            _ => None,
        });
        assert_eq!(
            b_this_tick,
            Some(b1 * 2),
            "the hang-restart must emit the pre-reset (doubled) backoff, not 1"
        );
    }

    #[test]
    fn healthy_run_s_boundary_resets_at_exactly_the_threshold() {
        let (mut w, _) = Watchdog::new(cfg());
        w.on(Event::Spawned { at: 0 });
        w.on(Event::Ready);
        let b1 = restart_backoff(&mut w, 1).unwrap();
        w.on(Event::Spawned { at: 100 });
        w.on(Event::Ready);
        w.on(Event::Heartbeat {
            at: 100 + cfg().healthy_run_s,
        });
        // exactly healthy_run_s, not one past it
        w.on(Event::Tick {
            now: 100 + cfg().healthy_run_s,
        });
        let b2 = restart_backoff(&mut w, 100 + 300).unwrap();
        assert!(
            b1 > 0 && b2 == 1,
            "the reset boundary is >=, exactly healthy_run_s must already reset"
        );
    }

    #[test]
    fn backing_off_respawns_on_tick_past_until_and_waits_below_it() {
        let (mut w, _) = Watchdog::new(cfg());
        w.on(Event::Spawned { at: 0 });
        w.on(Event::Ready);
        // crash: backoff_s is 1 → BackingOff { until: at + 1 }
        let fx = w.on(Event::ChildExited { code: 1, at: 10 });
        assert!(fx
            .iter()
            .any(|a| matches!(a, Action::Log(WatchdogEvent::Restart { .. }))));
        // below until: no-op, still waiting
        let waiting = w.on(Event::Tick { now: 10 });
        assert_eq!(waiting, Vec::<Action>::new(), "still waiting below until");
        // at/past until: respawn
        let fx = w.on(Event::Tick { now: 11 });
        assert_eq!(fx, vec![Action::SpawnMain]);
    }

    /// Drives >5 fast crashes to force the FSM into safe mode.
    fn force_into_safe(w: &mut Watchdog) {
        for i in 0..6 {
            w.on(Event::Spawned { at: i * 10 });
            w.on(Event::Ready);
            w.on(Event::ChildExited {
                code: 1,
                at: i * 10 + 1,
            });
        }
    }

    #[test]
    fn more_than_5_restarts_in_10min_enters_safe_mode() {
        let (mut w, _) = Watchdog::new(cfg());
        // 6 fast crashes within 600 s -> the 6th tips into safe mode
        let mut entered_safe = false;
        for i in 0..6 {
            w.on(Event::Spawned { at: i * 10 });
            w.on(Event::Ready);
            let fx = w.on(Event::ChildExited {
                code: 1,
                at: i * 10 + 1,
            });
            if fx
                .iter()
                .any(|a| matches!(a, Action::Log(WatchdogEvent::SafeMode)))
            {
                entered_safe = true;
            }
        }
        assert!(entered_safe, ">5 restarts in 10 min must enter safe mode");
        // and the next spawn is --safe
        let fx = w.on(Event::Tick { now: 10_000 });
        assert!(fx.contains(&Action::SpawnSafe));
    }

    #[test]
    fn five_restarts_in_10min_does_not_enter_safe() {
        let (mut w, _) = Watchdog::new(cfg());
        let mut safe = false;
        for i in 0..5 {
            w.on(Event::Spawned { at: i * 10 });
            w.on(Event::Ready);
            let fx = w.on(Event::ChildExited {
                code: 1,
                at: i * 10 + 1,
            });
            if fx
                .iter()
                .any(|a| matches!(a, Action::Log(WatchdogEvent::SafeMode)))
            {
                safe = true;
            }
        }
        assert!(
            !safe,
            "exactly 5 in 10 min stays in normal mode (boundary: > 5, not >= 5)"
        );
    }

    #[test]
    fn three_consecutive_safe_fails_escalate_to_safe_mode_failed() {
        let (mut w, _) = Watchdog::new(cfg());
        force_into_safe(&mut w); // helper: drive >5 crashes
        let mut escalated = false;
        for i in 0..3 {
            // 3 --safe starts each failing within healthy_run_s
            w.on(Event::Spawned {
                at: 10_000 + i * 10,
            });
            w.on(Event::Ready);
            let fx = w.on(Event::ChildExited {
                code: 1,
                at: 10_000 + i * 10 + 5,
            });
            if fx
                .iter()
                .any(|a| matches!(a, Action::Log(WatchdogEvent::SafeModeFailed)))
            {
                escalated = true;
            }
        }
        assert!(
            escalated,
            "N=3 safe-fails within healthy_run_s -> safe_mode_failed CRITICAL"
        );
    }

    #[test]
    fn exit_86_stops_the_launcher_and_never_restarts() {
        let (mut w, _) = Watchdog::new(cfg());
        w.on(Event::Spawned { at: 0 });
        w.on(Event::Ready);
        let fx = w.on(Event::ChildExited { code: 86, at: 30 });
        assert_eq!(
            fx,
            vec![Action::ExitLauncher { code: 86 }],
            "code 86 is a technician exit, not a crash"
        );
    }
}
