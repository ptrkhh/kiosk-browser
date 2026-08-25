//! Config-fetch task (spec §5, P1-D2a Task 3): the poll loop that periodically fetches the
//! remote config, feeds it through `ConfigManager::apply_fetched`, and reports the outcome —
//! either an at-level `AppEvent::ConfigApplied` or a telemetry-only rejection. Validation
//! itself (parse, signature, device binding, anti-rollback, schema/ranges) lives entirely in
//! kiosk-core; this module never reimplements it.
//!
//! Wired into `main.rs` (Task 6): `main` extracts the real `ConfigManager`/URL/`poll_s`
//! from a `Booted` and spawns [`run`].

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use kiosk_core::app::state::Event as AppEvent;
use kiosk_core::config::ConfigManager;
use tokio::sync::{mpsc, Notify};
use tokio_util::sync::CancellationToken;

use crate::credential_acl;
use crate::nav_policy::{NavPolicy, SharedNavPolicy};

/// The result of one fetch-and-apply attempt.
#[derive(Debug)]
pub enum FetchOutcome {
    Applied {
        home_url: String,
        revision: Option<i64>,
        warnings: Vec<String>,
    },
    Rejected(String),
    Unreachable,
}

/// Maps a fetched body (or a transport failure) onto a `FetchOutcome`. Pure: no I/O, so this
/// is host-tested directly against a real `ConfigManager` (no fake needed — kiosk-core's own
/// `apply_fetched` already has full coverage; this just checks the mapping).
///
/// `Err(())` (timeout / transport error, cfg-10) is `Unreachable`, never a rejection — the
/// prober owns connectivity, not this task (see `run`'s doc comment).
pub fn apply(manager: &mut ConfigManager, body: Result<Vec<u8>, ()>) -> FetchOutcome {
    match body {
        Err(()) => FetchOutcome::Unreachable,
        Ok(bytes) => match manager.apply_fetched(&bytes) {
            Ok(applied) => FetchOutcome::Applied {
                home_url: manager.home_url(),
                revision: applied.revision,
                warnings: applied.warnings,
            },
            Err(e) => FetchOutcome::Rejected(e.to_string()),
        },
    }
}

/// Fetches the config body over HTTP. Any failure (DNS, connect, timeout, non-2xx handled by
/// `apply_fetched`'s own parsing) maps to `Err(())` — the caller only distinguishes
/// "unreachable" from "got bytes", never why.
pub async fn fetch_bytes(url: &str) -> Result<Vec<u8>, ()> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10)) // cfg-10
        .build()
        .map_err(|_| ())?;
    let resp = client.get(url).send().await.map_err(|_| ())?;
    let bytes = resp.bytes().await.map_err(|_| ())?;
    Ok(bytes.to_vec())
}

/// Poll every `poll_s`, or immediately when `Effect::RefetchConfig` pinged `refetch`.
/// Emits `AppEvent::ConfigApplied{url}` AT LEVEL on every successful apply — re-sent every
/// poll, even when the url is unchanged; the FSM handles a repeated or changed url naturally.
/// `Unreachable` is silently ignored: the prober (not this task) owns connectivity signaling.
// Two params over clippy's 7-arg threshold: the live nav policy (pre-existing) plus the
// SEC-09 reload gate's credential path/violation channel. A struct wrapper would obscure
// the call site for what is still a flat, one-purpose parameter list.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    mut manager: ConfigManager,
    url: String,
    poll_s: u64,
    tx: mpsc::Sender<AppEvent>,
    telem: crate::telemetry::Telemetry,
    refetch: Arc<Notify>,
    cancel: CancellationToken,
    nav_policy: SharedNavPolicy,
    policy_updates: std::sync::mpsc::Sender<Arc<NavPolicy>>,
    credential_path: PathBuf,
    credential_violation_tx: mpsc::Sender<String>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(poll_s.max(1)));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = interval.tick() => {}
            _ = refetch.notified() => {}
        }
        // SEC-09 reload gate: a cheap security-info read (never the credential's
        // own contents) on every poll tick — a DACL that went bad since boot must
        // stop this task from ever applying another fetched config. `try_send`
        // (capacity 1): the receiver only ever needs the FIRST report, and this
        // loop breaks right after, so there is at most one send per process life.
        if credential_acl::is_violation(credential_acl::credential_is_owner_only(&credential_path))
        {
            telem.config_error(crate::boot::CREDENTIAL_PERMISSIONS_REASON);
            let _ = credential_violation_tx
                .try_send(crate::boot::CREDENTIAL_PERMISSIONS_MESSAGE.to_string());
            break;
        }
        match apply(&mut manager, fetch_bytes(&url).await) {
            FetchOutcome::Applied {
                home_url,
                revision,
                warnings,
            } => {
                telem.set_level(&manager.current().logging.level);
                telem.config_applied(revision, &warnings);
                // Store the new policy BEFORE the FSM (and thus any navigation it
                // triggers) hears about the applied config — so a navigation kicked off
                // by this very apply is judged against the new allowlist, never the
                // stale one.
                let policy = Arc::new(NavPolicy::from_config(
                    &manager.current().content,
                    &home_url,
                ));
                nav_policy.store(policy.clone());
                let _ = policy_updates.send(policy);
                let _ = tx.send(AppEvent::ConfigApplied { url: home_url }).await;
            }
            FetchOutcome::Rejected(reason) => telem.config_error(&reason),
            FetchOutcome::Unreachable => { /* prober owns connectivity; no event */ }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The only ini fixture that `BootstrapConfig::parse` actually accepts: the four
    /// `[kiosk]` fields it requires (`config_url`, `site`, `project_id`, `credential`) plus
    /// `[bootstrap] url`, plus an explicit `device_id` (no platform machine-id reader exists
    /// yet — see `boot.rs`). The brief's own fixture (`[bootstrap]\nconfig_url=...\n
    /// bootstrap_url=...`) does not parse; this is Task 2's verified-working one.
    const VALID_INI: &str = "[kiosk]\n\
        config_url = https://cfg.test/c.json\n\
        device_id = test-device\n\
        site = s\n\
        project_id = p\n\
        credential = cred.json\n\
        \n\
        [bootstrap]\n\
        url = https://site.test/\n";

    fn manager() -> ConfigManager {
        crate::boot::boot(VALID_INI, tempfile::tempdir().unwrap().path())
            .unwrap()
            .manager
    }

    #[test]
    fn a_transport_error_is_unreachable_not_a_rejection() {
        let mut m = manager();
        assert!(matches!(apply(&mut m, Err(())), FetchOutcome::Unreachable));
    }

    #[test]
    fn garbage_body_is_rejected_not_applied() {
        let mut m = manager();
        match apply(&mut m, Ok(b"not json".to_vec())) {
            FetchOutcome::Rejected(_) => {}
            other => panic!("expected Rejected, got {other:?}"),
        }
    }
}
