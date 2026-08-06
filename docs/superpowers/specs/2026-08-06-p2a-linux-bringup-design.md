# P2-A — Linux Bring-up Spine + Nav Guard (Design)

> First sub-project of P2 (Linux/WebKitGTK port). Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.6 (WebKitGTK
> enforcement points), §4 (Linux paths). **Builds on P1 D2a/D2b** (FSM spine, `nav::decide`,
> the reviewed `#[cfg(windows)] mod windows_impl` semantics in `crates/kiosk-main/src/`) —
> this reimplements NO decision logic; it gives the existing `#[cfg(not(windows))]` stubs in
> `crates/kiosk-main/src/{nav,recovery,clear}.rs` real bodies, and moves the parts that a
> shipped Tauri/wry API already covers onto that API instead of hand-writing them.

**Status:** rev 3, 2026-08-06 — rev 2 (adversarial design review) verified against the
codebase and vendored sources; rev 3 restores popup navigate-in-place per parent §7. Executes on a Linux
host (Wayland). Pure additions (origin constants, event mappings, the Unix credential-mode
check) are host-tested; GTK signal wiring is covered by an in-session headless-compositor
smoke, which is the merge gate.

## Goal

`kiosk-main` boots on Linux/Wayland with the full P1 FSM spine live — nav guard, error/offline
paths, profile-clear completion, renderer-crash recovery, telemetry spooling — verified
headless in-session. Target floor: Debian 12 / Ubuntu 22.04, x86_64 (per parent spec §2;
specific hardware TBD — revisit at P2-G packaging).

## Scope

**In:** Linux bodies for `crates/kiosk-main/src/{nav,recovery,clear}.rs` as
`#[cfg(not(windows))] mod linux_impl` blocks mirroring the existing `windows_impl` shape;
platform-conditional app/asset origin constants; scheme-aware `is_remote_origin`; Linux
`resolve_data_dir` (`/var/lib/kiosk/`) and `machine_id` (`/etc/machine-id`); the Unix
implementation of `credential_is_owner_only` (SEC-09, see C12); `offline.html` asset-origin
portability; the headless smoke harness.

**Out (stubs unchanged):** `hardening.rs`, `egress.rs`, `scheme_guard.rs` downloads/PDF →
P2-B; heartbeat pipe transport → P2-C (kiosk-main runs standalone-mode in A, which the
existing `KIOSK_HEARTBEAT_PIPE`-absent path already supports); `idle.rs`, `gesture.rs` →
P2-D. Keep-awake stays Windows-only until P2-B (systemd-inhibit). All A changes are
`cfg`-gated or host-tested pure logic; the existing CI Windows build job keeps P1 green.

**Security controls that remain Windows-only in P2-A:** **SEC-10** non-frame subresource
egress (`egress.rs`, no Linux body → P2-B). The all-frame nav enforcement below covers the
iframe-navigation subset only; `<img>`, CSS `url()`, `fetch()` and beacon egress is
unenforced on Linux. **Residual risk: do not field a Linux device before P2-B.**
SEC-09 is **not** deferred — see C12.

## Architecture — platform access layer

Most of the Linux behaviour this sub-project needs is already shipped by wry and exposed by
Tauri. Use it; do not re-derive it:

| Need | Route | Not this |
|---|---|---|
| Nav blocking | `WebviewWindowBuilder::on_navigation` (`tauri-2.11.5/src/webview/webview_window.rs:266`) | a hand-written `decide-policy` handler |
| Popups | `WebviewWindowBuilder::on_new_window` → navigate-in-place + `NewWindowResponse::Deny` (`webview_window.rs:315`, `webview/mod.rs:253`; see Components) | `NEW_WINDOW_ACTION` plumbing |
| Crash recovery action | `WebviewWindow::navigate(Url)` (`webview/mod.rs:1689`) | any GTK call |

The direct `webkit2gtk` dependency is therefore justified by **three signals plus one
callback**, and nothing else: `load-changed` (`web_view.rs:2287`), `load-failed`
(`:2316`), `load-failed-with-tls-errors` (`:2355`), `web-process-terminated` (`:2853`), and
`WebsiteDataManagerExtManual::clear`'s completion callback. None of these has a wry or Tauri
route. `with_webview` → `PlatformWebview::inner()` → `webkit2gtk::WebView` is the route to
**those** things — not a general-purpose escape hatch.

Dependency: `[target.'cfg(target_os = "linux")'.dependencies] webkit2gtk = { version =
"2.0.2", features = ["v2_16"] }` — the version already in our lock via wry. Cargo unifies
semver-compatible requirements to a single crate, and `Cargo.lock` supplies the exact pin, so
`PlatformWebview::inner()`'s type and ours cannot diverge; no `=` requirement is needed (and
`webview2-com = "0.38"` at `crates/kiosk-main/Cargo.toml:28` is a caret, so `=` would be a new
discipline, not the cited precedent). `v2_16` is what `clear` needs; features are cumulative.
`glib`/`gio` come via `webkit2gtk::glib` / `webkit2gtk::gio` re-exports — no new direct deps.

**P2-A introduces no `v2_40`-gated symbol**; the deployed WebKitGTK floor stays whatever wry
0.55.1 already requires. Do not reintroduce `ResponsePolicyDecision::is_main_frame_main_resource`
without re-deriving that floor. The four signals above are stable since 2.20;
`WebsiteDataManager::clear` is `v2_16`-gated.

Threading: the `with_webview` closure runs on the GTK main thread, where signal connects are
legal; handlers talk outward only through the existing `Send + Clone` handles
(`mpsc::Sender<AppEvent>` via `try_send`, `Telemetry`). GTK objects never leave the main thread.

## Origins & paths

`APP_ORIGIN` stays a `const`, gaining a compile-time switch — the same shape Tauri uses
internally (`tauri-2.11.5/src/manager/mod.rs:340-345`):

```rust
const APP_ORIGIN: &str = if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" };
```

Same one-line treatment for the asset origin (`http://kioskasset.localhost` /
`kioskasset://localhost`). `bundled_url` (`main.rs:59`), every effect target, and
`boot.rs:202`'s `APP_SAFE_URL` keep working with **zero signature churn**. Rewrite
`main.rs:45-52`'s doc comment onto the new shape: keep the `AppManager::tauri_protocol_url`
citation, drop "Revisit if/when a Linux/macOS target ships (spec P2/P3)" — this sub-project
resolves it.

`is_remote_origin` (`nav_policy.rs:233-243`) becomes a `(scheme, host)` match with **no
`cfg`**, because the current host-only match classifies every Linux app origin as remote
(`tauri://localhost`, `kioskasset://localhost` and `ipc://localhost` all have host
`"localhost"`), which would make bundled pages self-block and would feed the error page's own
load into the FSM:

- app-origin iff `scheme ∈ {tauri, kioskasset, ipc}` **and** `host == "localhost"`, or
  `scheme ∈ {http, https}` and `host ∈ {tauri.localhost, kioskasset.localhost, ipc.localhost}`;
- **the host is required on the custom schemes too** — otherwise `tauri://evil.test/`
  classifies as app-origin;
- **parse failure → `false`, unchanged.** Failing closed here would newly block unparseable
  URLs on Windows, break `nav.rs:334-337`, and invert `resource_allowed`'s inline/hostless
  rule (`nav_policy.rs:131-134`);
- never add bare `"localhost"` to the host set — the smoke harness's own
  `http://localhost:PORT` home would become app-origin.

`offline.html` picks the mp4 URL by `location.protocol` (page-local JS, no serve-time
templating). `resolve_data_dir()` → `/var/lib/kiosk/` on Linux (parent spec §4, never
operator-overridden). `machine_id()` → `/etc/machine-id`, trimmed; absent/unreadable degrades
exactly as the Windows no-machine-guid path does.

## Components

### Nav guard — one builder line, not a handler

```rust
// #[cfg(not(windows))], at the WebviewWindowBuilder in main.rs:1014-1049
builder = builder.on_navigation(move |url| {
    match should_block(&policy.load(), url.as_str(), true) {
        Some(reason) => { telem.nav_blocked(reason.as_str(), url.as_str()); false }
        None => true,
    }
});
let handle = app.handle().clone();
builder = builder.on_new_window(move |url, _features| {
    let _ = handle.get_webview_window(WINDOW_LABEL).map(|w| w.navigate(url));
    NewWindowResponse::Deny
});
```

wry already installs the `decide-policy` handler this drives
(`wry-0.55.1/src/webkitgtk/mod.rs:547-576`): `NavigationAction` only, every frame, `use`/
`ignore`, correct return value. **Do not write a `decide-policy` handler.** `should_block`
(`nav.rs:56`) is the existing reviewed composition — main-frame gate, app-origin
short-circuit, `NavPolicy::decision_for` → `kiosk_core::nav::decide`. Windows keeps
`nav.rs:169` untouched.

The `true` third argument is **not** the Windows justification (`NavigationStarting` is
main-frame-only; that does not transfer). It is a deliberate Linux decision: enforce on
**all frames**.

**Divergence from Windows, both directions — a 1:1-parity spec must state both:**

- *Stricter:* Windows waves sub-frames past the guard (`nav.rs:56-59`, pinned by the test at
  `nav.rs:307`) because `egress.rs` catches them; Linux blocks them at the nav guard. A
  blocked sub-frame reports `nav.blocked{reason: "not_allowlisted"}` where Windows reports
  `nav.blocked{reason: "egress"}` (`egress.rs:22`). Operator dashboards must treat both as
  "sub-frame blocked"; do not retrofit `"egress"` onto the Linux guard.
- *More permissive:* non-frame subresource egress is unenforced on Linux — see §Scope/Out
  for the complete list of controls that are Windows-only in P2-A.
- *Parity, worth stating because it looks like a divergence:* an unparseable navigation URI is
  allowed on **both** platforms — on Linux via `tauri-runtime-wry-2.11.4/src/lib.rs:4903`'s
  `unwrap_or(true)`, on Windows via `should_block`'s `is_remote_origin` short-circuit at
  `nav.rs:57`. Same fail-open, same trust boundary, pre-existing.
- *Popups — parity, corrected in rev 3:* rev 2 denied outright, reasoning "fail-closed either
  way, the difference is UX, not control" — but navigate-in-place is a requirement of the
  parent spec of record, not a UX nicety: §7's hardening table, "Downloads / popups / file
  pickers — blocked; **new windows navigate in place**". So `on_new_window` hands the URL
  back into the main webview and *then* denies (snippet above): `navigate` is a
  dispatcher-proxied non-blocking send, safe from the event-loop thread, and the resulting
  load re-enters `on_navigation` and faces the guard — exactly Windows' `SetHandled(true)` +
  `Navigate` take-over (`nav.rs:186-207`, which cites the same requirement). A blocked popup
  still yields exactly one `nav.blocked`: the guard emits it, and the interrupted load's
  `load-failed` is dropped by the policy filter below. Deny explicitly rather than relying on
  wry not connecting `connect_create`.

Amend `telemetry.rs:120-121`'s doc comment: "A navigation was cancelled by the native guard
(main-frame on Windows; any frame on Linux — see `nav.rs`)." Doc-only, `cfg`-free.

Scheme-allowlist enforcement rides along, since `decide` already covers it: `scheme_guard.rs`'s
Linux stub becomes a documented "covered by the nav guard on Linux" no-op. Downloads/PDF stay
P2-B.

### `nav.rs` — load lifecycle

- **READY pulse and `AppEvent::NavigationCommitted` come from `load-changed(FINISHED)`, not
  `COMMITTED`.** Windows sources both from `NavigationCompleted` + `IsSuccess`
  (`nav.rs:209-261`) — i.e. load *finished*. `Committed` fires on first response, so mapping it
  would emit `NavigationCommitted` then `NavigationFailed` for one navigation:
  `(ErrorPage, NavigationCommitted) → Online` (`state.rs:259-264`) then
  `(Online, NavigationFailed) → ErrorPage{attempts: 1}` (`state.rs:238-243`), which **resets the
  retry counter**, so `error_max_retries` (`state.rs:272-275`) is never reached and the kiosk
  never falls through to the offline video.
- **Failure latch:** `load-failed`/`load-failed-with-tls-errors` set a flag;
  `FINISHED` emits only when the flag is clear; the flag is **cleared on
  `load-changed(Started)`** — the only per-load boundary WebKit provides. Clearing on `Started`
  rather than on the suppressed `FINISHED` is load-bearing: a `load-failed` with no following
  `FINISHED` must not arm the latch across navigations, which would swallow the next successful
  load's `Committed` and park the kiosk on the offline video with a healthy network.
  One `Rc<Cell<bool>>`.
- **URL classification:** `FINISHED` classifies with `WebViewExt::uri()` read at signal time
  (`web_view.rs:1219`); `load-failed` classifies with the `failing_uri` the signal already
  hands it. Do **not** port the Windows `navId → uri` map — WebKit gives no navigation id, and
  that map exists on Windows only because `NavigationCompleted` lacks a URI. Documented
  divergence: `uri()` is post-redirect, the Windows map pre-redirect; both classify the same
  for `is_remote_origin`, since a redirect cannot cross into our own registered schemes.
  `uri()` returns `Option` — a `None` classifies as not-remote and is suppressed; never
  `unwrap()` in a signal handler (wry does, at `mod.rs:476,479`; do not copy it).
- Do **not** use `on_page_load` for the committed half: it has no failure variant, so splitting
  the two halves across two mechanisms buys nothing while still requiring the dependency.
- `AppEvent::NavigationFailed` and `telem.nav_error(&err.to_string())` fire for remote origins
  only, **both inside the same filter**, mirroring `nav.rs:243-256` where the `nav_error` call
  sits after the `feeds_fsm` early return. App-origin load failures stay silent, as on Windows.
  There is no error→kind table: Windows deliberately formats the raw status and says so.
- **Policy cancellations are dropped statelessly:** a `load-failed` whose `glib::Error` matches
  `PolicyError::FrameLoadInterruptedByPolicyChange` (`err.kind::<PolicyError>()`,
  `enums.rs:2702`) **returns immediately — no `AppEvent`, no telemetry, no latch change.** The
  guard already emitted the single `nav.blocked`, matching Windows' one-event-per-blocked-
  navigation (`nav.rs:163-168`). This also makes the handler independent of whether the
  cancellation surfaces as a `load-failed` at all.
  - *Why a phantom `FINISHED` after a cancelled load is harmless, written down because it is
    not obvious:* `uri()` then returns the **unchanged current** document. In `ErrorPage` /
    `Offline` / `Clearing` that is a bundled app-origin page, so `is_remote_origin` filters it;
    in `Online` the resulting `NavigationCommitted` lands on the catch-all `_ => Vec::new()`
    (`state.rs:334-336`) and is a no-op. **This holds only while no
    `(Online, NavigationCommitted)` arm exists** — the dangerous neighbour is one line away at
    `state.rs:259-264`.
  - *Invariant:* this filter assumes P2-A installs exactly one `navigation_handler` and
    subscribes no RESPONSE or download decision. P2-B adds both, and they raise the same error
    code from a different cause; P2-B must re-derive this filter or it will start swallowing
    real `NavigationFailed`s.
- **Assumption, not derivable from the pinned bindings:** `load-changed`/`load-failed` are
  `WebKitWebView`-level signals tracking the **main frame's** load only, so sub-frame loads
  never drive them. The latch and the policy filter both depend on this
  (`web_view.rs:2286,2315` give signatures only, no doc text). Pinned observationally by smoke
  scenario 5, not asserted here. This is the one load-bearing assumption left, and it is why
  scenario 5 is blocking.

### `recovery.rs` — renderer crash

`web-process-terminated(reason)` → `webview.crash` telemetry + `WebviewWindow::navigate(home
.parse()?)` (it takes `Url`, not `&str`). All three WebKit reasons are process-gone, so all
three take `NavigateHome`; there is no `Reload` branch (Windows reserves `Reload` for
`RENDER_PROCESS_UNRESPONSIVE`, which has no WebKitGTK analogue).

Add a separate `#[cfg(not(windows))] fn termination_label(reason: WebProcessTerminationReason)
-> &'static str`. **Do not call `kind_label` or `recovery_action` with a WebKit reason** — they
take a raw `i32` in the WebView2 constant space (`recovery.rs:50-59,66,76`), which overlaps the
WebKit space with entirely different meanings: `Crashed = 0` would label as
`browser_process_exited`, and `recovery_action(2)` would return `Reload` for a dead process.

### `clear.rs` — profile clear with real completion

Copy `wry-0.55.1/src/webkitgtk/mod.rs:809-819` and substitute a real callback for wry's
`|_| {}`: `PlatformWebview::inner()` → `WebViewExt::context()` → `WebContextExt::
website_data_manager()` → `WebsiteDataManagerExtManual::clear(WebsiteDataTypes::ALL,
glib::TimeSpan::from_seconds(0), None::<&gio::Cancellable>, cb)` → `AppEvent::ProfileCleared`.

`WebviewWindow::clear_all_browsing_data()` (`webview/mod.rs:2122`) already performs this clear;
the hand-rolled version buys **only** the completion callback — which is the entire point, since
that callback is what releases the FSM's `Clearing` gate. `TimeSpan::from_seconds(0)` means
*all data, all time* — it is not a placeholder, do not "fix" it. Every failure path (no data
manager, clear error) still sends `ProfileCleared`, so `Clearing` is never stranded; the failure
signal is the existing `Telemetry::nav_error`, reused as `clear.rs:44-53` documents. Windows
parity is `ClearBrowsingData` (not `…Async` — `clear.rs:98-101` records that correction).

### `credential_acl.rs` — SEC-09 on Unix (C12)

**Replace** the `#[cfg(not(windows))]` stub at `credential_acl.rs:100-104` (do not add beside
it — `#[cfg(unix)]` and `#[cfg(not(windows))]` both match on Linux, giving a duplicate
definition):

```rust
#[cfg(unix)]
pub fn credential_is_owner_only(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o077 == 0)
}
```

The stub returns `Ok(true)` unconditionally, so **both** SEC-09 gates — boot (`boot.rs:165`)
and every config fetch (`fetch.rs:100`) — fail open on Linux with no `config.warn` and no
telemetry. Its own doc comment states the premise "the kiosk target is Windows x64 only"
(`credential_acl.rs:27-30`): **P2-A is the commit that falsifies that sentence**, so P2-A owns
the fix. Rewrite that doc comment in the same edit. Fail-closed semantics come free — a
missing or unreadable file yields `Err`, which `is_violation` (`credential_acl.rs:23-25`)
already treats as a violation, exactly as on Windows.

`ponytail:` mode bits only, no uid check — a root-owned `0o600` file is the deployment shape;
add an owner check if a non-root service user lands in P2-C.

## Smoke harness (merge gate)

Headless Wayland compositor in-session: **weston headless**. Fixtures: local HTTP server
serving a **signed** config via the P1 `kioskctl` signing harness; telemetry asserted from the
**on-disk spool** (the durable record — no fake-GCL endpoint needed).

1. boot → splash → remote home `nav.committed` (local httpd allowlisted);
2. off-list navigation → exactly one `nav.blocked`, page unchanged; a `target=_blank` click
   navigates in place (no second window; an off-list one → exactly one `nav.blocked`);
3. config/network down → offline fallback page loads from app origin;
4. kill the `WebKitWebProcess` → `webview.crash` spooled + recovery navigate-home;
5. **iframe:** an in-allowlist iframe loads; an off-allowlist iframe is blocked with
   `nav.blocked{reason: "not_allowlisted"}` exactly once, the top-level page is unchanged, **and
   the blocked iframe produces no `NavigationFailed`, no `nav.error` and no error-page
   transition** — this last assertion is what pins the main-frame scope of
   `load-changed`/`load-failed`;
6. profile clear: no app-path producer for `ClearProfile` until P2-D, so a dedicated harness
   binary (cargo example) creates a webview under the compositor, drives `clear::clear`
   directly, and asserts cookie-gone + `ProfileCleared` received;
7. **safe boot:** fixture is a **malformed `kiosk.ini`** — the only path reaching `safe_boot`
   (`boot.rs:139-145`); the credential-failure paths (`boot.rs:164-170,181-186`) keep the
   operator's own bootstrap URL and are out of scope here. Asserts `safe.html` renders from the
   app origin, and the safe config's `device_id` reflects `/etc/machine-id`
   (`boot.rs:191-199`). One fixture, both new Linux surfaces.

**Gate:** scenarios 1–7 under weston headless, **all blocking**. Non-blocking: cage-headless (a
bonus attempt — its failure does not block), screenshots (best-effort), real-cage/hardware. The
smoke is human-run in-session and is deliberately **not** wired into `ci.yml`; automating the
compositor harness is P2-F. `WEBKIT_DISABLE_COMPOSITING_MODE=1` permitted in the smoke
environment only.

## Testing

- **Host tests (existing per-PR ubuntu CI job):** `APP_ORIGIN`/asset-origin both forms;
  `is_remote_origin` over the Linux origins, `tauri://evil.test/` (must be remote), and the
  parse-failure arm; `termination_label` over all three `WebProcessTerminationReason` variants
  plus `__Unknown` (pattern at `recovery.rs:170-208`); `credential_is_owner_only` on Unix
  (permissive mode → `false`, `0o600` → `true`, missing path → `Err`). The wiring itself also
  *compiles* per-PR on Linux CI — coverage the COM code never had.
- **Smoke (in-session, gate):** scenarios 1–7 above.
- Existing gates unchanged: clippy `-D warnings` both platforms, CI Windows build, JWT/spool
  contract tests. The Linux dead-code warnings shrink as stubs gain bodies; any remainder stays
  `cfg`-annotated, not `allow`ed away.

## Error handling

Same doctrine as Windows: handler errors degrade to telemetry (`config.warn` / `nav.blocked` /
`webview.crash`), never panic in a signal handler; `try_send` drops on a full channel (bounded,
same as D2a); clear completion always releases the gate.

## Open decisions to resolve at plan time

- **Wayland monitor placement:** Wayland reports dummy monitor positions; `display.monitor`
  selection may degrade to primary + `config.warn` (the code's existing fallback at
  `main.rs:385-388`). Confirm tao's actual behavior under weston during implementation;
  document observed behavior.
- Exact weston invocation + compositor teardown discipline for CI-adjacent reuse (P2-F will
  want this harness).

## Scope / defer

Deferred to their owning sub-projects: hardening controls + egress + downloads (P2-B),
launcher/heartbeat/systemd (P2-C), native input + idle (P2-D), offline-video soak (P2-E),
update path (P2-F), packaging + OS image + hardware validation (P2-G). Offline-video in A is
wiring-only (asset serves, page renders headless); playback quality is P2-E's.
