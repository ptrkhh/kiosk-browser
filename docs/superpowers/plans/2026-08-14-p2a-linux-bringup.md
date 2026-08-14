# P2-A — Linux Bring-up Spine + Nav Guard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST:** Linux/Wayland. Tasks 1–4 and 7 are pure/host-tested (`cargo test`, any Linux host). Tasks 5, 6, 8 compile on Linux and are proven by the Task 9/10 smoke. **The smoke (Tasks 9–10) is the merge gate and is human-run in-session, not CI.**

**Goal:** `kiosk-main` boots on Linux/Wayland with the full P1 FSM spine live — nav guard, error/offline paths, profile-clear completion, renderer-crash recovery, telemetry spooling — verified headless under weston.

**Architecture:** Every Linux body mirrors the shape the reviewed `windows_impl` blocks already use: a `#[cfg(windows)] pub fn` + `#[cfg(not(windows))] pub fn` pair delegating to a private platform module. Decision logic is never re-derived — `should_block`, `kiosk_core::nav::decide`, `NavPolicy::decision_for` are called, not reimplemented. Shipped Tauri/wry APIs are used where they exist (`on_navigation`, `on_new_window`, `navigate`); the direct `webkit2gtk` dependency is justified by exactly four signals plus one callback.

**Tech Stack:** Rust 2021, Tauri 2.11.5, wry 0.55.1, webkit2gtk 2.0.2 (`v2_16`), GTK3, weston (headless), WebKitGTK 2.52.x.

**Spec:** `docs/superpowers/specs/2026-08-06-p2a-linux-bringup-design.md` (rev 3)

## Global Constraints

- **Windows must stay byte-unchanged in behavior.** Every change is either `#[cfg]`-gated, or pure logic whose Windows-visible result is identical and covered by an existing test.
- **Dependency:** `[target.'cfg(target_os = "linux")'.dependencies] webkit2gtk = { version = "2.0.2", features = ["v2_16"] }`. Caret requirement, not `=`. No new direct `glib`/`gio` deps — use the `webkit2gtk::glib` / `webkit2gtk::gio` re-exports.
- **No `v2_40`-gated symbol may be introduced.** In particular do not reintroduce `ResponsePolicyDecision::is_main_frame_main_resource`. The four signals used (`load-changed`, `load-failed`, `load-failed-with-tls-errors`, `web-process-terminated`) are stable since 2.20; `WebsiteDataManager::clear` is `v2_16`.
- **Do not write a `decide-policy` handler.** wry already installs it (`wry-0.55.1/src/webkitgtk/mod.rs:547-576`); `on_navigation` is its front door.
- **Signal handlers never panic and never block:** outward communication is `mpsc::Sender::try_send` and `Telemetry` only. No `unwrap()` on `uri()` (wry does at `mod.rs:476,479` — do not copy it).
- **GTK objects never leave the GTK main thread.** The `with_webview` closure body runs on it; only `Send + Clone` handles are captured.
- **Exactly one `navigation_handler`** is installed by P2-A, and no RESPONSE or download policy decision is subscribed. P2-B adds both and must re-derive the policy-cancellation filter in Task 6.
- **Clippy `-D warnings` on both platforms.** Remaining Linux dead code stays `cfg`-annotated, never `#[allow]`ed away.
- `WEBKIT_DISABLE_COMPOSITING_MODE=1` is permitted **in the smoke environment only**, never in shipped code or units.

## File Structure

| File | Responsibility after this plan |
|---|---|
| `crates/kiosk-main/Cargo.toml` | adds the Linux-only `webkit2gtk` dependency |
| `crates/kiosk-main/src/main.rs` | platform-conditional `APP_ORIGIN`/asset origin; Linux `resolve_data_dir`/`machine_id`; the two Linux builder lines (nav guard + popup take-over) |
| `crates/kiosk-main/src/nav_policy.rs` | `is_remote_origin` becomes a `(scheme, host)` match, **no `cfg`** |
| `crates/kiosk-main/src/nav.rs` | gains `#[cfg(not(windows))] mod linux_impl` — load lifecycle, failure latch, policy filter, READY pulse; `should_block` becomes `pub(crate)` |
| `crates/kiosk-main/src/recovery.rs` | gains `termination_label` + `#[cfg(not(windows))] mod linux_impl` |
| `crates/kiosk-main/src/clear.rs` | gains `#[cfg(not(windows))] mod linux_impl` with the real completion callback |
| `crates/kiosk-main/src/credential_acl.rs` | `#[cfg(not(windows))]` stub **replaced** by a `#[cfg(unix)]` mode check |
| `crates/kiosk-main/bundled/offline.html` | picks the mp4 URL from `location.protocol` |
| `crates/kiosk-main/examples/clear_probe.rs` | new — drives `clear::clear` under a compositor for smoke 6 |
| `packaging/smoke/` | new — weston-headless harness scripts + fixtures (scenarios 1–7) |

---

### Task 1: Platform-conditional origins and scheme-aware `is_remote_origin`

**Files:**
- Modify: `crates/kiosk-main/src/main.rs:45-52` (`APP_ORIGIN` + its doc comment), and the `kioskasset` origin literal used at `main.rs:997-998`
- Modify: `crates/kiosk-main/src/nav_policy.rs:233-243` (`is_remote_origin`)
- Test: `crates/kiosk-main/src/nav_policy.rs` (`mod tests`)

**Interfaces:**
- Produces: `const APP_ORIGIN: &str`, `const ASSET_ORIGIN: &str` (both `main.rs`, unchanged types); `pub fn is_remote_origin(url: &str) -> bool` (unchanged signature)
- Consumes: nothing

- [ ] **Step 1: Write the failing tests** in `nav_policy.rs`'s `mod tests`

```rust
#[test]
fn linux_app_origins_are_not_remote() {
    assert!(!is_remote_origin("tauri://localhost/splash.html"));
    assert!(!is_remote_origin("kioskasset://localhost/kiosk-offline.mp4"));
    assert!(!is_remote_origin("ipc://localhost/"));
}

#[test]
fn windows_app_origins_are_not_remote() {
    assert!(!is_remote_origin("http://tauri.localhost/splash.html"));
    assert!(!is_remote_origin("http://kioskasset.localhost/kiosk-offline.mp4"));
    assert!(!is_remote_origin("http://ipc.localhost/"));
}

/// The host is required on the custom schemes too, or `tauri://evil.test/` would
/// classify as our own origin and skip the guard entirely.
#[test]
fn a_custom_scheme_on_a_foreign_host_is_remote() {
    assert!(is_remote_origin("tauri://evil.test/"));
    assert!(is_remote_origin("kioskasset://evil.test/x"));
}

/// The smoke harness serves its home from `http://localhost:PORT`. Bare `localhost`
/// must never join the app-origin host set or the harness's own home page would stop
/// feeding the FSM.
#[test]
fn bare_localhost_over_http_is_remote() {
    assert!(is_remote_origin("http://localhost:8099/home.html"));
}

/// Unchanged from P1: parse failure is NOT a block. Failing closed here would newly
/// block unparseable URLs on Windows and invert `resource_allowed`'s hostless rule.
#[test]
fn unparseable_stays_not_remote() {
    assert!(!is_remote_origin("not a url"));
    assert!(!is_remote_origin("about:blank"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-main nav_policy::tests -- --nocapture`
Expected: FAIL — `linux_app_origins_are_not_remote` and `a_custom_scheme_on_a_foreign_host_is_remote` fail (the current host-only match calls every `*://localhost` app origin remote and every `tauri://evil.test` app-origin).

- [ ] **Step 3: Rewrite `is_remote_origin`**

```rust
/// App-origin (bundled pages / mp4) vs remote content. Single source of truth;
/// `nav::feeds_fsm` delegates here so the FSM-feed filter and the nav guard agree by
/// construction.
///
/// Matched on `(scheme, host)`, with **no `cfg`**: both platforms' spellings are
/// recognised everywhere, because a host-only match classifies Linux's
/// `tauri://localhost` as remote (bundled pages would self-block and the error page's
/// own load would feed the FSM), and a scheme-only match would let `tauri://evil.test/`
/// pass as app-origin. Never add bare `"localhost"` — the smoke harness's own
/// `http://localhost:PORT` home must stay remote.
pub fn is_remote_origin(url: &str) -> bool {
    let Ok(u) = tauri::Url::parse(url) else {
        // Parse failure → not remote, unchanged from P1. Failing closed here would
        // newly block unparseable URLs on Windows, break `nav.rs`'s classification and
        // invert `resource_allowed`'s inline/hostless rule.
        return false;
    };
    let Some(host) = u.host_str() else { return false };
    let app_origin = match u.scheme() {
        "tauri" | "kioskasset" | "ipc" => host == "localhost",
        "http" | "https" => matches!(
            host,
            "tauri.localhost" | "kioskasset.localhost" | "ipc.localhost"
        ),
        _ => false,
    };
    !app_origin
}
```

- [ ] **Step 4: Switch the origin constants to the compile-time form**

In `main.rs`, replace the `APP_ORIGIN` const and its doc comment (`main.rs:45-52`) with:

```rust
/// The app origin for bundled pages. Windows/`wry` cannot navigate the top-level frame
/// to a custom scheme, so Tauri serves bundled assets at an `http://` host there; on
/// Linux/WebKitGTK the origin is the literal custom scheme. Same compile-time switch
/// Tauri uses internally (`tauri-2.11.5/src/manager/mod.rs:340-345`,
/// `AppManager::tauri_protocol_url`).
const APP_ORIGIN: &str = if cfg!(windows) {
    "http://tauri.localhost"
} else {
    "tauri://localhost"
};

/// The `kioskasset` custom-scheme origin (the offline mp4), same switch as `APP_ORIGIN`.
const ASSET_ORIGIN: &str = if cfg!(windows) {
    "http://kioskasset.localhost"
} else {
    "kioskasset://localhost"
};
```

Then replace every hard-coded `http://kioskasset.localhost` string in `main.rs` with `ASSET_ORIGIN` (build the URL as `format!("{ASSET_ORIGIN}/kiosk-offline.mp4")`), and update the comment at `main.rs:997` that spells the derivation out.

- [ ] **Step 5: Run the full crate test suite**

Run: `cargo test -p kiosk-main`
Expected: PASS, including the pre-existing `nav.rs` tests that assert the Windows spellings (`app_origin_bundled_pages_do_not_feed_the_fsm`, `bundled_app_origin_pages_are_never_blocked`) — they must still pass unchanged, which is the proof that Windows behavior did not move.

- [ ] **Step 6: Commit**

```bash
git add crates/kiosk-main/src/main.rs crates/kiosk-main/src/nav_policy.rs
git commit -m "feat(linux): platform-conditional app origins, scheme-aware is_remote_origin"
```

---

### Task 2: Linux `resolve_data_dir` and `machine_id`

**Files:**
- Modify: `crates/kiosk-main/src/main.rs:433-441` (`resolve_data_dir`), `:478-482` (the `#[cfg(not(windows))] machine_id` stub)
- Test: `crates/kiosk-main/src/main.rs` (`mod tests`)

**Interfaces:**
- Produces: `fn resolve_data_dir() -> PathBuf` → `/var/lib/kiosk/` on Linux; `fn machine_id() -> Option<String>`; `fn parse_machine_id(raw: &str) -> Option<String>` (pure, host-tested)
- Consumes: nothing

> **Cross-spec constraint (ledger X5/C16):** P2-C's launcher gains its own Linux `resolve_data_dir` returning **the same** `/var/lib/kiosk/`. A mismatch silently kills the TEL-10 spool drain, because the launcher drains the `spool/main` partition kiosk-main writes. The literal below is the one P2-C must copy.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn machine_id_is_trimmed() {
    assert_eq!(
        parse_machine_id("2c4a1b6e8f9d4c3b8a7e6f5d4c3b2a19\n"),
        Some("2c4a1b6e8f9d4c3b8a7e6f5d4c3b2a19".to_string())
    );
}

/// An empty or whitespace-only `/etc/machine-id` degrades exactly as the Windows
/// no-MachineGuid path does: `None`, no panic, boot continues with the fallback id.
#[test]
fn an_empty_machine_id_file_degrades_to_none() {
    assert_eq!(parse_machine_id(""), None);
    assert_eq!(parse_machine_id("   \n"), None);
}

#[cfg(not(windows))]
#[test]
fn the_linux_data_dir_is_var_lib_kiosk() {
    assert_eq!(resolve_data_dir(), std::path::PathBuf::from("/var/lib/kiosk"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-main machine_id`
Expected: FAIL with "cannot find function `parse_machine_id`".

- [ ] **Step 3: Implement**

Replace the `resolve_data_dir` body with a `cfg`-split, and the `machine_id` stub with the `/etc/machine-id` read:

```rust
/// The data dir (cache, spool, last-good) — `%ProgramData%\kiosk\` on Windows,
/// `/var/lib/kiosk/` on Linux (spec §4). Never operator-overridden (unlike the install
/// dir): this is not something a `kiosk.ini` deployment ever needs to relocate.
///
/// The launcher's `resolve_data_dir` must return the identical path — it drains the
/// `spool/main` partition written here (P2-C C16).
#[cfg(windows)]
fn resolve_data_dir() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("kiosk")
}

#[cfg(not(windows))]
fn resolve_data_dir() -> PathBuf {
    PathBuf::from("/var/lib/kiosk")
}

/// Pure, host-tested: the `/etc/machine-id` contents → a device id, or `None` when the
/// file is empty/whitespace. Split out of `machine_id` so the trimming rule is testable
/// without an `/etc` fixture.
#[cfg(not(windows))]
fn parse_machine_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// systemd's `/etc/machine-id` (spec §4). Absent or unreadable degrades exactly as the
/// Windows missing-MachineGuid path does — `None`, no panic, boot continues.
#[cfg(not(windows))]
fn machine_id() -> Option<String> {
    parse_machine_id(&std::fs::read_to_string("/etc/machine-id").ok()?)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kiosk-main machine_id && cargo test -p kiosk-main data_dir`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-main/src/main.rs
git commit -m "feat(linux): /var/lib/kiosk data dir and /etc/machine-id device id"
```

---

### Task 3: SEC-09 credential-at-rest check on Unix (C12)

**Files:**
- Modify: `crates/kiosk-main/src/credential_acl.rs:99-104` — **replace** the `#[cfg(not(windows))]` stub, and rewrite the module doc's "the kiosk target is Windows x64 only" premise
- Test: `crates/kiosk-main/src/credential_acl.rs` (`mod tests`)

**Interfaces:**
- Produces: `#[cfg(unix)] pub fn credential_is_owner_only(path: &Path) -> io::Result<bool>`
- Consumes: nothing (call sites `boot.rs:165` and `fetch.rs:100` are already wired and unchanged)

> Do **not** add a `#[cfg(unix)]` function beside the `#[cfg(not(windows))]` stub — both match on Linux and you get a duplicate definition. Replace it.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(unix)]
mod unix_mode_tests {
    use super::credential_is_owner_only;
    use std::os::unix::fs::PermissionsExt;

    fn write_with_mode(name: &str, mode: u32) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, b"{}").expect("fixture write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .expect("fixture chmod");
        path
    }

    #[test]
    fn owner_only_0600_passes() {
        let p = write_with_mode("kiosk-acl-0600.json", 0o600);
        assert_eq!(credential_is_owner_only(&p).unwrap(), true);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn group_or_world_readable_fails() {
        for (name, mode) in [
            ("kiosk-acl-0640.json", 0o640),
            ("kiosk-acl-0604.json", 0o604),
            ("kiosk-acl-0666.json", 0o666),
        ] {
            let p = write_with_mode(name, mode);
            assert_eq!(credential_is_owner_only(&p).unwrap(), false, "mode {mode:o}");
            let _ = std::fs::remove_file(p);
        }
    }

    /// A missing file is an `Err`, which `is_violation` already treats as a violation —
    /// fail-closed comes free, exactly as on Windows.
    #[test]
    fn a_missing_file_is_an_error_not_a_pass() {
        let p = std::env::temp_dir().join("kiosk-acl-does-not-exist.json");
        let _ = std::fs::remove_file(&p);
        assert!(credential_is_owner_only(&p).is_err());
    }

    #[test]
    fn the_error_is_a_violation() {
        let p = std::env::temp_dir().join("kiosk-acl-missing-2.json");
        let _ = std::fs::remove_file(&p);
        assert!(super::super::is_violation(&credential_is_owner_only(&p)));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-main credential_acl`
Expected: FAIL — `group_or_world_readable_fails` and `a_missing_file_is_an_error_not_a_pass` fail, because the stub returns `Ok(true)` unconditionally.

- [ ] **Step 3: Replace the stub**

```rust
/// SEC-09 on Unix: the credential must not be group- or world-accessible. Mode bits
/// only, no uid check — a root-owned `0o600` file is the deployment shape (P2-G G16).
///
/// ponytail: mode bits only; add an owner check if a non-root service user lands.
///
/// A missing or unreadable file yields `Err`, which `is_violation` already treats as a
/// violation — fail-closed, exactly as on Windows.
#[cfg(unix)]
pub fn credential_is_owner_only(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o077 == 0)
}
```

Rewrite the module doc comment (`credential_acl.rs:27-30`): the sentence "the kiosk target is Windows x64 only" is false as of this commit. Replace with a one-line note that Windows reads the DACL and Unix reads the mode bits, both feeding the same `is_violation` gate.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kiosk-main credential_acl`
Expected: PASS (4 new tests + the existing `kiosk-core::acl` tests untouched).

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-main/src/credential_acl.rs
git commit -m "fix(linux): SEC-09 credential mode check, replacing the fail-open stub"
```

---

### Task 4: Renderer-crash recovery on Linux

**Files:**
- Modify: `crates/kiosk-main/Cargo.toml` (add the Linux `webkit2gtk` dependency)
- Modify: `crates/kiosk-main/src/recovery.rs` (add `termination_label`, add `#[cfg(not(windows))] mod linux_impl`, point the `#[cfg(not(windows))] pub fn install` at it)
- Test: `crates/kiosk-main/src/recovery.rs` (`mod tests`)

**Interfaces:**
- Consumes: `Telemetry::webview_crash`, `SharedNavPolicy` (both already exist and are used by `windows_impl`)
- Produces: `#[cfg(not(windows))] fn termination_label(reason: WebProcessTerminationReason) -> &'static str`

> **Never** call `kind_label` or `recovery_action` with a WebKit reason. They take a raw `i32` in the WebView2 constant space: `Crashed = 0` would label as `browser_process_exited`, and `recovery_action(2)` would return `Reload` for a dead process.

- [ ] **Step 1: Add the dependency**

In `crates/kiosk-main/Cargo.toml`, after the `[target.'cfg(windows)'.dependencies]` block:

```toml
# Linux/WebKitGTK. Justified by exactly four signals plus one callback that have no wry
# or Tauri route: load-changed, load-failed, load-failed-with-tls-errors,
# web-process-terminated, and WebsiteDataManagerExtManual::clear's completion callback.
# Version is the one already in our lock via wry; Cargo unifies semver-compatible
# requirements to one crate, so `PlatformWebview::inner()`'s type and ours cannot
# diverge. `v2_16` is what `clear` needs; features are cumulative. glib/gio arrive as
# `webkit2gtk::glib` / `webkit2gtk::gio` re-exports — no new direct deps.
[target.'cfg(target_os = "linux")'.dependencies]
webkit2gtk = { version = "2.0.2", features = ["v2_16"] }
```

- [ ] **Step 2: Write the failing test**

```rust
#[cfg(not(windows))]
#[test]
fn every_webkit_termination_reason_has_a_label() {
    use webkit2gtk::WebProcessTerminationReason as R;
    assert_eq!(termination_label(R::Crashed), "webkit_crashed");
    assert_eq!(termination_label(R::ExceededMemoryLimit), "webkit_exceeded_memory_limit");
    assert_eq!(termination_label(R::TerminatedByApi), "webkit_terminated_by_api");
    assert_eq!(termination_label(R::__Unknown(99)), "webkit_unrecognized");
}

/// The two constant spaces must never be crossed: WebKit's `Crashed` is 0, which
/// `kind_label` would render as WebView2's `browser_process_exited`.
#[cfg(not(windows))]
#[test]
fn webkit_labels_never_collide_with_the_webview2_space() {
    use webkit2gtk::WebProcessTerminationReason as R;
    assert_ne!(termination_label(R::Crashed), kind_label(0));
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p kiosk-main recovery`
Expected: FAIL with "cannot find function `termination_label`".

- [ ] **Step 4: Implement the label and the Linux install**

```rust
/// A stable, greppable label for the `webview.crash` telemetry `kind` field, in the
/// **WebKit** reason space. Deliberately prefixed `webkit_` and deliberately NOT routed
/// through `kind_label`, whose `i32`s are WebView2 constants with different meanings.
#[cfg(not(windows))]
fn termination_label(reason: webkit2gtk::WebProcessTerminationReason) -> &'static str {
    use webkit2gtk::WebProcessTerminationReason as R;
    match reason {
        R::Crashed => "webkit_crashed",
        R::ExceededMemoryLimit => "webkit_exceeded_memory_limit",
        R::TerminatedByApi => "webkit_terminated_by_api",
        _ => "webkit_unrecognized",
    }
}

#[cfg(not(windows))]
pub fn install(window: &tauri::WebviewWindow, telem: Telemetry, nav_policy: SharedNavPolicy) {
    linux_impl::install(window, telem, nav_policy);
}

#[cfg(not(windows))]
mod linux_impl {
    use webkit2gtk::WebViewExt;

    use crate::nav_policy::SharedNavPolicy;
    use crate::telemetry::Telemetry;

    /// All three WebKit termination reasons mean the web process is gone, so all three
    /// take `NavigateHome`. There is no `Reload` branch: Windows reserves it for
    /// `RENDER_PROCESS_UNRESPONSIVE`, which has no WebKitGTK analogue (hang detection on
    /// Linux is the JS ping, P2-C C17).
    pub fn install(window: &tauri::WebviewWindow, telem: Telemetry, nav_policy: SharedNavPolicy) {
        let window_handle = window.clone();
        let result = window.with_webview(move |platform_webview| {
            let webview = platform_webview.inner();
            webview.connect_web_process_terminated(move |_wv, reason| {
                let label = super::termination_label(reason);
                telem.webview_crash(label);
                let home = nav_policy.load().home().to_string();
                match tauri::Url::parse(&home) {
                    Ok(url) => {
                        if let Err(e) = window_handle.navigate(url) {
                            eprintln!("recovery: navigate home failed after {label}: {e}");
                        }
                    }
                    Err(e) => eprintln!("recovery: home URL unparseable ({home}): {e}"),
                }
            });
        });
        if let Err(e) = result {
            eprintln!("recovery: with_webview failed, renderer crash will never be recovered: {e}");
        }
    }
}
```

> Read `windows_impl::install` first and mirror its exact `Telemetry`/`SharedNavPolicy` call shape — including how it reads `home` from the policy. If the accessor is named something other than `home()`, use the name `windows_impl` uses; do not add a new accessor.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p kiosk-main recovery && cargo clippy -p kiosk-main --all-targets -- -D warnings`
Expected: PASS, clippy clean.

- [ ] **Step 6: Commit**

```bash
git add crates/kiosk-main/Cargo.toml crates/kiosk-main/src/recovery.rs
git commit -m "feat(linux): web-process-terminated crash recovery with a WebKit reason label"
```

---

### Task 5: Nav guard and popup take-over on the builder

**Files:**
- Modify: `crates/kiosk-main/src/nav.rs:56` — `fn should_block` becomes `pub(crate) fn should_block`
- Modify: `crates/kiosk-main/src/main.rs:1014-1049` — two `#[cfg(not(windows))]` builder lines
- Modify: `crates/kiosk-main/src/scheme_guard.rs` — the Linux stub's message becomes the documented "covered by the nav guard" no-op
- Modify: `crates/kiosk-main/src/telemetry.rs:120-121` — doc comment only
- Test: `crates/kiosk-main/src/nav.rs` (`mod tests`)

**Interfaces:**
- Consumes: `should_block(&NavPolicy, &str, bool) -> Option<BlockReason>` (Task 5 widens its visibility), `Telemetry::nav_blocked`
- Produces: the installed `navigation_handler` — the invariant Task 6's policy filter depends on

- [ ] **Step 1: Write the failing test** (in `nav.rs`'s `mod tests`) pinning the Linux all-frames decision

```rust
/// Linux enforces the guard on ALL frames — the deliberate divergence from Windows,
/// where sub-frames are waved past because `egress.rs` catches them. This test pins the
/// argument the Linux builder line passes, so a later edit cannot quietly flip it.
#[test]
fn the_guard_blocks_an_off_allowlist_sub_frame_when_told_it_is_in_scope() {
    let p = policy(&["https://home.test/*"], "https://home.test/app");
    assert_eq!(
        should_block(&p, "https://evil.test/frame", true),
        Some(BlockReason::NotAllowlisted)
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kiosk-main nav::tests`
Expected: FAIL to compile if `should_block` is still private to the module's test scope after the visibility change is made in the wrong direction; otherwise PASS immediately — this test is a pin, and the real gate is smoke scenarios 2 and 5. Proceed either way once the assertion is present.

- [ ] **Step 3: Widen `should_block` and wire the builder**

`nav.rs`: `fn should_block` → `pub(crate) fn should_block`. Windows' `windows_impl` keeps calling it through `super::`.

`main.rs`, immediately before `let window = builder.build()?;`:

```rust
// Linux nav guard (P2-A): wry already installs the `decide-policy` handler this drives
// (`wry-0.55.1/src/webkitgtk/mod.rs:547-576`) — NavigationAction only, every frame,
// correct return value. Do NOT hand-write a `decide-policy` handler.
//
// The `true` third argument is a deliberate Linux decision — enforce on ALL frames —
// not a transfer of the Windows justification. A blocked sub-frame therefore reports
// `nav.blocked{reason: "not_allowlisted"}` where Windows reports `"egress"`.
#[cfg(not(windows))]
{
    let guard_policy = nav_policy.clone();
    let guard_telem = telem.clone();
    builder = builder.on_navigation(move |url| {
        match nav::should_block(&guard_policy.load(), url.as_str(), true) {
            Some(reason) => {
                guard_telem.nav_blocked(reason.as_str(), url.as_str());
                false
            }
            None => true,
        }
    });
    // §7: "new windows navigate in place". Hand the URL back to the main webview and
    // THEN deny: `navigate` is a dispatcher-proxied non-blocking send, safe from the
    // event-loop thread, and the resulting load re-enters `on_navigation` above and
    // faces the same guard — exactly Windows' `SetHandled(true)` + `Navigate`. Deny
    // explicitly rather than relying on wry not connecting `connect_create`.
    let popup_handle = app.handle().clone();
    builder = builder.on_new_window(move |url, _features| {
        if let Some(w) = popup_handle.get_webview_window(WINDOW_LABEL) {
            let _ = w.navigate(url);
        }
        tauri::webview::NewWindowResponse::Deny
    });
}
```

> Bind `nav_policy`/`telem` clones **before** the builder chain; both are `Send + Clone`, which is what the `Fn(..) + Send + 'static` bounds require.

`scheme_guard.rs`: rewrite the `#[cfg(not(windows))]` stub's `eprintln!` into a doc comment stating that scheme-allowlist enforcement rides the nav guard on Linux (`kiosk_core::nav::decide` already covers schemes), and that downloads/PDF are P2-B. The function body becomes an empty no-op.

`telemetry.rs:120-121`: amend the doc comment to "A navigation was cancelled by the native guard (main-frame on Windows; any frame on Linux — see `nav.rs`)."

- [ ] **Step 4: Verify it compiles and the suite is green**

Run: `cargo test -p kiosk-main && cargo clippy -p kiosk-main --all-targets -- -D warnings`
Expected: PASS. Behavior proof is smoke scenarios 2 and 5 (Task 9).

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-main/src/nav.rs crates/kiosk-main/src/main.rs \
        crates/kiosk-main/src/scheme_guard.rs crates/kiosk-main/src/telemetry.rs
git commit -m "feat(linux): nav guard on all frames, popups navigate in place"
```

---

### Task 6: Load lifecycle — `nav.rs` Linux body

**Files:**
- Modify: `crates/kiosk-main/src/nav.rs` — replace the `#[cfg(not(windows))] pub fn install` stub body with a delegation, add `#[cfg(not(windows))] mod linux_impl`
- Test: smoke scenarios 1, 3, 5 (Task 9) — the assertions here are behavioral, not unit-testable

**Interfaces:**
- Consumes: `feeds_fsm`, `should_block` (Task 5), `AppEvent::{NavigationCommitted, NavigationFailed}`, `Telemetry::nav_error`, `Arc<Notify>` ready pulse
- Produces: the installed lifecycle handlers. **P2-B must re-derive the policy-cancellation filter** below when it adds RESPONSE/download decisions, or it will start swallowing real `NavigationFailed`s.

Four rules this body exists to satisfy, each load-bearing:

1. **READY and `NavigationCommitted` come from `load-changed(FINISHED)`, never `COMMITTED`.** `Committed` fires on first response, so mapping it emits `NavigationCommitted` then `NavigationFailed` for one navigation, which resets the retry counter and means `error_max_retries` is never reached and the kiosk never falls through to the offline video.
2. **Failure latch cleared on `Started`, not on the suppressed `FINISHED`.** A `load-failed` with no following `FINISHED` must not arm the latch across navigations.
3. **Policy cancellations return immediately** — no `AppEvent`, no telemetry, no latch change. The guard already emitted the single `nav.blocked`.
4. **READY pulses on the first successful load of ANY origin,** including the bundled offline page — before the `feeds_fsm` filter, which scopes the FSM only.

- [ ] **Step 1: Write the implementation**

```rust
#[cfg(not(windows))]
pub fn install(
    window: &tauri::WebviewWindow,
    tx: mpsc::Sender<AppEvent>,
    telem: Telemetry,
    nav_policy: SharedNavPolicy,
    ready: std::sync::Arc<tokio::sync::Notify>,
) {
    linux_impl::install(window, tx, telem, nav_policy, ready);
}

#[cfg(not(windows))]
mod linux_impl {
    use std::cell::Cell;
    use std::rc::Rc;

    use kiosk_core::app::state::Event as AppEvent;
    use tokio::sync::mpsc;
    use webkit2gtk::{LoadEvent, PolicyError, WebViewExt};

    use crate::nav_policy::SharedNavPolicy;
    use crate::telemetry::Telemetry;

    /// A `load-failed` raised by our own guard's cancellation. Dropped statelessly: the
    /// guard already emitted the single `nav.blocked`, matching Windows'
    /// one-event-per-blocked-navigation.
    ///
    /// **Invariant:** this filter assumes exactly one `navigation_handler` is installed
    /// and that no RESPONSE or download decision is subscribed. P2-B adds both, and they
    /// raise the same error code from a different cause — P2-B must re-derive this.
    fn is_policy_cancellation(err: &webkit2gtk::glib::Error) -> bool {
        matches!(
            err.kind::<PolicyError>(),
            Some(PolicyError::FrameLoadInterruptedByPolicyChange)
        )
    }

    pub fn install(
        window: &tauri::WebviewWindow,
        tx: mpsc::Sender<AppEvent>,
        telem: Telemetry,
        _nav_policy: SharedNavPolicy,
        ready: std::sync::Arc<tokio::sync::Notify>,
    ) {
        let result = window.with_webview(move |platform_webview| {
            let webview = platform_webview.inner();

            // One latch, GTK-main-thread-only, so `Rc<Cell<..>>` is sufficient.
            let failed = Rc::new(Cell::new(false));
            let ready_latch = Rc::new(Cell::new(false));

            let failed_changed = failed.clone();
            let tx_changed = tx.clone();
            webview.connect_load_changed(move |wv, event| {
                match event {
                    // The only per-load boundary WebKit gives us. Clearing here rather
                    // than on the suppressed FINISHED is load-bearing: a load-failed with
                    // no following FINISHED must not arm the latch across navigations,
                    // which would swallow the next successful load's commit and park the
                    // kiosk on the offline video with a healthy network.
                    LoadEvent::Started => failed_changed.set(false),
                    LoadEvent::Finished => {
                        if failed_changed.get() {
                            return;
                        }
                        // READY pulses on the first successful load of ANY origin,
                        // including the bundled offline page: the watchdog asks "is the
                        // app alive and rendering", not "is the site reachable". Must run
                        // BEFORE the feeds_fsm filter.
                        if !ready_latch.replace(true) {
                            ready.notify_one();
                        }
                        // `uri()` is read at signal time and is post-redirect (the Windows
                        // navId→uri map is pre-redirect); both classify identically for
                        // `is_remote_origin`, since a redirect cannot cross into our own
                        // registered schemes. `None` classifies as not-remote — never
                        // unwrap in a signal handler.
                        let Some(uri) = wv.uri() else { return };
                        if !super::feeds_fsm(uri.as_str()) {
                            return;
                        }
                        let _ = tx_changed.try_send(AppEvent::NavigationCommitted);
                    }
                    _ => {}
                }
            });

            let failed_load = failed.clone();
            let tx_load = tx.clone();
            let telem_load = telem.clone();
            webview.connect_load_failed(move |_wv, _event, failing_uri, err| {
                if is_policy_cancellation(err) {
                    // No AppEvent, no telemetry, no latch change.
                    return true;
                }
                failed_load.set(true);
                // Both the FSM event and the telemetry sit inside the remote-origin
                // filter, mirroring Windows where `nav_error` sits after the `feeds_fsm`
                // early return. App-origin load failures stay silent.
                if super::feeds_fsm(failing_uri) {
                    telem_load.nav_error(&err.to_string());
                    let _ = tx_load.try_send(AppEvent::NavigationFailed);
                }
                true
            });

            let failed_tls = failed;
            let tx_tls = tx;
            let telem_tls = telem;
            webview.connect_load_failed_with_tls_errors(move |_wv, failing_uri, _cert, _errors| {
                failed_tls.set(true);
                if super::feeds_fsm(failing_uri) {
                    telem_tls.nav_error("tls_error");
                    let _ = tx_tls.try_send(AppEvent::NavigationFailed);
                }
                true
            });
        });
        if let Err(e) = result {
            eprintln!("nav: with_webview failed, navigation outcome will never be observed: {e}");
        }
    }
}
```

> Check each signal's exact closure signature against `webkit2gtk-2.0.2/src/auto/web_view.rs` (`:2287`, `:2316`, `:2355`) before compiling and adjust argument names/types to match; the bodies above are what must not change.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p kiosk-main && cargo clippy -p kiosk-main --all-targets -- -D warnings`
Expected: builds clean. Any `Rc`-across-closures error means a `.clone()` was dropped — add it, do not reach for `Arc`/`Mutex` (these handlers are GTK-main-thread-only by construction).

- [ ] **Step 3: Record the one remaining assumption**

Add to the module doc of `linux_impl`: `load-changed`/`load-failed` are `WebKitWebView`-level signals tracking the **main frame's** load only; the latch and the policy filter both depend on it, the pinned bindings document only signatures, and smoke scenario 5 is what pins it observationally.

- [ ] **Step 4: Commit**

```bash
git add crates/kiosk-main/src/nav.rs
git commit -m "feat(linux): load lifecycle — FINISHED-sourced commit, failure latch, policy filter"
```

---

### Task 7: Profile clear with a real completion callback

**Files:**
- Modify: `crates/kiosk-main/src/clear.rs:27-33` — replace the stub with a delegation; add `#[cfg(not(windows))] mod linux_impl`
- Test: smoke scenario 6 (Task 10)

**Interfaces:**
- Consumes: `AppEvent::ProfileCleared`, `Telemetry::nav_error` (reused as `clear.rs:44-53` documents)
- Produces: nothing downstream

**Invariant, inherited verbatim from the Windows body:** `ProfileCleared` is sent **exactly once per call on every path**. A kiosk stranded on the `Clearing` gate is worse than a failed clear.

- [ ] **Step 1: Implement**

```rust
#[cfg(not(windows))]
pub fn clear(window: &tauri::WebviewWindow, tx: mpsc::Sender<AppEvent>, telem: Telemetry) {
    linux_impl::clear(window, tx, telem);
}

#[cfg(not(windows))]
mod linux_impl {
    use kiosk_core::app::state::Event as AppEvent;
    use tokio::sync::mpsc;
    use webkit2gtk::{
        gio, glib, WebContextExt, WebViewExt, WebsiteDataManagerExtManual, WebsiteDataTypes,
    };

    use crate::telemetry::Telemetry;

    fn report_failure(telem: &Telemetry, reason: &str) {
        eprintln!("clear: {reason}");
        telem.nav_error(reason);
    }

    /// `WebviewWindow::clear_all_browsing_data()` already performs this clear; the
    /// hand-rolled version buys ONLY the completion callback — which is the entire point,
    /// since that callback is what releases the FSM's `Clearing` gate. Shape copied from
    /// `wry-0.55.1/src/webkitgtk/mod.rs:809-819` with a real callback in place of wry's
    /// `|_| {}`.
    pub fn clear(window: &tauri::WebviewWindow, tx: mpsc::Sender<AppEvent>, telem: Telemetry) {
        let tx_outer = tx.clone();
        let telem_outer = telem.clone();
        let result = window.with_webview(move |platform_webview| {
            let webview = platform_webview.inner();
            let Some(manager) = webview.context().and_then(|c| c.website_data_manager()) else {
                report_failure(&telem, "clear_profile: no website data manager, profile not cleared");
                let _ = tx.try_send(AppEvent::ProfileCleared);
                return;
            };
            let telem_done = telem.clone();
            let tx_done = tx.clone();
            // `TimeSpan::from_seconds(0)` means *all data, all time* — it is not a
            // placeholder, do not "fix" it.
            manager.clear(
                WebsiteDataTypes::ALL,
                glib::TimeSpan::from_seconds(0),
                None::<&gio::Cancellable>,
                move |result| {
                    if let Err(e) = result {
                        report_failure(
                            &telem_done,
                            &format!("clear_profile: clear completed with an error, best-effort clear only: {e}"),
                        );
                    }
                    // Success OR failure: the gate must release either way.
                    let _ = tx_done.try_send(AppEvent::ProfileCleared);
                },
            );
        });
        if let Err(e) = result {
            report_failure(
                &telem_outer,
                &format!("clear_profile: with_webview failed, profile not cleared: {e}"),
            );
            let _ = tx_outer.try_send(AppEvent::ProfileCleared);
        }
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p kiosk-main && cargo clippy -p kiosk-main --all-targets -- -D warnings`
Expected: clean. If `website_data_manager()` returns a non-`Option`, drop the `and_then` and keep the failure branch reachable only through `context()`.

- [ ] **Step 3: Commit**

```bash
git add crates/kiosk-main/src/clear.rs
git commit -m "feat(linux): profile clear with the completion callback that releases the gate"
```

---

### Task 8: `offline.html` asset-origin portability

**Files:**
- Modify: `crates/kiosk-main/bundled/offline.html:22-30`

**Interfaces:**
- Consumes: `ASSET_ORIGIN`'s two spellings (Task 1) — but only as knowledge; there is no serve-time templating

- [ ] **Step 1: Replace the hard-coded `src` with page-local selection**

Remove the `src="http://kioskasset.localhost/kiosk-offline.mp4"` attribute from the `<video>` element and set it from `location.protocol`, keeping the existing comment about the asset being a runtime deployment file:

```html
<script>
  // The asset origin differs per platform (`http://kioskasset.localhost` on Windows,
  // `kioskasset://localhost` on Linux/WebKitGTK). Chosen page-locally from the origin
  // this page itself was served from — no serve-time templating.
  document.getElementById('offline-video').src =
    location.protocol === 'tauri:'
      ? 'kioskasset://localhost/kiosk-offline.mp4'
      : 'http://kioskasset.localhost/kiosk-offline.mp4';
</script>
```

Give the `<video>` element `id="offline-video"` if it does not already have one, and place the script after the element so the lookup resolves.

- [ ] **Step 2: Verify the arch-09 degrade path still holds**

Read the existing `offline.html` error handlers: an absent/404 mp4 must still degrade to the black splash rather than hanging. Confirm the handler is attached to the same element and still fires when `src` is set from script (attach handlers **before** assigning `src`).

- [ ] **Step 3: Commit**

```bash
git add crates/kiosk-main/bundled/offline.html
git commit -m "feat(linux): offline.html selects the mp4 origin from location.protocol"
```

---

### Task 9: Smoke harness under weston headless — scenarios 1–5

**Files:**
- Create: `packaging/smoke/run-smoke.sh` (compositor lifecycle + scenario driver)
- Create: `packaging/smoke/fixtures/home.html`, `packaging/smoke/fixtures/iframe-host.html`, `packaging/smoke/fixtures/kiosk.ini`, `packaging/smoke/fixtures/config.json` (signed)
- Create: `packaging/smoke/README.md` (how to run, what each scenario proves)

**Interfaces:**
- Consumes: the built `kiosk-main` binary, the P1 `kioskctl` signing harness (`cargo run -p kiosk-core --example kioskctl`, see `docs/testing/p1d2-signed-config-smoke.md`)
- Produces: the harness P2-F later automates; keep compositor start/stop in one function so P2-F can reuse it

**Environment (verified present on this host):** weston 13.0.0, cage 0.1.5, Xwayland, xdotool, WebKitGTK 2.52.3, GTK 3.24.41.

- [ ] **Step 1: Write the compositor lifecycle**

```bash
#!/usr/bin/env bash
# P2-A smoke harness (merge gate). Scenarios 1-7, all blocking, under weston headless.
# Human-run in-session; deliberately NOT wired into ci.yml — that is P2-F.
set -euo pipefail

SMOKE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNTIME_DIR="$(mktemp -d)"
export XDG_RUNTIME_DIR="$RUNTIME_DIR"
export WAYLAND_DISPLAY="wayland-smoke"
export WEBKIT_DISABLE_COMPOSITING_MODE=1   # smoke environment ONLY
chmod 700 "$RUNTIME_DIR"

start_compositor() {
  weston --backend=headless-backend.so --socket="$WAYLAND_DISPLAY" --idle-time=0 &
  WESTON_PID=$!
  for _ in $(seq 1 50); do
    [ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ] && return 0
    sleep 0.1
  done
  echo "weston did not create its socket" >&2
  exit 1
}

stop_compositor() {
  kill "${WESTON_PID:-}" 2>/dev/null || true
  wait "${WESTON_PID:-}" 2>/dev/null || true
  rm -rf "$RUNTIME_DIR"
}
trap stop_compositor EXIT
```

- [ ] **Step 2: Write the fixture server and the signed config**

The home fixture is served over plain HTTP from a port the allowlist names:

```bash
serve_fixtures() {
  ( cd "$SMOKE_DIR/fixtures" && python3 -m http.server 8099 >/dev/null 2>&1 ) &
  HTTPD_PID=$!
}
```

`fixtures/config.json` sets `content.url` to `http://localhost:8099/home.html` and `content.allowlist` to `["http://localhost:8099/*"]`. Sign it with the P1 harness and place the signed output plus `kiosk-credential.json` (mode `0600`) where `kiosk.ini` points. Follow `docs/testing/p1d2-signed-config-smoke.md` exactly — do not invent a second signing path.

- [ ] **Step 3: Assert from the on-disk spool, not from a fake endpoint**

```bash
# The spool is the durable telemetry record — no fake GCL endpoint needed.
spool_events() { cat /var/lib/kiosk/spool/main/*.jsonl 2>/dev/null; }
assert_event_count() {  # name, expected count
  local got; got="$(spool_events | grep -c "\"event\":\"$1\"" || true)"
  [ "$got" = "$2" ] || { echo "FAIL: $1 expected $2, got $got" >&2; exit 1; }
}
```

Point `resolve_data_dir` at the real `/var/lib/kiosk` (Task 2) — create it writable before the run.

- [ ] **Step 4: Implement scenarios 1–5**

1. **Boot → splash → remote home commits.** Assert one `nav.committed` for `http://localhost:8099/home.html`, and that the window reached fullscreen on the headless output. Record tao's observed monitor behavior under weston in `README.md` (this closes the spec's open decision on Wayland monitor placement — Wayland reports dummy monitor positions and `display.monitor` may degrade to primary + `config.warn`).
2. **Off-list navigation blocked.** Drive a link click to `http://evil.test/`; assert **exactly one** `nav.blocked` and that the page is unchanged. Then a `target=_blank` click to an in-allowlist URL: assert it navigates **in place** (no second window, `nav.committed` on the main webview); and an off-list `target=_blank`: exactly one `nav.blocked`.
3. **Offline fallback.** Stop the fixture httpd, drive a reload; assert the offline page loads from the app origin and the mp4 element resolves against `kioskasset://localhost`.
4. **Renderer crash.** `pkill -f WebKitWebProcess`; assert one `webview.crash` with `kind:"webkit_crashed"` and a subsequent navigate-home commit.
5. **Iframe (blocking — this is what pins the main-frame assumption).** Load `iframe-host.html` with one in-allowlist iframe and one off-allowlist iframe. Assert: the in-allowlist frame loads; the off-list frame produces **exactly one** `nav.blocked{reason:"not_allowlisted"}`; the top-level page is unchanged; and **no `nav.error`, no `NavigationFailed`, no error-page transition** appears in the spool.

- [ ] **Step 5: Run the harness**

Run: `bash packaging/smoke/run-smoke.sh`
Expected: scenarios 1–5 PASS. A failure here is a merge blocker, not a flake — scenario 5 in particular is the observational pin for the load-signal scope.

- [ ] **Step 6: Commit**

```bash
git add packaging/smoke
git commit -m "test(linux): weston-headless smoke harness, scenarios 1-5"
```

---

### Task 10: Smoke scenarios 6–7 and the gate

**Files:**
- Create: `crates/kiosk-main/examples/clear_probe.rs`
- Modify: `packaging/smoke/run-smoke.sh` (scenarios 6–7), `packaging/smoke/README.md`
- Create: `packaging/smoke/fixtures/kiosk-malformed.ini`

**Interfaces:**
- Consumes: `clear::clear` (Task 7), `boot::safe_boot` path, `machine_id` (Task 2)

- [ ] **Step 1: Write the clear-probe example**

There is no app-path producer for `ClearProfile` until P2-D, so scenario 6 needs its own binary. `examples/clear_probe.rs` builds a Tauri webview under the compositor, navigates it to a page that sets a cookie, calls `clear::clear` directly with a real `mpsc::Sender`, waits for `ProfileCleared` with a timeout, then re-navigates and asserts the cookie is gone. Exit non-zero with a printed reason on any failure.

- [ ] **Step 2: Add scenario 6 to the harness**

```bash
# 6. profile clear: dedicated harness binary, since no app path produces ClearProfile
#    until P2-D.
cargo run -p kiosk-main --example clear_probe || { echo "FAIL: scenario 6" >&2; exit 1; }
```

- [ ] **Step 3: Add scenario 7 (safe boot)**

Point the run at `fixtures/kiosk-malformed.ini` — a deliberately unparseable ini, which is the **only** path that reaches `safe_boot` (`boot.rs:139-145`). The credential-failure paths (`boot.rs:164-170,181-186`) keep the operator's bootstrap URL and are out of scope. Assert: `safe.html` renders from the app origin, and the safe config's `device_id` equals the trimmed contents of `/etc/machine-id`.

- [ ] **Step 4: Run the complete gate**

Run: `bash packaging/smoke/run-smoke.sh`
Expected: scenarios 1–7 all PASS under weston headless. Non-blocking extras, run and recorded but not gating: a cage-headless attempt, screenshots, real-cage/hardware.

- [ ] **Step 5: Record the results and the resolved open decisions**

In `packaging/smoke/README.md`, write down: the observed tao/Wayland monitor behavior (spec §Open decisions), the exact weston invocation and teardown discipline (P2-F will reuse it), and the pass/fail line for each of the seven scenarios with the date and the WebKitGTK/weston/cage versions the run proved.

- [ ] **Step 6: Commit**

```bash
git add crates/kiosk-main/examples/clear_probe.rs packaging/smoke
git commit -m "test(linux): smoke scenarios 6-7, P2-A merge gate complete"
```

---

## Self-Review

**Spec coverage:** origins/paths → T1, T2; `is_remote_origin` → T1; nav guard + popups → T5; load lifecycle (READY, latch, policy filter, classification) → T6; recovery → T4; clear → T7; SEC-09/C12 → T3; `offline.html` → T8; smoke 1–7 → T9, T10; host tests → T1–T4; open decisions (Wayland monitors, weston invocation) → T9 Step 4 / T10 Step 5.

**Deliberately not covered (spec §Scope/Out):** `hardening.rs`, `egress.rs`, downloads/PDF (P2-B); heartbeat transport (P2-C) — kiosk-main runs standalone-mode, which the existing `KIOSK_HEARTBEAT_PIPE`-absent path already supports; `idle.rs`, `gesture.rs` (P2-D); keep-awake (P2-B).

**Residual risk carried out of this plan:** non-frame subresource egress is unenforced on Linux until P2-B lands. **Do not field a Linux device before P2-B.**
