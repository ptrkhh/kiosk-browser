//! The driver actor + `EffectSink` seam: executes the FSM's effects. Wired into
//! `main.rs` (Task 6): `TauriSink` is the production `EffectSink`, constructed there
//! together with a `Driver` and spawned via [`run`].

use kiosk_core::app::state::{Effect, Event as AppEvent, Machine};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Executes the effects the FSM returns. The production impl (`TauriSink`, Task 6) drives
/// the webview; tests use a recording fake. Sync: the webview marshals internally.
pub trait EffectSink {
    fn dispatch(&mut self, effect: Effect);
}

/// Owns the single `Machine`. Not `Sync`; lives inside the driver task alone.
pub struct Driver {
    pub machine: Machine,
}

impl Driver {
    pub fn handle(&mut self, event: AppEvent, sink: &mut dyn EffectSink) {
        for effect in self.machine.on(event) {
            sink.dispatch(effect);
        }
    }
}

/// The driver task: drains the event channel until the channel closes or cancellation.
pub async fn run(
    mut rx: mpsc::Receiver<AppEvent>,
    mut driver: Driver,
    mut sink: Box<dyn EffectSink + Send>,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            maybe = rx.recv() => match maybe {
                Some(event) => driver.handle(event, sink.as_mut()),
                None => break,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::app::state::{
        Effect, Event as AppEvent, Machine, MachineConfig, DEFAULT_ERROR_RETRY_SECONDS,
    };
    // `Fallback` lives in `config::schema`; `app::state` only `use`s it privately (not
    // re-exported), so `kiosk_core::app::state::Fallback` (as the brief's snippet has it)
    // does not resolve outside the crate. Same type, its actual public home.
    use kiosk_core::config::schema::Fallback;

    #[derive(Default)]
    struct RecordingSink {
        effects: Vec<Effect>,
    }
    impl EffectSink for RecordingSink {
        fn dispatch(&mut self, effect: Effect) {
            self.effects.push(effect);
        }
    }

    fn cfg() -> MachineConfig {
        MachineConfig {
            fallback: Fallback::Video,
            error_max_retries: 5,
            idle_clear: true,
            error_retry_seconds: DEFAULT_ERROR_RETRY_SECONDS,
        }
    }

    #[test]
    fn boot_with_config_navigates_home() {
        let mut d = Driver {
            machine: Machine::new(cfg()),
        };
        let mut sink = RecordingSink::default();
        d.handle(
            AppEvent::ConfigApplied {
                url: "https://home.test/".into(),
            },
            &mut sink,
        );
        assert_eq!(
            sink.effects,
            vec![Effect::Navigate("https://home.test/".into())]
        );
    }

    #[test]
    fn boot_without_config_shows_video() {
        let mut d = Driver {
            machine: Machine::new(cfg()),
        };
        let mut sink = RecordingSink::default();
        d.handle(AppEvent::ConfigUnavailable, &mut sink);
        assert_eq!(sink.effects, vec![Effect::ShowVideo]);
    }

    #[test]
    fn offline_then_reconnect_refetches_then_navigates() {
        let mut d = Driver {
            machine: Machine::new(cfg()),
        };
        let mut sink = RecordingSink::default();
        d.handle(
            AppEvent::ConfigApplied {
                url: "https://home.test/".into(),
            },
            &mut sink,
        );
        d.handle(
            AppEvent::LinkChanged(kiosk_core::net::prober::Link::Offline),
            &mut sink,
        );
        sink.effects.clear();
        d.handle(AppEvent::Reconnected, &mut sink);
        assert_eq!(sink.effects, vec![Effect::RefetchConfig]);
    }
}
