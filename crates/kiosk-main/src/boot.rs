//! Boot wiring (spec §5, P1-D2a Task 2): parse `kiosk.ini`, resolve device identity, boot
//! the `ConfigManager`, and derive the driver's `MachineConfig` + first `AppEvent`. Pure
//! marshalling only — parsing, signature verification, defaults and validation all live in
//! kiosk-core; this module never reimplements them.
//!
//! Wired into `main.rs` (Task 6): `main` calls [`boot`] and builds the `Driver`/
//! `TauriSink` from the returned `Booted`.

use std::path::Path;

use kiosk_core::app::state::{Event as AppEvent, MachineConfig, DEFAULT_ERROR_RETRY_SECONDS};
use kiosk_core::config::bootstrap::BootstrapConfig;
use kiosk_core::config::schema::Content;
use kiosk_core::config::signature::VerifyingKey;
use kiosk_core::config::store::ConfigStore;
use kiosk_core::config::ConfigManager;
use kiosk_core::error::ConfigError;
use kiosk_core::logging::auth::ServiceAccount;

use crate::credential_acl;

/// The stable `config.error` reason (spec §8/SEC-09) for a credential whose DACL
/// grants read to someone other than its owner/SYSTEM — reported distinctly from a
/// signature/rollback/device-binding rejection. Shared with `fetch::run` (the
/// reload gate), which re-checks the same DACL on the config-poll cadence.
pub(crate) const CREDENTIAL_PERMISSIONS_REASON: &str = "credential_permissions";

/// The exact operator-facing message (spec §8/SEC-09) for a credential-permissions
/// violation, shown on `safe.html?err=` and handed to `telem.config_error` (which
/// only carries `reason`, not this text — the reload gate emits the same text via
/// [`CREDENTIAL_PERMISSIONS_REASON`]'s companion `try_send`).
pub(crate) const CREDENTIAL_PERMISSIONS_MESSAGE: &str =
    "credential file permissions are not owner-only — refusing to load";

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

/// Config/credential load decision. Faults carry a default-backed boot so the normal
/// hardened window and safe-page path can still run without remote work.
pub enum BootOutcome {
    Ready(Booted),
    RenderSafe {
        booted: Booted,
        error: String,
        /// `Some(CREDENTIAL_PERMISSIONS_REASON)` iff this fault is the SEC-09
        /// credential-DACL violation — `None` for every other boot fault (bad
        /// `kiosk.ini`, missing/malformed credential JSON), which today has no
        /// stable reason code and is never spooled via telemetry.
        reason: Option<&'static str>,
    },
}

impl BootOutcome {
    pub fn into_parts(self) -> (Booted, Option<String>, Option<&'static str>) {
        match self {
            Self::Ready(booted) => (booted, None, None),
            Self::RenderSafe {
                booted,
                error,
                reason,
            } => (booted, Some(error), reason),
        }
    }
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

/// Load boot config and credential without letting either fault abort the process.
pub fn load(ini_path: &Path, data_dir: &Path, machine_id: Option<&str>) -> BootOutcome {
    let loaded = std::fs::read_to_string(ini_path)
        .map_err(|e| format!("kiosk-main: cannot read {} ({e})", ini_path.display()))
        .and_then(|text| {
            boot(&text, data_dir).map_err(|e| {
                format!(
                    "kiosk-main: {} is not a valid kiosk.ini: {e}",
                    ini_path.display()
                )
            })
        });

    let booted = match loaded {
        Ok(booted) => booted,
        Err(error) => {
            return BootOutcome::RenderSafe {
                booted: safe_boot(data_dir, machine_id),
                error,
                reason: None,
            }
        }
    };

    let credential_path = ini_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&booted.manager.bootstrap().credential);
    let credential = std::fs::read_to_string(&credential_path)
        .map_err(|e| format!("cannot read {} ({e})", credential_path.display()))
        .and_then(|json| {
            ServiceAccount::from_json(&json)
                .map(|_| ())
                .map_err(|e| format!("cannot parse {} ({e})", credential_path.display()))
        });

    match credential {
        // SEC-09 boot gate: the credential parses fine, but that alone is not
        // enough to trust it — check its DACL before letting `telemetry::build`
        // ever touch it. `Ok(false)`/`Err` (fail closed) wins over `Ready`: a
        // world-readable service-account key must never go live just because its
        // JSON happens to be well-formed.
        Ok(())
            if credential_acl::is_violation(credential_acl::credential_is_owner_only(
                &credential_path,
            )) =>
        {
            BootOutcome::RenderSafe {
                booted,
                error: CREDENTIAL_PERMISSIONS_MESSAGE.to_string(),
                reason: Some(CREDENTIAL_PERMISSIONS_REASON),
            }
        }
        Ok(()) => BootOutcome::Ready(booted),
        Err(error) => BootOutcome::RenderSafe {
            booted,
            error: format!("kiosk-main: {error}"),
            reason: None,
        },
    }
}

fn safe_boot(data_dir: &Path, machine_id: Option<&str>) -> Booted {
    let device_id = machine_id
        .map(str::trim)
        .filter(|id| !id.is_empty() && !id.chars().any(|c| matches!(c, '\r' | '\n')))
        .unwrap_or("unknown");
    let ini = format!(
        "[kiosk]\nconfig_url = https://invalid/\ndevice_id = {device_id}\nsite = safe\nproject_id = safe\ncredential = missing\n\n[bootstrap]\nurl = {APP_SAFE_URL}\n"
    );
    boot(&ini, data_dir).expect("built-in safe config is valid")
}

const APP_SAFE_URL: &str = "http://tauri.localhost/safe.html";

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

    fn valid_ini() -> &'static str {
        "[kiosk]\nconfig_url = https://cfg.test/c.json\ndevice_id = test-device\nsite = s\nproject_id = p\ncredential = cred.json\n\n[bootstrap]\nurl = https://site.test/\n"
    }

    fn assert_render_safe(outcome: BootOutcome, error_fragment: &str) {
        match outcome {
            BootOutcome::RenderSafe { error, .. } => assert!(
                error.contains(error_fragment),
                "expected {error_fragment:?} in {error:?}"
            ),
            BootOutcome::Ready(_) => panic!("load fault must render safe"),
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

    #[test]
    fn corrupt_config_load_returns_render_safe() {
        let dir = tempfile::tempdir().unwrap();
        let ini_path = dir.path().join("kiosk.ini");
        std::fs::write(&ini_path, "not ini").unwrap();

        match load(&ini_path, dir.path(), Some("machine-01")) {
            BootOutcome::RenderSafe { booted, error, .. } => {
                assert_eq!(booted.manager.device_id(), "machine-01");
                assert!(error.contains("not a valid kiosk.ini"));
            }
            BootOutcome::Ready(_) => panic!("config fault must render safe"),
        }
    }

    #[test]
    fn missing_config_load_returns_render_safe() {
        let dir = tempfile::tempdir().unwrap();
        assert_render_safe(
            load(&dir.path().join("kiosk.ini"), dir.path(), None),
            "cannot read",
        );
    }

    #[test]
    fn missing_credential_load_returns_render_safe() {
        let dir = tempfile::tempdir().unwrap();
        let ini_path = dir.path().join("kiosk.ini");
        std::fs::write(&ini_path, valid_ini()).unwrap();
        assert_render_safe(load(&ini_path, dir.path(), None), "cannot read");
    }

    #[test]
    fn malformed_credential_load_returns_render_safe() {
        let dir = tempfile::tempdir().unwrap();
        let ini_path = dir.path().join("kiosk.ini");
        std::fs::write(&ini_path, valid_ini()).unwrap();
        std::fs::write(dir.path().join("cred.json"), "{}").unwrap();
        assert_render_safe(load(&ini_path, dir.path(), None), "cannot parse");
    }

    /// A syntactically valid service-account JSON (no PEM validation — see
    /// `ServiceAccount::from_json`, which only checks the three fields are
    /// non-empty) — good enough to clear the boot gate's JSON-validity check so
    /// the DACL check underneath it is what's actually under test below.
    fn valid_credential_json() -> &'static str {
        r#"{"private_key":"x","client_email":"a@b","token_uri":"https://oauth2.googleapis.com/token"}"#
    }

    /// Forces `path` to an owner-only DACL (current user only), mirroring
    /// `credential_acl::tests::credential_is_owner_only_reflects_real_dacl`'s
    /// fixture setup: a freshly created temp file's inherited ACL is NOT
    /// owner-only on a typical dev/CI host (e.g. an inherited `BUILTIN\
    /// Administrators` ACE distinct from the owner SID), so tests that need a
    /// genuinely-trusted credential must force it rather than assume it.
    #[cfg(windows)]
    fn force_owner_only_acl(path: &Path) {
        use std::process::Command;

        let out = Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .expect("failed to spawn whoami");
        let sid = String::from_utf8_lossy(&out.stdout)
            .trim()
            .rsplit(',')
            .next()
            .expect("whoami /user csv output should have a SID field")
            .trim_matches('"')
            .to_string();

        assert!(Command::new("icacls")
            .arg(path)
            .arg("/setowner")
            .arg(format!("*{sid}"))
            .output()
            .expect("failed to spawn icacls /setowner")
            .status
            .success());
        assert!(Command::new("icacls")
            .arg(path)
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg(format!("*{sid}:(F)"))
            .output()
            .expect("failed to spawn icacls /inheritance:r")
            .status
            .success());
    }

    /// The SEC-09 boot gate must not fire on a normal, valid boot — the
    /// non-Windows stub (`credential_is_owner_only` -> `Ok(true)`) and a real
    /// Windows owner-only ACL both take this same `Ready` path.
    #[test]
    fn valid_credential_load_is_ready_not_render_safe() {
        let dir = tempfile::tempdir().unwrap();
        let ini_path = dir.path().join("kiosk.ini");
        std::fs::write(&ini_path, valid_ini()).unwrap();
        let cred_path = dir.path().join("cred.json");
        std::fs::write(&cred_path, valid_credential_json()).unwrap();
        // A fresh temp file's inherited ACL is not guaranteed owner-only (see
        // `force_owner_only_acl`'s doc) — force it so this test genuinely
        // exercises the non-violation path, not an accident of the host's
        // default ACL.
        #[cfg(windows)]
        force_owner_only_acl(&cred_path);
        match load(&ini_path, dir.path(), None) {
            BootOutcome::Ready(_) => {}
            BootOutcome::RenderSafe { error, .. } => {
                panic!("a valid ini + valid credential must boot Ready, got {error:?}")
            }
        }
    }

    /// Windows-host: a credential whose DACL grants read to `BUILTIN\Users` (a
    /// non-owner principal) must render safe with the distinct
    /// `credential_permissions` reason — never the generic "cannot read"/"cannot
    /// parse" messages the pre-existing content-validation faults above use.
    #[cfg(windows)]
    #[test]
    fn bad_credential_dacl_load_returns_render_safe_with_credential_permissions_reason() {
        use std::process::Command;

        let dir = tempfile::tempdir().unwrap();
        let ini_path = dir.path().join("kiosk.ini");
        std::fs::write(&ini_path, valid_ini()).unwrap();
        let cred_path = dir.path().join("cred.json");
        std::fs::write(&cred_path, valid_credential_json()).unwrap();
        force_owner_only_acl(&cred_path);

        // Widen the DACL: BUILTIN\Users (well-known SID, locale-independent) gets
        // read — a violation per kiosk-core's `acl::is_read_owner_only`.
        let out = Command::new("icacls")
            .arg(&cred_path)
            .arg("/grant")
            .arg("*S-1-5-32-545:(R)")
            .output()
            .expect("failed to spawn icacls");
        assert!(
            out.status.success(),
            "icacls /grant failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        match load(&ini_path, dir.path(), None) {
            BootOutcome::RenderSafe { error, reason, .. } => {
                assert_eq!(reason, Some(CREDENTIAL_PERMISSIONS_REASON));
                assert_eq!(error, CREDENTIAL_PERMISSIONS_MESSAGE);
            }
            BootOutcome::Ready(_) => panic!("a bad credential DACL must render safe"),
        }
    }
}
