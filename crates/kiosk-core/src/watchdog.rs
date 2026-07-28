//! Watchdog supervise FSM (spec §3.1). Pure Mealy machine: no process/pipe/
//! clock — time is injected via event fields (`at`/`now`). The launcher shell
//! (a later plan) feeds this real events and executes the `Action`s returned.
//!
//! Rules 1-3 + 9 (READY arming, heartbeat/miss disambiguation, channel-fault
//! grace, exit-86) plus rules 4-6 (backoff doubling with a 60s ceiling,
//! respawn-on-tick, healthy_run_s backoff/crash-loop-window reset) are
//! implemented here. Crash-loop -> safe mode (rules 7-8) is Task 4; the
//! `safe`/`safe_fails` fields it needs are declared now so that task is
//! purely additive.

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
    #[allow(dead_code)] // Tasks 3/4
    safe: bool, // running --safe
    spawned_at: u64,
    last_heartbeat: u64,
    backoff_s: u64,     // current backoff (Task 3)
    restarts: Vec<u64>, // restart timestamps, sliding window (Task 4)
    #[allow(dead_code)] // Tasks 3/4
    safe_fails: u32, // consecutive --safe fails within healthy_run_s (Task 4)
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
        let fx = vec![
            Action::DrainOrphanedSpool,
            Action::Log(WatchdogEvent::Restart {
                code,
                backoff_s: self.backoff_s,
                cause,
            }),
        ];
        self.restarts.push(at);
        self.backoff_s = self.backoff_s.saturating_mul(2).min(60);
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
                    Phase::Armed => {
                        let fx = if now.saturating_sub(self.last_heartbeat) >= MISS_LIMIT_S {
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
                        } else {
                            Vec::new()
                        };
                        // rule 6: a run past healthy_run_s clears backoff + the
                        // crash-loop window. Idempotent; applied after the miss
                        // check above so a same-tick restart still emits the
                        // pre-reset backoff_s, and this only resets state for
                        // next time.
                        if now.saturating_sub(self.spawned_at) >= self.cfg.healthy_run_s {
                            self.backoff_s = 1;
                            self.restarts.clear();
                        }
                        fx
                    }
                    Phase::BackingOff { until } if now >= until => {
                        self.spawned_at = now;
                        self.last_heartbeat = now;
                        self.phase = Phase::Spawning {
                            grace_until: now + self.cfg.startup_grace_s,
                        };
                        vec![Action::SpawnMain]
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
        w.on(Event::Tick { now: 100 + 121 }); // ran > healthy_run_s (120)
        let b2 = restart_backoff(&mut w, 100 + 300).unwrap();
        assert!(
            b1 > 0 && b2 == 1,
            "a run past healthy_run_s clears backoff (was {b1}, now {b2})"
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
