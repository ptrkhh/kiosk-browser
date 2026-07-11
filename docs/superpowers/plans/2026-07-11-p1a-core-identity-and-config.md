# P1-A — `kiosk-core`: Device Identity + Configuration Subsystem

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the complete configuration subsystem in `kiosk-core` — `kiosk.ini` bootstrap parsing, device-identity resolution, the remote-config schema with defaults and range checks, Ed25519 signature verification over RFC 8785 JCS canonicalization, anti-rollback revision enforcement, last-known-good persistence, and the manager that orchestrates them in the spec's strict validation order.

**Architecture:** Everything lands in `crates/kiosk-core` as pure Rust — no Tauri, no per-OS API (spec §4 layering rule), so every task is host-testable with plain `cargo test`. The subsystem is a library the later `kiosk-main` and `kiosk-launcher` plans consume: `ConfigManager::boot()` yields the config to run with (last-good → bootstrap fallback), `ConfigManager::apply_fetched()` validates a downloaded config in the order **signature → anti-rollback → schema/ranges** (spec §5.2, §8/SEC-11) and either applies+persists it or rejects the whole document and keeps last-good. Reading the machine ID and performing the HTTP fetch are explicitly **out of scope** — they are platform/IO concerns injected by the caller, which is what keeps this crate pure.

**Tech Stack:** Rust stable, `serde`/`serde_json` (strict JSON), `serde_jcs` (RFC 8785), `ed25519-dalek` v2, `base64`, `thiserror`, `rust-ini`; `tempfile` for tests.

## Global Constraints

- Spec of record: `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (Revision 2). On conflict, the spec wins; note the conflict in the commit message.
- **Layering rule (spec §4):** `kiosk-core` must never depend on Tauri or any per-OS API. Every task here is host-testable. A dependency that pulls in `windows`, `gtk`, `jni`, or `tauri` is a plan violation — stop and report.
- Rust pinned `stable` via `rust-toolchain.toml`; edition 2021; workspace resolver "2".
- Lint gates before every commit: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- **Strict JSON (spec cfg-13):** the on-device parser is `serde_json`. Comments are NOT accepted. The `jsonc` block in the spec is annotated documentation only.
- **Validation is whole-document and atomic (spec cfg-11):** any invalid/out-of-range value rejects the **entire** fetched config — never apply partially. Unknown fields **warn**, they do not reject. Any rejection keeps last-good.
- **Validation order is fixed (spec §5.2, §8):** (1) signature, (2) anti-rollback, (3) schema & ranges. `sig` and `revision` are REQUIRED on every *fetched* config. Locally-sourced config (`[bootstrap]` url, `config-lastgood.json`) is exempt from (1)–(2).
- **Fail closed:** if no pinned public key is compiled in, signature verification fails and every fetched config is rejected. That is correct behavior, not a bug.
- Commit messages: conventional prefix (`feat:`, `fix:`, `chore:`, `docs:`), ending with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`. Commit with `git -c user.name="Patrick Hermawan" -c user.email="patrick.hermawan@outlook.com" commit ...`.
- Never commit anything under `.superpowers/`.
- No new runtime dependency beyond those named in a task without stating why in the commit message.

## File structure

All paths under `crates/kiosk-core/`:

| File | Responsibility |
|---|---|
| `src/lib.rs` | crate root; re-exports; existing `app_version()` |
| `src/error.rs` | `ConfigError`, `FieldError` — the one error vocabulary for the subsystem |
| `src/identity.rs` | pure device-id resolution, `{device_id}` templating, opaque-GUID detection |
| `src/config/mod.rs` | `ConfigManager` — orchestrates boot + apply in the spec's validation order |
| `src/config/bootstrap.rs` | `kiosk.ini` → `BootstrapConfig` |
| `src/config/schema.rs` | `RemoteConfig` + sections, serde defaults, unknown-field capture |
| `src/config/validate.rs` | range/version checks, capability warnings, whole-document error list |
| `src/config/signature.rs` | JCS canonicalization + Ed25519 verify + pinned key |
| `src/config/store.rs` | `config-lastgood.json` persistence + last-applied revision |

---

### Task 1: Error vocabulary + `kiosk.ini` bootstrap parser

**Files:**
- Create: `crates/kiosk-core/src/error.rs`
- Create: `crates/kiosk-core/src/config/mod.rs` (module declarations only in this task)
- Create: `crates/kiosk-core/src/config/bootstrap.rs`
- Modify: `crates/kiosk-core/src/lib.rs` (add `pub mod config; pub mod error;`)
- Modify: `crates/kiosk-core/Cargo.toml` (add deps)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `kiosk_core::error::FieldError { field: String, reason: String, value: String }` (Clone, Debug, PartialEq, Serialize)
  - `kiosk_core::error::ConfigError` enum with variants `Invalid { errors: Vec<FieldError>, rejected_revision: Option<i64> }`, `Signature(String)`, `Rollback { got: i64, last: i64 }`, `UnsupportedVersion(u32)`, `Io(String)`, `Parse(String)`
  - `kiosk_core::config::bootstrap::BootstrapConfig` (fields below) with `BootstrapConfig::parse(&str) -> Result<BootstrapConfig, ConfigError>`
  - `kiosk_core::config::bootstrap::BootstrapExitGesture { pin_hash: String, taps: u8, region: String }`

- [ ] **Step 1: Add dependencies**

Replace `crates/kiosk-core/Cargo.toml` with:

```toml
[package]
name = "kiosk-core"
version.workspace = true
edition.workspace = true

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_jcs = "0.1"
ed25519-dalek = "2"
base64 = "0.22"
thiserror = "2"
rust-ini = "0.21"

[dev-dependencies]
tempfile = "3"
```

(`rust-ini` is used rather than a hand-rolled parser because the spec's `kiosk.ini` carries trailing `; comment` text after values — see the test in Step 2, which pins that behavior regardless of what the crate does by default.)

- [ ] **Step 2: Write the failing tests**

Create `crates/kiosk-core/src/config/bootstrap.rs`:

```rust
//! `kiosk.ini` — the local, per-device bootstrap config written at install time
//! (spec §5.1). This is the only config that exists before any network fetch.

use crate::error::{ConfigError, FieldError};

/// Bootstrap exit gesture, used before the first remote fetch (spec cfg-12).
/// If absent here AND in remote config, the exit gesture is DISABLED.
#[derive(Debug, Clone, PartialEq)]
pub struct BootstrapExitGesture {
    pub pin_hash: String,
    pub taps: u8,
    pub region: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BootstrapConfig {
    pub config_url: String,
    /// `None` when the ini value is empty → caller resolves the machine id.
    pub device_id: Option<String>,
    pub site: String,
    /// `None` when empty → falls back to `site` (spec §6 TEL-04).
    pub region: Option<String>,
    pub project_id: String,
    pub credential: String,
    pub startup_grace_s: u64,
    pub healthy_run_s: u64,
    pub channel_grace_s: u64,
    /// Android-only, local-only. NEVER settable from remote config (spec §5.1).
    pub demo_mode: bool,
    /// `[bootstrap] url` — the home URL until/unless remote config supplies `content.url`.
    pub bootstrap_url: String,
    pub exit_gesture: Option<BootstrapExitGesture>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
[kiosk]
config_url    = https://storage.googleapis.com/kiosk/devices/lobby-01.json
device_id     =                       ; empty -> auto (machine GUID / machine-id / Android ID)
site          = jakarta-hq
region        =                       ; optional
project_id    = my-gcp-project
credential    = kiosk-credential.json
startup_grace_s = 90
healthy_run_s   = 120
channel_grace_s = 30
demo_mode       = false

[bootstrap]
url = https://app.example.com/kiosk   ; home URL until remote config arrives

[exit_gesture]
pin_hash = $argon2id$v=19$m=65536,t=3,p=4$c2FsdA$aGFzaA
taps     = 7
region   = top-left
"#;

    #[test]
    fn parses_full_ini_and_strips_inline_comments() {
        let c = BootstrapConfig::parse(FULL).expect("must parse");
        assert_eq!(
            c.config_url,
            "https://storage.googleapis.com/kiosk/devices/lobby-01.json"
        );
        // Empty value with a trailing comment must be None, NOT the comment text.
        assert_eq!(c.device_id, None);
        assert_eq!(c.region, None);
        assert_eq!(c.site, "jakarta-hq");
        assert_eq!(c.project_id, "my-gcp-project");
        assert_eq!(c.credential, "kiosk-credential.json");
        assert_eq!(c.startup_grace_s, 90);
        assert_eq!(c.healthy_run_s, 120);
        assert_eq!(c.channel_grace_s, 30);
        assert!(!c.demo_mode);
        // Trailing comment must not leak into the URL.
        assert_eq!(c.bootstrap_url, "https://app.example.com/kiosk");
        let g = c.exit_gesture.expect("exit gesture present");
        assert_eq!(g.pin_hash, "$argon2id$v=19$m=65536,t=3,p=4$c2FsdA$aGFzaA");
        assert_eq!(g.taps, 7);
        assert_eq!(g.region, "top-left");
    }

    #[test]
    fn applies_defaults_for_optional_watchdog_fields() {
        let ini = r#"
[kiosk]
config_url = https://example.com/c.json
site = s
project_id = p
credential = cred.json

[bootstrap]
url = https://app.example.com/
"#;
        let c = BootstrapConfig::parse(ini).expect("must parse");
        assert_eq!(c.startup_grace_s, 90);
        assert_eq!(c.healthy_run_s, 120);
        assert_eq!(c.channel_grace_s, 30);
        assert!(!c.demo_mode);
        assert_eq!(c.exit_gesture, None, "no [exit_gesture] section => disabled");
    }

    #[test]
    fn missing_required_fields_are_reported_together() {
        let ini = "[bootstrap]\nurl = https://app.example.com/\n";
        let err = BootstrapConfig::parse(ini).expect_err("must reject");
        match err {
            ConfigError::Invalid { errors, .. } => {
                let fields: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();
                assert!(fields.contains(&"kiosk.config_url"), "got {fields:?}");
                assert!(fields.contains(&"kiosk.site"), "got {fields:?}");
                assert!(fields.contains(&"kiosk.project_id"), "got {fields:?}");
                assert!(fields.contains(&"kiosk.credential"), "got {fields:?}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn missing_bootstrap_url_is_rejected() {
        let ini = "[kiosk]\nconfig_url = https://e/c.json\nsite = s\nproject_id = p\ncredential = c.json\n";
        let err = BootstrapConfig::parse(ini).expect_err("must reject");
        match err {
            ConfigError::Invalid { errors, .. } => {
                assert!(errors.iter().any(|e| e.field == "bootstrap.url"));
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn non_numeric_grace_value_is_a_field_error() {
        let ini = r#"
[kiosk]
config_url = https://e/c.json
site = s
project_id = p
credential = c.json
startup_grace_s = ninety

[bootstrap]
url = https://app.example.com/
"#;
        let err = BootstrapConfig::parse(ini).expect_err("must reject");
        match err {
            ConfigError::Invalid { errors, .. } => {
                let e = errors
                    .iter()
                    .find(|e| e.field == "kiosk.startup_grace_s")
                    .expect("field error present");
                assert_eq!(e.value, "ninety");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }
}
```

Create `crates/kiosk-core/src/config/mod.rs`:

```rust
//! Configuration subsystem (spec §5, §8).

pub mod bootstrap;
```

Create `crates/kiosk-core/src/error.rs`:

```rust
//! The single error vocabulary for the config subsystem.

use serde::Serialize;

/// One offending field in a rejected config. The `config.error` telemetry
/// payload carries a list of these (spec §5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldError {
    pub field: String,
    pub reason: String,
    pub value: String,
}

impl FieldError {
    pub fn new(field: impl Into<String>, reason: impl Into<String>, value: impl Into<String>) -> Self {
        FieldError {
            field: field.into(),
            reason: reason.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Whole-document rejection: the config is never applied partially (spec cfg-11).
    #[error("invalid config: {} field error(s)", .errors.len())]
    Invalid {
        errors: Vec<FieldError>,
        rejected_revision: Option<i64>,
    },
    #[error("signature: {0}")]
    Signature(String),
    #[error("anti-rollback: revision {got} <= last applied {last}")]
    Rollback { got: i64, last: i64 },
    #[error("unsupported_version: {0}")]
    UnsupportedVersion(u32),
    #[error("io: {0}")]
    Io(String),
    #[error("parse: {0}")]
    Parse(String),
}
```

Modify `crates/kiosk-core/src/lib.rs` — add these two lines directly below the existing `//!` doc comment block, above `app_version`:

```rust
pub mod config;
pub mod error;
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p kiosk-core bootstrap`
Expected: FAIL to compile with ``no function or associated item named `parse` found for struct `BootstrapConfig` ``.

- [ ] **Step 4: Implement the parser**

Add to `crates/kiosk-core/src/config/bootstrap.rs`, above the `#[cfg(test)]` module:

```rust
/// Strip a trailing `;`/`#` comment and surrounding whitespace from a raw ini value.
/// `rust-ini`'s behavior here is version-dependent, so we normalize explicitly —
/// the spec's kiosk.ini puts comments after values (spec §5.1).
fn clean(raw: &str) -> String {
    let cut = raw
        .find(|c| c == ';' || c == '#')
        .map(|i| &raw[..i])
        .unwrap_or(raw);
    cut.trim().to_string()
}

fn required<'a>(
    v: Option<&'a str>,
    field: &str,
    errors: &mut Vec<FieldError>,
) -> Option<String> {
    match v.map(clean).filter(|s| !s.is_empty()) {
        Some(s) => Some(s),
        None => {
            errors.push(FieldError::new(field, "required field is missing or empty", ""));
            None
        }
    }
}

fn optional(v: Option<&str>) -> Option<String> {
    v.map(clean).filter(|s| !s.is_empty())
}

fn number<T: std::str::FromStr>(
    v: Option<&str>,
    field: &str,
    default: T,
    errors: &mut Vec<FieldError>,
) -> T {
    match optional(v) {
        None => default,
        Some(s) => match s.parse::<T>() {
            Ok(n) => n,
            Err(_) => {
                errors.push(FieldError::new(field, "not a valid number", &s));
                default
            }
        },
    }
}

impl BootstrapConfig {
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let ini = ini::Ini::load_from_str(text)
            .map_err(|e| ConfigError::Parse(format!("kiosk.ini: {e}")))?;
        let mut errors: Vec<FieldError> = Vec::new();

        let k = ini.section(Some("kiosk"));
        let get = |key: &str| k.and_then(|s| s.get(key));

        let config_url = required(get("config_url"), "kiosk.config_url", &mut errors);
        let site = required(get("site"), "kiosk.site", &mut errors);
        let project_id = required(get("project_id"), "kiosk.project_id", &mut errors);
        let credential = required(get("credential"), "kiosk.credential", &mut errors);

        let startup_grace_s = number(get("startup_grace_s"), "kiosk.startup_grace_s", 90u64, &mut errors);
        let healthy_run_s = number(get("healthy_run_s"), "kiosk.healthy_run_s", 120u64, &mut errors);
        let channel_grace_s = number(get("channel_grace_s"), "kiosk.channel_grace_s", 30u64, &mut errors);

        let demo_mode = match optional(get("demo_mode")) {
            None => false,
            Some(s) => match s.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => true,
                "false" | "0" | "no" => false,
                _ => {
                    errors.push(FieldError::new("kiosk.demo_mode", "not a boolean", &s));
                    false
                }
            },
        };

        let bootstrap_url = required(
            ini.section(Some("bootstrap")).and_then(|s| s.get("url")),
            "bootstrap.url",
            &mut errors,
        );

        let exit_gesture = ini.section(Some("exit_gesture")).and_then(|s| {
            let pin_hash = optional(s.get("pin_hash"))?;
            let taps = number(s.get("taps"), "exit_gesture.taps", 7u8, &mut errors);
            let region = optional(s.get("region")).unwrap_or_else(|| "top-left".to_string());
            Some(BootstrapExitGesture { pin_hash, taps, region })
        });

        if !errors.is_empty() {
            return Err(ConfigError::Invalid { errors, rejected_revision: None });
        }

        Ok(BootstrapConfig {
            config_url: config_url.expect("checked above"),
            device_id: optional(get("device_id")),
            site: site.expect("checked above"),
            region: optional(get("region")),
            project_id: project_id.expect("checked above"),
            credential: credential.expect("checked above"),
            startup_grace_s,
            healthy_run_s,
            channel_grace_s,
            demo_mode,
            bootstrap_url: bootstrap_url.expect("checked above"),
            exit_gesture,
        })
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p kiosk-core bootstrap`
Expected: 5 tests PASS.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean, all tests pass.

```bash
git add crates/kiosk-core/
git commit -m "feat(core): kiosk.ini bootstrap parser and config error vocabulary

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Device identity — resolution, templating, opaque-GUID detection

**Files:**
- Create: `crates/kiosk-core/src/identity.rs`
- Modify: `crates/kiosk-core/src/lib.rs` (add `pub mod identity;`)

**Interfaces:**
- Consumes: `kiosk_core::error::{ConfigError, FieldError}` (Task 1).
- Produces:
  - `kiosk_core::identity::effective_device_id(configured: Option<&str>, machine_id: Option<&str>) -> Result<String, ConfigError>`
  - `kiosk_core::identity::expand_device_id_template(url: &str, device_id: &str) -> String`
  - `kiosk_core::identity::is_opaque_guid(id: &str) -> bool`

**Scope note:** reading the actual machine ID (Windows machine GUID, Linux `/etc/machine-id`, Android ID) is a **platform** concern and stays out of `kiosk-core` (spec §4 layering rule). The caller passes it in; this task owns the pure resolution rules.

- [ ] **Step 1: Write the failing tests**

Create `crates/kiosk-core/src/identity.rs`:

```rust
//! Device identity (spec §4, cfg-09). The effective device_id is used BOTH as the
//! `{device_id}` URL template value and as the Cloud Logging `device_id` label, so
//! URL identity and log identity are identical by construction.

use crate::error::{ConfigError, FieldError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_id_wins_over_machine_id() {
        let id = effective_device_id(Some("lobby-01"), Some("9f8e7d6c")).unwrap();
        assert_eq!(id, "lobby-01");
    }

    #[test]
    fn falls_back_to_machine_id_when_unconfigured() {
        let id = effective_device_id(None, Some("9f8e7d6c")).unwrap();
        assert_eq!(id, "9f8e7d6c");
    }

    #[test]
    fn blank_configured_id_is_treated_as_unset() {
        let id = effective_device_id(Some("   "), Some("machine-1")).unwrap();
        assert_eq!(id, "machine-1");
    }

    #[test]
    fn errors_when_neither_source_yields_an_id() {
        let err = effective_device_id(None, None).expect_err("must fail");
        match err {
            ConfigError::Invalid { errors, .. } => {
                assert_eq!(errors[0].field, "kiosk.device_id");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn expands_every_device_id_placeholder() {
        let url = expand_device_id_template(
            "https://app.example.com/k?device={device_id}&d={device_id}",
            "lobby-01",
        );
        assert_eq!(
            url,
            "https://app.example.com/k?device=lobby-01&d=lobby-01"
        );
    }

    #[test]
    fn leaves_url_untouched_when_no_placeholder() {
        let url = expand_device_id_template("https://app.example.com/k", "lobby-01");
        assert_eq!(url, "https://app.example.com/k");
    }

    #[test]
    fn detects_opaque_guids() {
        // Machine GUIDs are opaque: a human cannot tell which kiosk this is.
        assert!(is_opaque_guid("6f9619ff-8b86-d011-b42d-00c04fc964ff"));
        assert!(is_opaque_guid("6F9619FF8B86D011B42D00C04FC964FF"));
        // Human-assigned names are not.
        assert!(!is_opaque_guid("lobby-01"));
        assert!(!is_opaque_guid("jakarta-hq-entrance"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p kiosk-core identity`
Expected: FAIL to compile with ``cannot find function `effective_device_id` in this scope``.

- [ ] **Step 3: Implement**

Add to `crates/kiosk-core/src/identity.rs`, above the `#[cfg(test)]` module:

```rust
/// Resolve the effective device id: `[kiosk] device_id` if non-empty, else the
/// caller-supplied machine id. Errors when neither is available — a kiosk with no
/// identity cannot be told apart in telemetry, so we refuse to guess.
pub fn effective_device_id(
    configured: Option<&str>,
    machine_id: Option<&str>,
) -> Result<String, ConfigError> {
    let pick = configured
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| machine_id.map(str::trim).filter(|s| !s.is_empty()));

    match pick {
        Some(id) => Ok(id.to_string()),
        None => Err(ConfigError::Invalid {
            errors: vec![FieldError::new(
                "kiosk.device_id",
                "empty and no machine id could be resolved",
                "",
            )],
            rejected_revision: None,
        }),
    }
}

/// Expand every `{device_id}` placeholder in a URL (spec cfg-09).
pub fn expand_device_id_template(url: &str, device_id: &str) -> String {
    url.replace("{device_id}", device_id)
}

/// True when the id looks like a machine GUID rather than a human-assigned name.
/// Callers warn on this at apply time: an opaque id makes fleet triage painful
/// (spec cfg-09).
pub fn is_opaque_guid(id: &str) -> bool {
    let hex: String = id.chars().filter(|c| *c != '-').collect();
    hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit())
}
```

Add `pub mod identity;` to `crates/kiosk-core/src/lib.rs` beside the other `pub mod` lines.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p kiosk-core identity`
Expected: 7 tests PASS.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/kiosk-core/
git commit -m "feat(core): device identity resolution and {device_id} templating

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Remote-config schema with defaults and unknown-field capture

**Files:**
- Create: `crates/kiosk-core/src/config/schema.rs`
- Modify: `crates/kiosk-core/src/config/mod.rs` (add `pub mod schema;`)

**Interfaces:**
- Consumes: nothing (pure types).
- Produces: `kiosk_core::config::schema::{RemoteConfig, Content, Display, Input, ExitGesture, Network, Maintenance, Logging, Fallback, TouchKeyboard, UrlDetail, Permissions}`. Every field has a serde default, so a body of `{}` deserializes successfully (spec §5.2). Each section captures unrecognized keys in `pub unknown: serde_json::Map<String, Value>` via `#[serde(flatten)]` — Task 4 turns those into warnings.
- `RemoteConfig` fields: `version: u32` (default 1), `revision: Option<i64>`, `sig: Option<String>`, `content: Content`, `display: Display`, `input: Input`, `network: Network`, `maintenance: Maintenance`, `logging: Logging`.

- [ ] **Step 1: Write the failing tests**

Create `crates/kiosk-core/src/config/schema.rs`:

```rust
//! The remote config document (spec §5.2). Every *content* field has a default, so a
//! signed body of `{}` is schema-valid. Unknown fields are captured, not rejected —
//! validation turns them into warnings.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_body_deserializes_to_all_defaults() {
        let c: RemoteConfig = serde_json::from_str("{}").expect("empty body is valid");
        assert_eq!(c.version, 1);
        assert_eq!(c.revision, None);
        assert_eq!(c.sig, None);
        assert_eq!(c.content.url, None);
        assert_eq!(c.content.allowlist, Vec::<String>::new());
        assert_eq!(c.content.fallback, Fallback::Video);
        assert_eq!(c.content.error_max_retries, 5);
        assert_eq!(c.content.zoom, 1.0);
        assert_eq!(c.content.idle_reset_seconds, 180);
        assert!(c.content.clear_data_on_reset);
        assert!(!c.content.pdf_view);
        assert!(!c.content.permissions.camera);
        assert_eq!(c.display.cursor_autohide_seconds, 5);
        assert_eq!(c.display.monitor, 0);
        assert!(c.display.keep_awake);
        assert_eq!(c.input.touch_keyboard, TouchKeyboard::Auto);
        assert!(!c.input.allow_context_menu);
        assert!(!c.input.allow_text_selection);
        assert_eq!(c.input.exit_gesture, None);
        assert_eq!(
            c.network.connectivity_check_url,
            "https://www.gstatic.com/generate_204"
        );
        assert_eq!(c.network.probe_online_s, 30);
        assert_eq!(c.network.probe_offline_s, 10);
        assert_eq!(c.network.config_poll_s, 300);
        assert_eq!(c.maintenance.nightly_reload, None);
        assert_eq!(c.maintenance.max_webview_mem_mb, 1500);
        assert_eq!(c.logging.level, "info");
        assert_eq!(c.logging.health_sample_s, 60);
        assert_eq!(c.logging.spool_max_mb, 50);
        assert_eq!(c.logging.spool_reserve_high_mb, 10);
        assert_eq!(c.logging.url_detail, UrlDetail::Path);
    }

    #[test]
    fn parses_a_populated_document() {
        let json = r#"{
          "version": 1,
          "revision": 42,
          "sig": "ed25519:AAAA",
          "content": {
            "url": "https://app.example.com/kiosk?device={device_id}",
            "allowlist": ["https://app.example.com/*"],
            "fallback": "error_page",
            "zoom": 1.25,
            "idle_reset_seconds": 0
          },
          "input": { "touch_keyboard": "off",
                     "exit_gesture": { "taps": 5, "region": "top-right", "pin_hash": "$argon2id$x" } },
          "logging": { "url_detail": "full" }
        }"#;
        let c: RemoteConfig = serde_json::from_str(json).expect("valid");
        assert_eq!(c.revision, Some(42));
        assert_eq!(c.sig.as_deref(), Some("ed25519:AAAA"));
        assert_eq!(
            c.content.url.as_deref(),
            Some("https://app.example.com/kiosk?device={device_id}")
        );
        assert_eq!(c.content.fallback, Fallback::ErrorPage);
        assert_eq!(c.content.zoom, 1.25);
        assert_eq!(c.content.idle_reset_seconds, 0);
        assert_eq!(c.input.touch_keyboard, TouchKeyboard::Off);
        let g = c.input.exit_gesture.expect("gesture");
        assert_eq!(g.taps, 5);
        assert_eq!(g.region, "top-right");
        assert_eq!(c.logging.url_detail, UrlDetail::Full);
        // Untouched sections still default.
        assert_eq!(c.network.config_poll_s, 300);
    }

    #[test]
    fn unknown_fields_are_captured_not_rejected() {
        let json = r#"{ "content": { "url": "https://a/", "future_knob": 7 },
                        "brand_new_section_ignored_by_serde": 1 }"#;
        let c: RemoteConfig = serde_json::from_str(json).expect("unknown fields must not reject");
        assert!(c.content.unknown.contains_key("future_knob"));
        assert!(c.unknown.contains_key("brand_new_section_ignored_by_serde"));
    }

    #[test]
    fn rejects_comments_strict_json_only() {
        // spec cfg-13: the on-device parser is strict JSON.
        let json = "{ /* nope */ \"version\": 1 }";
        assert!(serde_json::from_str::<RemoteConfig>(json).is_err());
    }

    #[test]
    fn bad_enum_value_is_a_parse_error() {
        let json = r#"{ "content": { "fallback": "carrier_pigeon" } }"#;
        assert!(serde_json::from_str::<RemoteConfig>(json).is_err());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p kiosk-core schema`
Expected: FAIL to compile — `cannot find type RemoteConfig in this scope`.

- [ ] **Step 3: Implement the schema**

Add to `crates/kiosk-core/src/config/schema.rs`, above the `#[cfg(test)]` module:

```rust
fn d_true() -> bool { true }
fn d_zoom() -> f64 { 1.0 }
fn d_version() -> u32 { 1 }
fn d_error_max_retries() -> u32 { 5 }
fn d_idle_reset() -> u64 { 180 }
fn d_cursor_autohide() -> u64 { 5 }
fn d_check_url() -> String { "https://www.gstatic.com/generate_204".to_string() }
fn d_probe_online() -> u64 { 30 }
fn d_probe_offline() -> u64 { 10 }
fn d_config_poll() -> u64 { 300 }
fn d_max_mem() -> u64 { 1500 }
fn d_level() -> String { "info".to_string() }
fn d_health_sample() -> u64 { 60 }
fn d_spool_max() -> u64 { 50 }
fn d_spool_reserve() -> u64 { 10 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fallback {
    Video,
    ErrorPage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchKeyboard {
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlDetail {
    Path,
    Host,
    Full,
}

/// Web-permission policy, default-deny (spec §7 M9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Permissions {
    #[serde(default)]
    pub camera: bool,
    #[serde(default)]
    pub microphone: bool,
    #[serde(default)]
    pub geolocation: bool,
    #[serde(default)]
    pub notifications: bool,
    #[serde(default)]
    pub clipboard_read: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Content {
    /// `None` ⇒ fall back to the `[bootstrap]` url (spec cfg-05).
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub allowlist: Vec<String>,
    #[serde(default)]
    pub frame_allowlist: Option<Vec<String>>,
    #[serde(default)]
    pub scheme_allowlist: Vec<String>,
    #[serde(default = "d_fallback")]
    pub fallback: Fallback,
    #[serde(default = "d_error_max_retries")]
    pub error_max_retries: u32,
    #[serde(default = "d_zoom")]
    pub zoom: f64,
    #[serde(default)]
    pub inject_css: String,
    #[serde(default)]
    pub inject_js: String,
    #[serde(default = "d_idle_reset")]
    pub idle_reset_seconds: u64,
    #[serde(default = "d_true")]
    pub clear_data_on_reset: bool,
    #[serde(default)]
    pub pdf_view: bool,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

fn d_fallback() -> Fallback { Fallback::Video }

impl Default for Content {
    fn default() -> Self {
        serde_json::from_str("{}").expect("Content defaults must deserialize")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Display {
    #[serde(default = "d_cursor_autohide")]
    pub cursor_autohide_seconds: u64,
    #[serde(default)]
    pub monitor: u32,
    #[serde(default = "d_true")]
    pub keep_awake: bool,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

impl Default for Display {
    fn default() -> Self {
        serde_json::from_str("{}").expect("Display defaults must deserialize")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExitGesture {
    pub taps: u8,
    pub region: String,
    #[serde(default)]
    pub min_len: Option<u8>,
    #[serde(default)]
    pub alphanumeric: bool,
    pub pin_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Input {
    #[serde(default = "d_touch_keyboard")]
    pub touch_keyboard: TouchKeyboard,
    #[serde(default)]
    pub allow_context_menu: bool,
    #[serde(default)]
    pub allow_text_selection: bool,
    #[serde(default)]
    pub exit_gesture: Option<ExitGesture>,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

fn d_touch_keyboard() -> TouchKeyboard { TouchKeyboard::Auto }

impl Default for Input {
    fn default() -> Self {
        serde_json::from_str("{}").expect("Input defaults must deserialize")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Network {
    #[serde(default = "d_check_url")]
    pub connectivity_check_url: String,
    #[serde(default = "d_probe_online")]
    pub probe_online_s: u64,
    #[serde(default = "d_probe_offline")]
    pub probe_offline_s: u64,
    #[serde(default = "d_config_poll")]
    pub config_poll_s: u64,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

impl Default for Network {
    fn default() -> Self {
        serde_json::from_str("{}").expect("Network defaults must deserialize")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Maintenance {
    #[serde(default)]
    pub nightly_reload: Option<String>,
    #[serde(default)]
    pub restart_app: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default = "d_max_mem")]
    pub max_webview_mem_mb: u64,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

impl Default for Maintenance {
    fn default() -> Self {
        serde_json::from_str("{}").expect("Maintenance defaults must deserialize")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Logging {
    #[serde(default = "d_level")]
    pub level: String,
    #[serde(default = "d_health_sample")]
    pub health_sample_s: u64,
    #[serde(default = "d_spool_max")]
    pub spool_max_mb: u64,
    #[serde(default = "d_spool_reserve")]
    pub spool_reserve_high_mb: u64,
    #[serde(default = "d_url_detail")]
    pub url_detail: UrlDetail,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

fn d_url_detail() -> UrlDetail { UrlDetail::Path }

impl Default for Logging {
    fn default() -> Self {
        serde_json::from_str("{}").expect("Logging defaults must deserialize")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteConfig {
    #[serde(default = "d_version")]
    pub version: u32,
    /// REQUIRED on every *fetched* config (spec §5.2); absent is legal only for
    /// locally-sourced config, which never passes through signature/rollback checks.
    #[serde(default)]
    pub revision: Option<i64>,
    #[serde(default)]
    pub sig: Option<String>,
    #[serde(default)]
    pub content: Content,
    #[serde(default)]
    pub display: Display,
    #[serde(default)]
    pub input: Input,
    #[serde(default)]
    pub network: Network,
    #[serde(default)]
    pub maintenance: Maintenance,
    #[serde(default)]
    pub logging: Logging,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        serde_json::from_str("{}").expect("RemoteConfig defaults must deserialize")
    }
}
```

Add `pub mod schema;` to `crates/kiosk-core/src/config/mod.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p kiosk-core schema`
Expected: 5 tests PASS.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/kiosk-core/
git commit -m "feat(core): remote config schema with defaults and unknown-field capture

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Whole-document validation — ranges, version, warnings

**Files:**
- Create: `crates/kiosk-core/src/config/validate.rs`
- Modify: `crates/kiosk-core/src/config/mod.rs` (add `pub mod validate;`)

**Interfaces:**
- Consumes: `schema::RemoteConfig` (Task 3), `error::{ConfigError, FieldError}` (Task 1).
- Produces:
  - `kiosk_core::config::validate::SCHEMA_MAJOR: u32 = 1`
  - `kiosk_core::config::validate::validate(cfg: &RemoteConfig) -> Result<Vec<String>, ConfigError>` — `Ok(warnings)` or `Err(ConfigError::Invalid{..} | ConfigError::UnsupportedVersion(..))`. Rejection is **whole-document**: all offending fields are collected into one error list.
  - `kiosk_core::config::validate::clamp_effective(cfg: &mut RemoteConfig)` — runtime clamps so a legacy last-good config can never disable polling (spec cfg-01).

Ranges (spec §5.2, verbatim): `zoom` [0.5, 3.0]; `cursor_autohide_seconds` [0, 3600]; `taps` [3, 10]; `probe_online_s` [5, 3600]; `probe_offline_s` [5, 3600]; `config_poll_s` [30, 3600] (0 is INVALID); `max_webview_mem_mb` {0} ∪ [256, 8192]; `health_sample_s` [10, 3600]; `spool_max_mb` [5, 1024] (0 rejected); `spool_reserve_high_mb` ≤ `spool_max_mb` (**clamped**, not rejected); `display.monitor` out-of-range **falls back to primary with a WARNING** rather than rejecting (display topology is device-local).

- [ ] **Step 1: Write the failing tests**

Create `crates/kiosk-core/src/config/validate.rs`:

```rust
//! Whole-document validation (spec §5.2 cfg-07/cfg-11/cfg-03, RT-08).
//! Any invalid value rejects the ENTIRE config — never a partial apply.

use crate::config::schema::RemoteConfig;
use crate::error::{ConfigError, FieldError};

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(json: &str) -> RemoteConfig {
        serde_json::from_str(json).expect("test json must parse")
    }

    #[test]
    fn empty_config_is_valid_with_no_warnings() {
        let warnings = validate(&cfg("{}")).expect("defaults are valid");
        assert!(warnings.is_empty(), "got {warnings:?}");
    }

    #[test]
    fn collects_every_out_of_range_field_in_one_rejection() {
        let c = cfg(r#"{
          "content": { "zoom": 9.0 },
          "network": { "probe_online_s": 1, "config_poll_s": 0 },
          "logging": { "spool_max_mb": 0, "health_sample_s": 1 }
        }"#);
        let err = validate(&c).expect_err("must reject");
        match err {
            ConfigError::Invalid { errors, .. } => {
                let f: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();
                assert!(f.contains(&"content.zoom"), "got {f:?}");
                assert!(f.contains(&"network.probe_online_s"), "got {f:?}");
                assert!(f.contains(&"network.config_poll_s"), "got {f:?}");
                assert!(f.contains(&"logging.spool_max_mb"), "got {f:?}");
                assert!(f.contains(&"logging.health_sample_s"), "got {f:?}");
                assert_eq!(errors.len(), 5, "whole-document: all errors at once");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn config_poll_zero_is_invalid_cannot_disable_the_only_remote_lever() {
        let err = validate(&cfg(r#"{"network":{"config_poll_s":0}}"#)).expect_err("must reject");
        match err {
            ConfigError::Invalid { errors, .. } => {
                assert_eq!(errors[0].field, "network.config_poll_s");
                assert_eq!(errors[0].value, "0");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn max_webview_mem_allows_zero_meaning_off_but_rejects_between() {
        assert!(validate(&cfg(r#"{"maintenance":{"max_webview_mem_mb":0}}"#)).is_ok());
        assert!(validate(&cfg(r#"{"maintenance":{"max_webview_mem_mb":256}}"#)).is_ok());
        assert!(validate(&cfg(r#"{"maintenance":{"max_webview_mem_mb":100}}"#)).is_err());
        assert!(validate(&cfg(r#"{"maintenance":{"max_webview_mem_mb":9000}}"#)).is_err());
    }

    #[test]
    fn exit_gesture_taps_range_is_enforced() {
        let bad = r#"{"input":{"exit_gesture":{"taps":99,"region":"top-left","pin_hash":"$h"}}}"#;
        let err = validate(&cfg(bad)).expect_err("must reject");
        match err {
            ConfigError::Invalid { errors, .. } => {
                assert_eq!(errors[0].field, "input.exit_gesture.taps");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn future_major_version_is_rejected_as_unsupported() {
        let err = validate(&cfg(r#"{"version":2}"#)).expect_err("must reject");
        match err {
            ConfigError::UnsupportedVersion(v) => assert_eq!(v, 2),
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    #[test]
    fn out_of_range_monitor_warns_instead_of_rejecting() {
        let warnings = validate(&cfg(r#"{"display":{"monitor":9}}"#))
            .expect("monitor must warn, not reject — display topology is device-local");
        assert!(
            warnings.iter().any(|w| w.contains("display.monitor")),
            "got {warnings:?}"
        );
    }

    #[test]
    fn unknown_fields_produce_warnings_not_errors() {
        let warnings = validate(&cfg(r#"{"content":{"future_knob":1},"nonsense":2}"#))
            .expect("unknown fields must not reject");
        assert!(warnings.iter().any(|w| w.contains("future_knob")), "got {warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("nonsense")), "got {warnings:?}");
    }

    #[test]
    fn unimplemented_features_warn_when_set_to_non_default() {
        // RT-08: knobs whose feature is not in this build's capability set must be
        // visible in telemetry, not silent no-ops.
        let warnings = validate(&cfg(r#"{"content":{"inject_js":"alert(1)"}}"#))
            .expect("accepted but unavailable");
        assert!(
            warnings.iter().any(|w| w.contains("content.inject_js")),
            "got {warnings:?}"
        );
    }

    #[test]
    fn spool_reserve_is_clamped_not_rejected() {
        let mut c = cfg(r#"{"logging":{"spool_max_mb":20,"spool_reserve_high_mb":50}}"#);
        let warnings = validate(&c).expect("clamped, not rejected");
        assert!(warnings.iter().any(|w| w.contains("spool_reserve_high_mb")));
        clamp_effective(&mut c);
        assert_eq!(c.logging.spool_reserve_high_mb, 20);
    }

    #[test]
    fn clamp_effective_restores_a_sane_poll_interval() {
        // A legacy last-good doc could carry an out-of-range value; the runtime clamp
        // guarantees polling can never be disabled (cfg-01).
        let mut c = cfg("{}");
        c.network.config_poll_s = 0;
        clamp_effective(&mut c);
        assert_eq!(c.network.config_poll_s, 30);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p kiosk-core validate`
Expected: FAIL to compile — `cannot find function validate in this scope`.

- [ ] **Step 3: Implement validation**

Add to `crates/kiosk-core/src/config/validate.rs`, above the `#[cfg(test)]` module:

```rust
/// The schema MAJOR version this build supports (spec cfg-03).
pub const SCHEMA_MAJOR: u32 = 1;

/// Config fields whose runtime feature is not implemented in this build.
/// Setting them to a non-default value is accepted but warned about (spec RT-08).
const UNIMPLEMENTED: &[(&str, &str)] = &[
    ("content.inject_css", "P2"),
    ("content.inject_js", "P2"),
    ("content.pdf_view", "P1"),
    ("maintenance.max_webview_mem_mb", "P2"),
    ("maintenance.restart_app", "P2"),
];

fn range_u64(
    value: u64,
    lo: u64,
    hi: u64,
    field: &str,
    errors: &mut Vec<FieldError>,
) {
    if value < lo || value > hi {
        errors.push(FieldError::new(
            field,
            format!("out of range [{lo}, {hi}]"),
            value.to_string(),
        ));
    }
}

/// Validate the whole document. `Ok(warnings)` or a rejection carrying EVERY
/// offending field (spec cfg-11: never a partial apply).
pub fn validate(cfg: &RemoteConfig) -> Result<Vec<String>, ConfigError> {
    if cfg.version > SCHEMA_MAJOR {
        return Err(ConfigError::UnsupportedVersion(cfg.version));
    }

    let mut errors: Vec<FieldError> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // content
    if !(0.5..=3.0).contains(&cfg.content.zoom) {
        errors.push(FieldError::new(
            "content.zoom",
            "out of range [0.5, 3.0]",
            cfg.content.zoom.to_string(),
        ));
    }

    // display — monitor is device-local: warn and fall back to primary, never reject.
    if cfg.display.monitor > 0 {
        warnings.push(format!(
            "display.monitor = {} — falls back to primary if that display is absent",
            cfg.display.monitor
        ));
    }
    range_u64(
        cfg.display.cursor_autohide_seconds,
        0,
        3600,
        "display.cursor_autohide_seconds",
        &mut errors,
    );

    // input
    if let Some(g) = &cfg.input.exit_gesture {
        if !(3..=10).contains(&g.taps) {
            errors.push(FieldError::new(
                "input.exit_gesture.taps",
                "out of range [3, 10]",
                g.taps.to_string(),
            ));
        }
        if g.pin_hash.trim().is_empty() {
            errors.push(FieldError::new(
                "input.exit_gesture.pin_hash",
                "must not be empty when an exit gesture is configured",
                "",
            ));
        }
    }

    // network
    range_u64(cfg.network.probe_online_s, 5, 3600, "network.probe_online_s", &mut errors);
    range_u64(cfg.network.probe_offline_s, 5, 3600, "network.probe_offline_s", &mut errors);
    // 0 is INVALID: polling is the only remote lever (cfg-01).
    range_u64(cfg.network.config_poll_s, 30, 3600, "network.config_poll_s", &mut errors);

    // maintenance — {0} ∪ [256, 8192]
    let mem = cfg.maintenance.max_webview_mem_mb;
    if mem != 0 && !(256..=8192).contains(&mem) {
        errors.push(FieldError::new(
            "maintenance.max_webview_mem_mb",
            "must be 0 (off) or within [256, 8192]",
            mem.to_string(),
        ));
    }

    // logging
    range_u64(cfg.logging.health_sample_s, 10, 3600, "logging.health_sample_s", &mut errors);
    range_u64(cfg.logging.spool_max_mb, 5, 1024, "logging.spool_max_mb", &mut errors);
    if cfg.logging.spool_reserve_high_mb > cfg.logging.spool_max_mb {
        warnings.push(format!(
            "logging.spool_reserve_high_mb ({}) > spool_max_mb ({}) — clamped",
            cfg.logging.spool_reserve_high_mb, cfg.logging.spool_max_mb
        ));
    }

    // unknown fields → warnings (spec §5.2: unknown fields warn, never reject)
    let unknowns: [(&str, &serde_json::Map<String, serde_json::Value>); 7] = [
        ("(root)", &cfg.unknown),
        ("content", &cfg.content.unknown),
        ("display", &cfg.display.unknown),
        ("input", &cfg.input.unknown),
        ("network", &cfg.network.unknown),
        ("maintenance", &cfg.maintenance.unknown),
        ("logging", &cfg.logging.unknown),
    ];
    for (section, map) in unknowns {
        for key in map.keys() {
            warnings.push(format!("unknown field {section}.{key} — ignored"));
        }
    }

    // capability set (RT-08)
    let defaults = RemoteConfig::default();
    for (field, phase) in UNIMPLEMENTED {
        let set = match *field {
            "content.inject_css" => cfg.content.inject_css != defaults.content.inject_css,
            "content.inject_js" => cfg.content.inject_js != defaults.content.inject_js,
            "content.pdf_view" => cfg.content.pdf_view != defaults.content.pdf_view,
            "maintenance.max_webview_mem_mb" => {
                cfg.maintenance.max_webview_mem_mb != defaults.maintenance.max_webview_mem_mb
            }
            "maintenance.restart_app" => {
                cfg.maintenance.restart_app != defaults.maintenance.restart_app
            }
            _ => false,
        };
        if set {
            warnings.push(format!(
                "field {field} accepted but feature unavailable in this build (introduced {phase})"
            ));
        }
    }

    if !errors.is_empty() {
        return Err(ConfigError::Invalid {
            errors,
            rejected_revision: cfg.revision,
        });
    }
    Ok(warnings)
}

/// Runtime clamps applied to the EFFECTIVE config, so a legacy last-good document can
/// never disable polling or over-reserve the spool (spec cfg-01).
pub fn clamp_effective(cfg: &mut RemoteConfig) {
    cfg.network.config_poll_s = cfg.network.config_poll_s.clamp(30, 3600);
    cfg.network.probe_online_s = cfg.network.probe_online_s.clamp(5, 3600);
    cfg.network.probe_offline_s = cfg.network.probe_offline_s.clamp(5, 3600);
    cfg.content.zoom = cfg.content.zoom.clamp(0.5, 3.0);
    if cfg.logging.spool_reserve_high_mb > cfg.logging.spool_max_mb {
        cfg.logging.spool_reserve_high_mb = cfg.logging.spool_max_mb;
    }
}
```

Add `pub mod validate;` to `crates/kiosk-core/src/config/mod.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p kiosk-core validate`
Expected: 11 tests PASS.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/kiosk-core/
git commit -m "feat(core): whole-document config validation with ranges and warnings

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Ed25519 signature verification over RFC 8785 JCS

**Files:**
- Create: `crates/kiosk-core/src/config/signature.rs`
- Modify: `crates/kiosk-core/src/config/mod.rs` (add `pub mod signature;`)

**Interfaces:**
- Consumes: `error::ConfigError` (Task 1).
- Produces:
  - `kiosk_core::config::signature::verify_signed(raw: &serde_json::Value, key: &VerifyingKey) -> Result<i64, ConfigError>` — returns the document's `revision` on success. Verifies the detached Ed25519 signature in `sig` over the **JCS canonicalization of the document with `sig` removed** (spec §8/SEC-11).
  - `kiosk_core::config::signature::pinned_key() -> Result<VerifyingKey, ConfigError>` — the compiled-in public key from `KIOSK_CONFIG_PUBKEY_B64` (build-time env). **Fails closed** when unset.
  - Re-export `pub use ed25519_dalek::VerifyingKey;`

**Signature format (spec §5.2):** `"sig": "ed25519:<base64-standard-of-64-byte-signature>"`.

**Why the key is a build-time env, not a constant:** the pinned key must NOT be co-located with the read credential and must be baked into the signed binary (spec §8). A release build sets `KIOSK_CONFIG_PUBKEY_B64`; a dev build without it rejects every fetched config, which is the correct fail-closed posture.

- [ ] **Step 1: Write the failing tests**

Create `crates/kiosk-core/src/config/signature.rs`:

```rust
//! Config integrity (spec §8/SEC-11): detached Ed25519 over the RFC 8785 (JCS)
//! canonicalization of the config object with `sig` removed, verified against a
//! pinned public key baked into the binary. GCS IAM is access control, not
//! authenticity — this is what stops a bucket-write attacker owning the fleet.

use crate::error::ConfigError;
use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier};
use serde_json::Value;

pub use ed25519_dalek::VerifyingKey;

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// Deterministic test key — no RNG needed, so tests are reproducible.
    fn test_keys() -> (SigningKey, VerifyingKey) {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let vk = sk.verifying_key();
        (sk, vk)
    }

    /// Sign a document the way the fleet signing tool must: JCS-canonicalize the
    /// object WITHOUT `sig`, sign those bytes, then insert `sig`.
    fn sign(doc: &Value, sk: &SigningKey) -> Value {
        let mut unsigned = doc.clone();
        unsigned.as_object_mut().unwrap().remove("sig");
        let canonical = serde_jcs::to_string(&unsigned).expect("jcs");
        let sig = sk.sign(canonical.as_bytes());
        let b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());
        let mut signed = unsigned;
        signed
            .as_object_mut()
            .unwrap()
            .insert("sig".to_string(), Value::String(format!("ed25519:{b64}")));
        signed
    }

    #[test]
    fn accepts_a_correctly_signed_document_and_returns_its_revision() {
        let (sk, vk) = test_keys();
        let doc = serde_json::json!({ "revision": 42, "content": { "url": "https://a/" } });
        let signed = sign(&doc, &sk);
        let rev = verify_signed(&signed, &vk).expect("must verify");
        assert_eq!(rev, 42);
    }

    #[test]
    fn signature_survives_key_reordering_that_is_jcs_canonicalization_working() {
        let (sk, vk) = test_keys();
        let doc = serde_json::json!({ "revision": 7, "content": { "url": "https://a/" } });
        let signed = sign(&doc, &sk);
        // Re-serialize through a BTreeMap-ish round trip to shuffle key order.
        let reordered: Value =
            serde_json::from_str(&serde_json::to_string(&signed).unwrap()).unwrap();
        assert_eq!(verify_signed(&reordered, &vk).unwrap(), 7);
    }

    #[test]
    fn rejects_a_tampered_body() {
        let (sk, vk) = test_keys();
        let doc = serde_json::json!({ "revision": 1, "content": { "url": "https://good/" } });
        let mut signed = sign(&doc, &sk);
        signed["content"]["url"] = Value::String("https://evil/".to_string());
        let err = verify_signed(&signed, &vk).expect_err("tampered body must fail");
        assert!(matches!(err, ConfigError::Signature(_)), "got {err:?}");
    }

    #[test]
    fn rejects_a_missing_signature() {
        let (_, vk) = test_keys();
        let doc = serde_json::json!({ "revision": 1 });
        let err = verify_signed(&doc, &vk).expect_err("unsigned must fail");
        assert!(matches!(err, ConfigError::Signature(_)));
    }

    #[test]
    fn rejects_a_signature_from_the_wrong_key() {
        let (sk, _) = test_keys();
        let other_vk = SigningKey::from_bytes(&[9u8; 32]).verifying_key();
        let doc = serde_json::json!({ "revision": 1 });
        let signed = sign(&doc, &sk);
        let err = verify_signed(&signed, &other_vk).expect_err("wrong key must fail");
        assert!(matches!(err, ConfigError::Signature(_)));
    }

    #[test]
    fn rejects_a_missing_revision_even_when_signed() {
        // spec §5.2: sig AND revision are both required on every fetched config.
        let (sk, vk) = test_keys();
        let doc = serde_json::json!({ "content": { "url": "https://a/" } });
        let signed = sign(&doc, &sk);
        let err = verify_signed(&signed, &vk).expect_err("missing revision must fail");
        assert!(matches!(err, ConfigError::Signature(_)), "got {err:?}");
    }

    #[test]
    fn rejects_a_malformed_sig_prefix() {
        let (sk, vk) = test_keys();
        let doc = serde_json::json!({ "revision": 1 });
        let mut signed = sign(&doc, &sk);
        signed["sig"] = Value::String("rsa:AAAA".to_string());
        let err = verify_signed(&signed, &vk).expect_err("wrong algorithm must fail");
        assert!(matches!(err, ConfigError::Signature(_)));
    }

    /// Operator helper: `cargo test -p kiosk-core print_signing_keypair -- --ignored --nocapture`
    /// prints a fresh keypair. The PUBLIC half goes into KIOSK_CONFIG_PUBKEY_B64 at
    /// build time; the PRIVATE half stays in the signing service, never on a device.
    #[test]
    #[ignore]
    fn print_signing_keypair() {
        let sk = SigningKey::from_bytes(&[42u8; 32]);
        let b64 = base64::engine::general_purpose::STANDARD;
        println!("PRIVATE (seed, keep secret): {}", b64.encode(sk.to_bytes()));
        println!(
            "KIOSK_CONFIG_PUBKEY_B64={}",
            b64.encode(sk.verifying_key().to_bytes())
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p kiosk-core signature`
Expected: FAIL to compile — `cannot find function verify_signed in this scope`.

- [ ] **Step 3: Implement**

Add to `crates/kiosk-core/src/config/signature.rs`, above the `#[cfg(test)]` module:

```rust
const SIG_PREFIX: &str = "ed25519:";

fn sig_err(msg: impl Into<String>) -> ConfigError {
    ConfigError::Signature(msg.into())
}

/// Verify the detached signature and return the document's `revision`.
/// Both `sig` and `revision` are REQUIRED on every fetched config (spec §5.2).
pub fn verify_signed(raw: &Value, key: &VerifyingKey) -> Result<i64, ConfigError> {
    let obj = raw
        .as_object()
        .ok_or_else(|| sig_err("config root is not a JSON object"))?;

    let sig_str = obj
        .get("sig")
        .and_then(Value::as_str)
        .ok_or_else(|| sig_err("missing required field `sig`"))?;

    let b64 = sig_str
        .strip_prefix(SIG_PREFIX)
        .ok_or_else(|| sig_err(format!("signature must start with `{SIG_PREFIX}`")))?;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| sig_err(format!("signature is not valid base64: {e}")))?;
    let sig_bytes: [u8; 64] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| sig_err(format!("signature must be 64 bytes, got {}", bytes.len())))?;
    let signature = Signature::from_bytes(&sig_bytes);

    // Canonicalize the document WITHOUT `sig` (RFC 8785 JCS).
    let mut unsigned = obj.clone();
    unsigned.remove("sig");
    let canonical = serde_jcs::to_string(&Value::Object(unsigned))
        .map_err(|e| sig_err(format!("JCS canonicalization failed: {e}")))?;

    key.verify(canonical.as_bytes(), &signature)
        .map_err(|_| sig_err("signature does not verify against the pinned key"))?;

    // A signed document without a revision cannot be anti-rollback checked, so it is
    // rejected here rather than silently applied.
    obj.get("revision")
        .and_then(Value::as_i64)
        .ok_or_else(|| sig_err("missing required field `revision`"))
}

/// The pinned public key, baked in at build time. Fails closed when absent: a build
/// with no pinned key rejects every fetched config (spec §8).
pub fn pinned_key() -> Result<VerifyingKey, ConfigError> {
    let b64 = option_env!("KIOSK_CONFIG_PUBKEY_B64").ok_or_else(|| {
        sig_err("no pinned public key compiled in (set KIOSK_CONFIG_PUBKEY_B64 at build)")
    })?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| sig_err(format!("pinned key is not valid base64: {e}")))?;
    let key_bytes: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| sig_err(format!("pinned key must be 32 bytes, got {}", bytes.len())))?;
    VerifyingKey::from_bytes(&key_bytes)
        .map_err(|e| sig_err(format!("pinned key is not a valid Ed25519 key: {e}")))
}
```

Add `pub mod signature;` to `crates/kiosk-core/src/config/mod.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p kiosk-core signature`
Expected: 7 tests PASS, 1 ignored (`print_signing_keypair`).

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/kiosk-core/
git commit -m "feat(core): Ed25519 config signature verification over RFC 8785 JCS

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: Last-known-good store + anti-rollback revision

**Files:**
- Create: `crates/kiosk-core/src/config/store.rs`
- Modify: `crates/kiosk-core/src/config/mod.rs` (add `pub mod store;`)

**Interfaces:**
- Consumes: `error::ConfigError` (Task 1).
- Produces:
  - `kiosk_core::config::store::ConfigStore::new(dir: impl Into<PathBuf>) -> ConfigStore`
  - `ConfigStore::load_last_good(&self) -> Option<StoredConfig>`
  - `ConfigStore::save_last_good(&self, raw: &serde_json::Value, revision: i64) -> Result<(), ConfigError>` — atomic (temp file + rename)
  - `ConfigStore::last_applied_revision(&self) -> i64` — `0` when nothing has ever been applied
  - `kiosk_core::config::store::StoredConfig { pub raw: serde_json::Value, pub revision: i64 }`

**Design (spec cfg-06):** exactly ONE artifact, `config-lastgood.json` = the most recent successfully-applied (hence valid, hence signature-verified) remote config, stored verbatim including its `sig` and `revision`. There is no separate "cache" file. The last-applied revision is read back from that document, so the two can never disagree.

- [ ] **Step 1: Write the failing tests**

Create `crates/kiosk-core/src/config/store.rs`:

```rust
//! Last-known-good config persistence (spec cfg-06). Exactly one artifact:
//! `config-lastgood.json`, the most recent successfully-applied remote config,
//! stored verbatim (sig + revision included) so it can be re-verified and so the
//! last-applied revision can never disagree with the stored document.

use crate::error::ConfigError;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const LAST_GOOD_FILE: &str = "config-lastgood.json";

#[derive(Debug, Clone, PartialEq)]
pub struct StoredConfig {
    pub raw: Value,
    pub revision: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(rev: i64) -> Value {
        serde_json::json!({ "revision": rev, "sig": "ed25519:AAAA", "content": { "url": "https://a/" } })
    }

    #[test]
    fn no_store_yields_no_last_good_and_revision_zero() {
        let dir = tempfile::tempdir().unwrap();
        let s = ConfigStore::new(dir.path());
        assert_eq!(s.load_last_good(), None);
        assert_eq!(s.last_applied_revision(), 0);
    }

    #[test]
    fn saves_and_reloads_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let s = ConfigStore::new(dir.path());
        s.save_last_good(&doc(42), 42).unwrap();

        let got = s.load_last_good().expect("must load");
        assert_eq!(got.revision, 42);
        // Verbatim: the signature must survive, or the document could never be re-verified.
        assert_eq!(got.raw["sig"], Value::String("ed25519:AAAA".into()));
        assert_eq!(got.raw["content"]["url"], Value::String("https://a/".into()));
        assert_eq!(s.last_applied_revision(), 42);
    }

    #[test]
    fn overwrite_advances_the_revision() {
        let dir = tempfile::tempdir().unwrap();
        let s = ConfigStore::new(dir.path());
        s.save_last_good(&doc(1), 1).unwrap();
        s.save_last_good(&doc(9), 9).unwrap();
        assert_eq!(s.last_applied_revision(), 9);
        assert_eq!(s.load_last_good().unwrap().revision, 9);
    }

    #[test]
    fn corrupt_store_is_treated_as_absent_not_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(LAST_GOOD_FILE), b"{ this is not json").unwrap();
        let s = ConfigStore::new(dir.path());
        assert_eq!(s.load_last_good(), None, "a corrupt file must not panic the kiosk");
        assert_eq!(s.last_applied_revision(), 0);
    }

    #[test]
    fn save_creates_the_directory_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("does").join("not").join("exist");
        let s = ConfigStore::new(&nested);
        s.save_last_good(&doc(3), 3).unwrap();
        assert_eq!(s.last_applied_revision(), 3);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p kiosk-core store`
Expected: FAIL to compile — `cannot find type ConfigStore in this scope`.

- [ ] **Step 3: Implement**

Add to `crates/kiosk-core/src/config/store.rs`, above the `#[cfg(test)]` module:

```rust
#[derive(Debug, Clone)]
pub struct ConfigStore {
    dir: PathBuf,
}

impl ConfigStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        ConfigStore { dir: dir.into() }
    }

    fn path(&self) -> PathBuf {
        self.dir.join(LAST_GOOD_FILE)
    }

    /// Load the last-known-good config. A missing OR corrupt file is treated as
    /// "none" — a kiosk must boot even with a damaged store.
    pub fn load_last_good(&self) -> Option<StoredConfig> {
        let text = std::fs::read_to_string(self.path()).ok()?;
        let raw: Value = serde_json::from_str(&text).ok()?;
        let revision = raw.get("revision").and_then(Value::as_i64)?;
        Some(StoredConfig { raw, revision })
    }

    /// The highest revision ever applied; `0` when nothing has been applied.
    /// Used for the anti-rollback check (spec §8/SEC-11).
    pub fn last_applied_revision(&self) -> i64 {
        self.load_last_good().map(|c| c.revision).unwrap_or(0)
    }

    /// Persist verbatim, atomically (temp file + rename) so a power cut mid-write
    /// cannot leave a half-written config that bricks the next boot.
    pub fn save_last_good(&self, raw: &Value, revision: i64) -> Result<(), ConfigError> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| ConfigError::Io(format!("create {}: {e}", self.dir.display())))?;

        let text = serde_json::to_string_pretty(raw)
            .map_err(|e| ConfigError::Io(format!("serialize config: {e}")))?;

        let tmp = self.dir.join(format!("{LAST_GOOD_FILE}.tmp"));
        std::fs::write(&tmp, text.as_bytes())
            .map_err(|e| ConfigError::Io(format!("write {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, self.path())
            .map_err(|e| ConfigError::Io(format!("rename into {}: {e}", self.path().display())))?;

        debug_assert_eq!(
            raw.get("revision").and_then(Value::as_i64),
            Some(revision),
            "stored document's revision must match the applied revision"
        );
        Ok(())
    }
}

/// Convenience for callers that hold a path rather than a store.
pub fn last_good_path(dir: &Path) -> PathBuf {
    dir.join(LAST_GOOD_FILE)
}
```

Add `pub mod store;` to `crates/kiosk-core/src/config/mod.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p kiosk-core store`
Expected: 5 tests PASS.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/kiosk-core/
git commit -m "feat(core): last-known-good config store with atomic writes

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: `ConfigManager` — boot + apply in the spec's validation order

**Files:**
- Modify: `crates/kiosk-core/src/config/mod.rs` (add the manager below the module declarations)

**Interfaces:**
- Consumes: `bootstrap::BootstrapConfig` (T1), `identity::{expand_device_id_template, is_opaque_guid}` (T2), `schema::RemoteConfig` (T3), `validate::{validate, clamp_effective}` (T4), `signature::{verify_signed, VerifyingKey}` (T5), `store::ConfigStore` (T6).
- Produces:
  - `kiosk_core::config::Source` enum: `LastGood | Bootstrap | Fetched`
  - `kiosk_core::config::Applied { config: RemoteConfig, revision: Option<i64>, warnings: Vec<String>, source: Source }`
  - `kiosk_core::config::ConfigManager::boot(bootstrap: BootstrapConfig, device_id: String, store: ConfigStore, key: Option<VerifyingKey>) -> (ConfigManager, Applied)`
  - `ConfigManager::apply_fetched(&mut self, body: &[u8]) -> Result<Applied, ConfigError>`
  - `ConfigManager::home_url(&self) -> String` — `content.url` (templated) or the bootstrap url
  - `ConfigManager::current(&self) -> &RemoteConfig`

**Order is non-negotiable (spec §5.2, §8):** parse (strict JSON) → **signature** → **anti-rollback** → **schema/ranges** → persist → adopt. A failure at any step returns the error and leaves `current` untouched (last-good survives).

**Boot (spec §5.2):** apply `config-lastgood.json` if present (used directly when the network is down), else the bootstrap url. Last-good is exempt from signature/rollback re-checking — it was verified when it was first applied — but it IS re-validated for schema/ranges and clamped, so a legacy document can never disable polling.

- [ ] **Step 1: Write the failing tests**

Append to `crates/kiosk-core/src/config/mod.rs`:

```rust
#[cfg(test)]
mod manager_tests {
    use super::*;
    use crate::config::signature::VerifyingKey;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::Value;

    fn keys() -> (SigningKey, VerifyingKey) {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let vk = sk.verifying_key();
        (sk, vk)
    }

    fn sign(doc: &Value, sk: &SigningKey) -> Vec<u8> {
        use base64::Engine as _;
        let mut unsigned = doc.clone();
        unsigned.as_object_mut().unwrap().remove("sig");
        let canonical = serde_jcs::to_string(&unsigned).unwrap();
        let sig = sk.sign(canonical.as_bytes());
        let b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());
        let mut signed = unsigned;
        signed
            .as_object_mut()
            .unwrap()
            .insert("sig".into(), Value::String(format!("ed25519:{b64}")));
        serde_json::to_vec(&signed).unwrap()
    }

    fn boot_config() -> bootstrap::BootstrapConfig {
        bootstrap::BootstrapConfig {
            config_url: "https://example.com/c.json".into(),
            device_id: Some("lobby-01".into()),
            site: "hq".into(),
            region: None,
            project_id: "proj".into(),
            credential: "cred.json".into(),
            startup_grace_s: 90,
            healthy_run_s: 120,
            channel_grace_s: 30,
            demo_mode: false,
            bootstrap_url: "https://boot.example.com/".into(),
            exit_gesture: None,
        }
    }

    fn manager(dir: &std::path::Path, key: Option<VerifyingKey>) -> (ConfigManager, Applied) {
        ConfigManager::boot(
            boot_config(),
            "lobby-01".to_string(),
            store::ConfigStore::new(dir),
            key,
        )
    }

    #[test]
    fn boots_on_bootstrap_url_when_no_last_good_exists() {
        let dir = tempfile::tempdir().unwrap();
        let (m, applied) = manager(dir.path(), None);
        assert_eq!(applied.source, Source::Bootstrap);
        assert_eq!(applied.revision, None);
        assert_eq!(m.home_url(), "https://boot.example.com/");
    }

    #[test]
    fn applies_a_valid_signed_config_and_persists_it() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));

        let body = sign(
            &serde_json::json!({
                "revision": 5,
                "content": { "url": "https://app.example.com/k?d={device_id}" }
            }),
            &sk,
        );
        let applied = m.apply_fetched(&body).expect("must apply");
        assert_eq!(applied.source, Source::Fetched);
        assert_eq!(applied.revision, Some(5));
        assert_eq!(m.home_url(), "https://app.example.com/k?d=lobby-01");

        // Persisted: a fresh manager on the same dir boots from last-good.
        let (m2, applied2) = manager(dir.path(), Some(vk));
        assert_eq!(applied2.source, Source::LastGood);
        assert_eq!(applied2.revision, Some(5));
        assert_eq!(m2.home_url(), "https://app.example.com/k?d=lobby-01");
    }

    #[test]
    fn rejects_an_unsigned_config_and_keeps_current() {
        let dir = tempfile::tempdir().unwrap();
        let (_, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));

        let body = serde_json::to_vec(
            &serde_json::json!({ "revision": 5, "content": { "url": "https://evil/" } }),
        )
        .unwrap();
        let err = m.apply_fetched(&body).expect_err("unsigned must be rejected");
        assert!(matches!(err, ConfigError::Signature(_)), "got {err:?}");
        assert_eq!(m.home_url(), "https://boot.example.com/", "current must survive");
    }

    #[test]
    fn rejects_a_replayed_or_stale_revision() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));

        let good = sign(&serde_json::json!({ "revision": 5, "content": {} }), &sk);
        m.apply_fetched(&good).expect("rev 5 applies");

        let stale = sign(&serde_json::json!({ "revision": 5, "content": {} }), &sk);
        let err = m.apply_fetched(&stale).expect_err("rev <= last must be rejected");
        match err {
            ConfigError::Rollback { got, last } => {
                assert_eq!(got, 5);
                assert_eq!(last, 5);
            }
            other => panic!("expected Rollback, got {other:?}"),
        }
    }

    #[test]
    fn rejects_out_of_range_values_whole_document_and_keeps_current() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));

        let body = sign(
            &serde_json::json!({
                "revision": 9,
                "content": { "url": "https://app/", "zoom": 99.0 },
                "network": { "config_poll_s": 0 }
            }),
            &sk,
        );
        let err = m.apply_fetched(&body).expect_err("out of range must be rejected");
        match err {
            ConfigError::Invalid { errors, rejected_revision } => {
                assert_eq!(rejected_revision, Some(9));
                assert!(errors.iter().any(|e| e.field == "content.zoom"));
                assert!(errors.iter().any(|e| e.field == "network.config_poll_s"));
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
        assert_eq!(m.home_url(), "https://boot.example.com/", "current must survive");
    }

    #[test]
    fn a_signed_empty_body_is_valid_and_falls_back_to_the_bootstrap_url() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));

        let body = sign(&serde_json::json!({ "revision": 1 }), &sk);
        let applied = m.apply_fetched(&body).expect("signed {} is schema-valid");
        assert_eq!(applied.revision, Some(1));
        assert_eq!(m.home_url(), "https://boot.example.com/", "content.url absent => bootstrap");
    }

    #[test]
    fn without_a_pinned_key_every_fetch_is_rejected_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, _) = keys();
        let (mut m, _) = manager(dir.path(), None); // no pinned key compiled in

        let body = sign(&serde_json::json!({ "revision": 1, "content": {} }), &sk);
        let err = m.apply_fetched(&body).expect_err("no key => reject");
        assert!(matches!(err, ConfigError::Signature(_)));
    }

    #[test]
    fn rejects_comments_in_the_fetched_body_strict_json() {
        let dir = tempfile::tempdir().unwrap();
        let (_, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));
        let err = m
            .apply_fetched(b"{ /* comment */ \"revision\": 1 }")
            .expect_err("comments are not strict JSON");
        assert!(matches!(err, ConfigError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn warns_when_device_id_template_resolves_to_an_opaque_guid() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let (mut m, _) = ConfigManager::boot(
            boot_config(),
            "6f9619ff-8b86-d011-b42d-00c04fc964ff".to_string(),
            store::ConfigStore::new(dir.path()),
            Some(vk),
        );
        let body = sign(
            &serde_json::json!({ "revision": 2, "content": { "url": "https://a/?d={device_id}" } }),
            &sk,
        );
        let applied = m.apply_fetched(&body).expect("applies");
        assert!(
            applied.warnings.iter().any(|w| w.contains("device_id")),
            "got {:?}",
            applied.warnings
        );
    }

    #[test]
    fn a_legacy_last_good_with_a_bad_poll_interval_is_clamped_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        // Simulate a stored doc from an older build carrying an out-of-range value.
        let stored = serde_json::json!({
            "revision": 3,
            "sig": "ed25519:AAAA",
            "network": { "config_poll_s": 1 }
        });
        store::ConfigStore::new(dir.path())
            .save_last_good(&stored, 3)
            .unwrap();

        let (m, applied) = manager(dir.path(), None);
        assert_eq!(applied.source, Source::LastGood);
        assert_eq!(
            m.current().network.config_poll_s,
            30,
            "clamped so polling can never be disabled (cfg-01)"
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p kiosk-core manager`
Expected: FAIL to compile — `cannot find type ConfigManager in this scope`.

- [ ] **Step 3: Implement the manager**

Rewrite `crates/kiosk-core/src/config/mod.rs` so the module declarations sit at the top, the manager below them, and the `manager_tests` module from Step 1 stays at the bottom:

```rust
//! Configuration subsystem (spec §5, §8).
//!
//! Validation order for a fetched config is fixed and non-negotiable:
//! parse (strict JSON) → signature → anti-rollback → schema/ranges → persist → adopt.
//! A failure at any step leaves the currently-running config untouched.

pub mod bootstrap;
pub mod schema;
pub mod signature;
pub mod store;
pub mod validate;

use crate::error::ConfigError;
use crate::identity::{expand_device_id_template, is_opaque_guid};
use schema::RemoteConfig;
use signature::VerifyingKey;

/// Where the currently-running config came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    LastGood,
    Bootstrap,
    Fetched,
}

/// The outcome of a boot or an apply — what the caller logs as
/// `config.applied` (spec §6).
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub config: RemoteConfig,
    pub revision: Option<i64>,
    pub warnings: Vec<String>,
    pub source: Source,
}

pub struct ConfigManager {
    bootstrap: bootstrap::BootstrapConfig,
    device_id: String,
    store: store::ConfigStore,
    /// `None` ⇒ no pinned key compiled in ⇒ every fetched config is rejected
    /// (fail closed, spec §8).
    key: Option<VerifyingKey>,
    current: RemoteConfig,
    revision: Option<i64>,
}

impl ConfigManager {
    /// BOOT (spec §5.2): apply `config-lastgood.json` if present — it is used directly
    /// when the network is down — else fall back to the bootstrap url. Last-good is NOT
    /// re-signature-checked (it was verified when first applied) but IS re-validated for
    /// schema/ranges and clamped, so a legacy document can never disable polling.
    pub fn boot(
        bootstrap: bootstrap::BootstrapConfig,
        device_id: String,
        store: store::ConfigStore,
        key: Option<VerifyingKey>,
    ) -> (ConfigManager, Applied) {
        let (mut config, revision, mut warnings, source) = match store.load_last_good() {
            Some(stored) => match serde_json::from_value::<RemoteConfig>(stored.raw) {
                Ok(cfg) => {
                    let warnings = validate::validate(&cfg).unwrap_or_else(|e| {
                        vec![format!("last-good failed re-validation, using it anyway: {e}")]
                    });
                    (cfg, Some(stored.revision), warnings, Source::LastGood)
                }
                Err(e) => (
                    RemoteConfig::default(),
                    None,
                    vec![format!("last-good is unreadable, falling back to bootstrap: {e}")],
                    Source::Bootstrap,
                ),
            },
            None => (RemoteConfig::default(), None, Vec::new(), Source::Bootstrap),
        };

        validate::clamp_effective(&mut config);
        warnings.extend(Self::identity_warnings(&config, &device_id));

        let m = ConfigManager {
            bootstrap,
            device_id,
            store,
            key,
            current: config.clone(),
            revision,
        };
        let applied = Applied { config, revision, warnings, source };
        (m, applied)
    }

    /// REFETCH (spec §5.2): validate a downloaded body in strict order and, only if every
    /// step passes, persist it as the new last-good and adopt it. On ANY failure the
    /// currently-running config is untouched.
    pub fn apply_fetched(&mut self, body: &[u8]) -> Result<Applied, ConfigError> {
        // 0. strict JSON (cfg-13 — comments are not accepted)
        let raw: serde_json::Value = serde_json::from_slice(body)
            .map_err(|e| ConfigError::Parse(format!("config is not strict JSON: {e}")))?;

        // 1. signature (returns the revision inside the signed payload)
        let key = self.key.as_ref().ok_or_else(|| {
            ConfigError::Signature(
                "no pinned public key compiled in — refusing every fetched config".to_string(),
            )
        })?;
        let revision = signature::verify_signed(&raw, key)?;

        // 2. anti-rollback
        let last = self.store.last_applied_revision();
        if revision <= last {
            return Err(ConfigError::Rollback { got: revision, last });
        }

        // 3. schema & ranges (whole-document)
        let config: RemoteConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ConfigError::Parse(format!("config does not match the schema: {e}")))?;
        let mut warnings = validate::validate(&config)?;

        // 4. persist, then adopt
        self.store.save_last_good(&raw, revision)?;

        let mut effective = config;
        validate::clamp_effective(&mut effective);
        warnings.extend(Self::identity_warnings(&effective, &self.device_id));

        self.current = effective.clone();
        self.revision = Some(revision);

        Ok(Applied {
            config: effective,
            revision: Some(revision),
            warnings,
            source: Source::Fetched,
        })
    }

    /// The URL to navigate to: `content.url` with `{device_id}` expanded, or the
    /// `[bootstrap]` url when the config supplies none (spec cfg-05).
    pub fn home_url(&self) -> String {
        match self.current.content.url.as_deref() {
            Some(url) => expand_device_id_template(url, &self.device_id),
            None => self.bootstrap.bootstrap_url.clone(),
        }
    }

    pub fn current(&self) -> &RemoteConfig {
        &self.current
    }

    pub fn revision(&self) -> Option<i64> {
        self.revision
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn bootstrap(&self) -> &bootstrap::BootstrapConfig {
        &self.bootstrap
    }

    /// cfg-09: templating with an opaque machine GUID makes fleet triage painful.
    fn identity_warnings(config: &RemoteConfig, device_id: &str) -> Vec<String> {
        let mut w = Vec::new();
        let uses_template = config
            .content
            .url
            .as_deref()
            .is_some_and(|u| u.contains("{device_id}"));
        if uses_template && is_opaque_guid(device_id) {
            w.push(format!(
                "content.url uses {{device_id}} but it resolves to an opaque GUID ({device_id}) \
                 — set [kiosk] device_id to a human-readable name"
            ));
        }
        w
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p kiosk-core manager`
Expected: 10 tests PASS.

- [ ] **Step 5: Run the whole suite and lint**

Run: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all `kiosk-core` tests pass (≈50 across the 7 tasks), plus the pre-existing `kiosk-main` CLI tests. Output pristine.

- [ ] **Step 6: Commit**

```bash
git add crates/kiosk-core/
git commit -m "feat(core): ConfigManager with signature, anti-rollback, and last-good fallback

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Plan self-review (run at write time)

**1. Spec coverage.** §5.1 `kiosk.ini` → T1. cfg-09 identity/templating → T2. §5.2 schema + defaults + strict JSON (cfg-13) → T3. cfg-07 ranges, cfg-11 whole-document, cfg-03 versioning, RT-08 capability warnings, cfg-01 clamps → T4. §8/SEC-11 Ed25519 + JCS + pinned key → T5. cfg-06 last-good single artifact + anti-rollback persistence → T6. §5.2 validation order, boot/refetch, cfg-05 bootstrap fallback → T7.

**Deliberately out of scope, with the plan that owns each:**
- HTTP fetch + GCS generation/`If-None-Match` (cfg-14) — needs an HTTP client; belongs with the telemetry/network plan.
- Post-apply reachability self-check + `config.reverted` (RT-04) — needs the connectivity prober and a real navigation; belongs with the `kiosk-main` state-machine plan.
- Machine-ID reading (Windows GUID / `/etc/machine-id` / Android ID) — platform code; belongs with the `kiosk-main` platform plan. This plan's `effective_device_id` takes it as a parameter, which is what keeps `kiosk-core` pure.
- `config.applied` / `config.error` / `config.warn` emission — needs the telemetry client (next plan). This plan returns `Applied { warnings }` and `ConfigError { errors, rejected_revision }` shaped exactly to be logged.

**2. Placeholder scan.** No TBD/TODO; every step carries complete code and an exact command with expected output.

**3. Type consistency.** `ConfigError`/`FieldError` (T1) are used unchanged in T2/T4/T5/T6/T7. `RemoteConfig` (T3) is the input to `validate`/`clamp_effective` (T4) and the payload of `Applied` (T7). `VerifyingKey` is re-exported from T5 and consumed by `ConfigManager::boot`/`apply_fetched` (T7). `ConfigStore` (T6) methods `load_last_good`/`save_last_good`/`last_applied_revision` are exactly the ones T7 calls. `expand_device_id_template`/`is_opaque_guid` (T2) are exactly the ones T7 calls.

## Follow-on plans (in order)

1. **P1-B — `kiosk-core` telemetry:** GCL REST client, RS256 JWT + OAuth token cache, trusted time from HTTP `Date`, `insertId` dedup, severity-tiered disk spool, batching + rate-limit/coalescing. Depends on this plan (credential path, `project_id`, `device_id`, `site`/`region` all come from `BootstrapConfig`).
2. **P1-C — connectivity prober FSM + navigation allowlist matcher** (both pure `kiosk-core`).
3. **P1-D — `kiosk-main` integration:** state machine, offline video, webview hardening. **Folds in the P0 gate verdict** — KEYBOARD=PARTIAL means Assigned Access / Shell Launcher is a hard deployment requirement and in-app hooks are defense-in-depth only; POINTER=PASS means the native corner-tap exit gesture is in. Also adds the gate's newly-surfaced items: disable the WebView2 right-click context menu; document that touch edge-swipes are OS-layer-only.
4. **P1-E — `kiosk-launcher` watchdog:** heartbeat, READY arming, liveness disambiguation, backoff, safe mode.
5. **P1-F — packaging:** WiX MSI (Authenticode-signed, credential ACL), §7.2 Windows OS-lockdown docs.
