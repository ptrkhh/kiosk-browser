//! Boot wiring (spec §5, P1-D2a Task 2): parse `kiosk.ini`, resolve device identity, boot
//! the `ConfigManager`, and derive the driver's `MachineConfig` + first `AppEvent`. Pure
//! marshalling only — parsing, signature verification, defaults and validation all live in
//! kiosk-core; this module never reimplements them.
//!
//! Not yet wired into `main.rs` — a later task (the actor spine, Task 6) constructs the
//! `Driver`/`TauriSink` from a `Booted`. Until then this module's public surface has no
//! caller outside its own tests (mirrors `driver.rs`'s Task 1 note).
#![allow(dead_code)]

use std::path::Path;

use kiosk_core::app::state::{Event as AppEvent, MachineConfig, DEFAULT_ERROR_RETRY_SECONDS};
use kiosk_core::config::bootstrap::BootstrapConfig;
use kiosk_core::config::schema::Content;
use kiosk_core::config::signature::VerifyingKey;
use kiosk_core::config::store::ConfigStore;
use kiosk_core::config::ConfigManager;
use kiosk_core::error::ConfigError;

/// Map the effective remote `content` onto the FSM's static config (spec §3.3).
pub fn machine_config(content: &Content) -> MachineConfig {
    MachineConfig {
        fallback: content.fallback,
        error_max_retries: content.error_max_retries,
        idle_clear: content.clear_data_on_reset,
        // No remote field controls the countdown length (spec §3.3); kiosk-core default.
        error_retry_seconds: DEFAULT_ERROR_RETRY_SECONDS,
    }
}

/// A config that resolves a home url boots straight to it; otherwise the machine waits on
/// the video (rule 2). Home is present whenever `content.url` or `[bootstrap] url` is set,
/// which `ConfigManager::home_url()` already resolves.
pub fn boot_event(home_url: &str) -> AppEvent {
    AppEvent::ConfigApplied {
        url: home_url.to_string(),
    }
}

/// The outcome of a boot: the live `ConfigManager`, the FSM config derived from its current
/// content, the first event to feed the driver, and any warnings surfaced along the way
/// (spec §6 `config.applied`).
pub struct Booted {
    pub manager: ConfigManager,
    pub machine_cfg: MachineConfig,
    pub first_event: AppEvent,
    pub warnings: Vec<String>,
}

/// The compiled-in Ed25519 verifying key (spec §8). `None` when no key was baked in at
/// build time (`KIOSK_CONFIG_PUBKEY_B64` unset) — fail-closed: `ConfigManager` then rejects
/// every fetched/last-good config, but boot itself still succeeds off the bootstrap config.
pub fn pinned_key() -> Option<VerifyingKey> {
    kiosk_core::config::signature::pinned_key().ok()
}

/// Parse `kiosk.ini`, resolve the device id, boot the `ConfigManager` against `data_dir`,
/// and derive the FSM's starting config + first event.
pub fn boot(ini_text: &str, data_dir: &Path) -> Result<Booted, ConfigError> {
    let bootstrap = BootstrapConfig::parse(ini_text)?;
    // ponytail: no platform machine-id reader is wired yet (Windows MachineGuid etc. is the
    // kiosk-main platform task's job, per kiosk-core's identity module docs) — pass `None`
    // and let `[kiosk] device_id` be the only source for now. A build with neither fails
    // closed here (a kiosk with no identity cannot be told apart in telemetry) rather than
    // guessing one; add the platform source as the second argument when that task lands.
    let device_id =
        kiosk_core::identity::effective_device_id(bootstrap.device_id.as_deref(), None)?;
    let store = ConfigStore::new(data_dir);
    let key = pinned_key();
    let (manager, applied) = ConfigManager::boot(bootstrap, device_id, store, key);
    let machine_cfg = machine_config(&manager.current().content);
    let first_event = boot_event(&manager.home_url());
    Ok(Booted {
        manager,
        machine_cfg,
        first_event,
        warnings: applied.warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::config::schema::{Content, Fallback};

    fn content(url: Option<&str>, fallback: Fallback, retries: u32, clear: bool) -> Content {
        Content {
            url: url.map(str::to_string),
            fallback,
            error_max_retries: retries,
            clear_data_on_reset: clear,
            ..Content::default()
        }
    }

    #[test]
    fn machine_config_maps_clear_flag_and_retries() {
        let c = content(Some("https://h/"), Fallback::ErrorPage, 3, false);
        let mc = machine_config(&c);
        assert_eq!(mc.error_max_retries, 3);
        assert_eq!(mc.fallback, Fallback::ErrorPage);
        assert!(!mc.idle_clear, "idle_clear mirrors clear_data_on_reset");
        assert_eq!(
            mc.error_retry_seconds,
            kiosk_core::app::state::DEFAULT_ERROR_RETRY_SECONDS
        );
    }

    #[test]
    fn boot_event_navigates_when_home_is_present() {
        assert_eq!(
            boot_event("https://h/"),
            AppEvent::ConfigApplied {
                url: "https://h/".into()
            }
        );
    }

    #[test]
    fn pinned_key_is_none_when_no_key_is_compiled_in() {
        // This dev build does not set KIOSK_CONFIG_PUBKEY_B64 — fail closed.
        assert!(pinned_key().is_none());
    }

    #[test]
    fn boot_with_no_store_uses_bootstrap_and_emits_config_applied() {
        let dir = tempfile::tempdir().unwrap();
        // A genuinely minimal-but-valid kiosk.ini: BootstrapConfig::parse requires
        // kiosk.config_url/site/project_id/credential and bootstrap.url. `device_id` is set
        // explicitly since no platform machine-id reader exists yet (see `boot`'s comment).
        let ini = "[kiosk]\nconfig_url = https://cfg.test/c.json\n\
                   device_id = test-device\nsite = s\nproject_id = p\n\
                   credential = cred.json\n\n\
                   [bootstrap]\nurl = https://site.test/\n";
        let booted = boot(ini, dir.path()).expect("boot ok");
        match booted.first_event {
            AppEvent::ConfigApplied { url } => assert_eq!(url, "https://site.test/"),
            other => panic!("expected ConfigApplied, got {other:?}"),
        }
    }
}
