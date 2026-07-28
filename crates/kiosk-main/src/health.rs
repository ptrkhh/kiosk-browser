//! Periodic `health.sample` timer (spec §6, P1-D2e Task 2). BASIC (P1) fields only —
//! CPU %, mem used/total, disk-free for the data-dir volume, uptime, and
//! `spool_dropped_expired`. Webview-process RSS / `max_webview_mem_mb` enforcement
//! is P2 and does not belong here. All sampling logic lives in
//! `kiosk_core::metrics` (Task 1); this module only owns the tick/cancel loop.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sysinfo::{Disks, System};
use tokio::time::{interval, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use crate::telemetry::Telemetry;

/// Emit a `health.sample` every `period_s` (spec §6; clamped to [10,3600], config
/// default 60 — `logging.health_sample_s`, already range-validated by
/// `kiosk_core::config::validate`; the clamp here is defense-in-depth only).
/// `dropped` reads the logger's `spool.dropped_expired` counter at sample time
/// (see `telemetry::run`'s doc comment for why this is a closure over a shared
/// atomic rather than a direct `&Logger` call). Cancel-aware.
#[allow(clippy::too_many_arguments)] // one parameter per HealthSample input + wiring; a
                                     // struct wrapper would just move these fields, not cut them.
pub async fn run(
    mut sys: System,
    mut disks: Disks,
    data_dir: std::path::PathBuf,
    started: Instant,
    period_s: u64,
    dropped: Arc<dyn Fn() -> u64 + Send + Sync>,
    telem: Telemetry,
    cancel: CancellationToken,
) {
    let mut tick = interval(Duration::from_secs(period_s.clamp(10, 3600)));
    // A heartbeat wants a steady cadence, not catch-up: after a suspend/resume or
    // runtime stall, fire one tick and resync rather than bursting every missed one.
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tick.tick() => {
                let s = kiosk_core::metrics::sample(&mut sys, &mut disks, &data_dir, started);
                telem.health(kiosk_core::metrics::to_fields(&s, dropped()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_exits_promptly_on_cancel() {
        let cancel = CancellationToken::new();
        cancel.cancel(); // already cancelled before the task ever ticks
        let task = run(
            System::new(),
            Disks::new(),
            std::path::PathBuf::from("."),
            Instant::now(),
            10,
            Arc::new(|| 0),
            Telemetry::disabled(),
            cancel,
        );
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("health::run should exit as soon as the token is cancelled, not hang");
    }
}
