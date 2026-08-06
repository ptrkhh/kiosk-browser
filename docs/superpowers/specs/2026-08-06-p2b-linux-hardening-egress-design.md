# P2-B — Linux Webview Hardening + Subresource Egress + Keep-Awake (Design)

> Second sub-project of P2. Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §7 (hardening
> matrix, SEC-10), §3.6 H2. **Builds on P2-A** (nav guard, load lifecycle, origin
> constants — `2026-08-06-p2a-linux-bringup-design.md` rev 3) and mirrors the reviewed
> D2b `windows_impl` semantics in `crates/kiosk-main/src/{hardening,egress,scheme_guard}.rs`.
> Same doctrine as A: shipped Tauri/wry APIs first, raw webkit2gtk only where no shipped
> route exists, behavior parity with what Windows *actually enforces* — not with what P1
> descoped.

**Status:** draft, 2026-08-06 (awaiting review). Closes the P2-A residual: after B, a
Linux device no longer has the "do not field before P2-B" egress hole. Pure additions
(filter compiler, CSP derivation, permission classifier) are host-tested; GTK/WebKit
wiring extends the A smoke harness.

## Goal

The three Windows-only control groups get Linux bodies with honest parity: `hardening.rs`
(settings flags, script dialogs, permissions), `egress.rs` (SEC-10 subresource
containment), `scheme_guard.rs` (downloads), plus `display.keep_awake`. "Honest parity"
cuts both ways: PDF blocking is **not** wired on Windows (`scheme_guard::pdf_decision` is
`#[allow(dead_code)]`, `scheme_guard.rs:36-40` — descoped ponytail with its reason on
record), so B does not wire it on Linux either; wiring it on *both* platforms is a
recorded future work item, not smuggled in here.

## Scope

**In:** Linux bodies for `hardening.rs` and `egress.rs`; downloads deny in the builder;
keep-awake; the pure helpers each needs (allowlist→content-filter compiler, allowlist→CSP
origin derivation, WebKit permission classifier); smoke scenarios 8–12.

**Out:** launcher/heartbeat/systemd → P2-C; idle/gesture → P2-D; video soak → P2-E;
CI automation of the harness → P2-F; OS image + logind config + WebKitGTK pin → P2-G.

**Documented divergences after B** (each justified in its section): path-scoped egress
blocks are enforced but *silent* on Linux; clipboard-read permission is unreachable
(always denied) on Linux; popups/downloads/schemes/frames otherwise match Windows or are
stricter, never looser.

## Architecture — routes

| Need | Route | Not this |
|---|---|---|
| Downloads deny | `WebviewWindowBuilder::on_download` (`tauri-2.11.5/src/webview/webview_window.rs:384`; `DownloadEvent::Requested` → `false`, `webview/mod.rs:75`) | a `download-started` signal handler |
| CSP belt inject/swap | `UserContentManager::{add_script, remove_script}` (safe bindings, `user_content_manager.rs:58,166`) | `initialization_script` (single-caller contract, `nav_policy.rs:146-150`) |
| Content filter | contained `unsafe` sys-FFI shim — the safe `add_filter`/`remove_filter` are commented out of webkit2gtk-rs 2.0.2 (`user_content_manager.rs:53,147`, gated-out `v2_24`), but `webkit2gtk-sys` is complete (`lib.rs:5411` store new, `:5467` save); removal via the safe `remove_filter_by_id` (`user_content_manager.rs:154`) | waiting for a bindings release |
| Settings/signals | safe webkit2gtk bindings: `set_enable_developer_extras` (`settings.rs:1475`), `set_zoom_level` (`web_view.rs:1980`), `connect_context_menu` (`:2074`), `connect_permission_request` (`:2428`), `connect_script_dialog` (`:2649`) | anything sys-level |
| Keep-awake | `systemd-inhibit` child process (below) | `gtk::Application::inhibit` or a `zbus` dependency |

**Feature/floor accounting (corrects this spec's own draft claim — checked, not
assumed):** three symbols above are feature-gated in the bindings —
`connect_script_dialog` (`v2_24`), `remove_filter_by_id` (`v2_26`), `remove_script`
(`v2_32`) — so B's direct dependency declares `features = ["v2_16", "v2_32"]`
(cumulative; supersedes A's bare `v2_16`). This changes nothing about the compiled
crate — wry itself enables `webkit2gtk/v2_40` (`wry-0.55.1/Cargo.toml`), so feature
unification already builds every gate ≤ `v2_40` — but our declared features must state
what *our* code calls, not lean on wry's. The **called-symbol floor** B introduces is
2.32, below both the 2.40 wry's own feature set implies and the Debian 12 shipped
version — no practical floor movement; the A-spec's floor-re-derivation rule is
discharged by this paragraph.

## Components

### `egress.rs` — SEC-10, two layers

Windows subscribes `WebResourceRequested` (all contexts) and 403s anything
`NavPolicy::resource_allowed` denies (`egress.rs:1-14`). WebKitGTK has no request-level
host API, so Linux composes two mechanisms, each doing what it is actually good at:

**Layer 1 — WebKit content filter (the enforcement boundary).** A pure, host-tested
compiler turns the live allowlist into WebKit's declarative content-rules JSON:
block-everything rule first, then `ignore-previous-rules` entries for (a) every
allowlist pattern (glob → the content-rules `url-filter` regex subset), (b) the app and
asset origins (A's constants — the bundled pages and the offline mp4 must never be
filtered), (c) the active content origin. Applied via the sys-FFI shim: store at
`data_dir/content-filters/`, async `save` → `add_filter` on the webview's existing
`UserContentManager`; on every `ConfigApplied`, compile under a fresh id, add, then
`remove_filter_by_id` the previous — never a gap with no filter while a page is live.
Compile/save failure → `config.warn` and Layer 2 stands alone (best-effort doctrine:
never block boot). Blocks at this layer are **silent** — WebKit fires no per-block
callback; that is Layer 2's job.

**Layer 2 — CSP belt (the observability layer).** P1 explicitly rejected injecting
`csp_policy` because its source list (`content_origin` + app origin only) is *tighter*
than the allowlist and would silently break an allowlisted CDN
(`nav_policy.rs:169-184`). B inverts the loss direction: a new pure derivation maps
every allowlist pattern to its **origin** (dropping path constraints — lossy toward
*permissive*), plus content origin + app origins. A belt that is looser than the
authority is safe; one that is tighter is the bug D2b refused to ship. The derived CSP
is injected as a document-start user script (adds the `<meta http-equiv>` plus a
`securitypolicyviolation` listener) and **swapped on `ConfigApplied`** via a kept
`UserScript` handle — `remove_script(&old)`, `add_script(&new)`; **never
`remove_all_scripts`** (`user_content_manager.rs:131`), which would destroy wry's own
IPC/initialization scripts living in the same manager. The violation listener reports
through a new `#[cfg(not(windows))]` Tauri command →
`telem.nav_blocked("egress", blocked_uri)` — the same `REASON_EGRESS` label Windows
emits (`egress.rs:22`), so operator dashboards see host-level egress blocks identically
on both platforms. Rate control is the Logger's existing `nav.blocked` bucket
(`egress.rs:112-116`'s no-second-limiter doctrine).

**Coverage honesty.** Off-origin egress (the exfil case) is blocked by *both* layers and
observable (CSP fires first, in-renderer). Path-scoped blocks (host allowlisted, path
off-pattern) are enforced by Layer 1 only and therefore silent — documented divergence.
CSP's own structural gaps — pre-installed service workers, preload timing
(`nav_policy.rs:152-167`) — are exactly the gaps a network-layer filter doesn't have,
which is why Layer 1 is the authority. Whether WebKit applies content rules to
service-worker-initiated fetches is **pinned by smoke, not assumed** (scenario 8's
SW variant). Windows is untouched: the native filter remains its sole boundary and the
belt is **not** injected there — D2b's decision stands on its platform.

### `hardening.rs` — control mapping, 1:1 where WebKit has the concept

| Windows (`windows_impl`) | Linux |
|---|---|
| `SetZoomFactor` (controller) | `WebViewExt::set_zoom_level` (`web_view.rs:1980`); whether full-content zoom needs `zoom-text-only=false` asserted explicitly — plan-time |
| `SetAreDefaultContextMenusEnabled(false)` | `connect_context_menu` → return `true` (suppress; `web_view.rs:2074`) |
| devtools off | `set_enable_developer_extras(false)` explicitly (`settings.rs:1475`) — wry already leaves it off unless `debug_assertions`/`devtools` feature (`wry/webkitgtk/mod.rs:28-35`); the explicit set is belt against a feature-flag mistake |
| autofill/password-save off | documented no-op — WebKitGTK ships no password manager/autofill store |
| script dialogs: none ever paints; `beforeunload` auto-leave; budget (`SCRIPT_DIALOG_BUDGET = 20`, `hardening.rs:102`) | `connect_script_dialog` → return `true` always (no dialog chrome exists to paint); `BeforeUnloadConfirm` → `confirm_set_confirmed(true)` (leave the page, matching Windows); same budget semantics mirrored |
| `PermissionRequested` → `classify_permission_kind(i32)` → `permission_allowed` (`hardening.rs:72`, `nav_policy.rs:219-227`) | `connect_permission_request` (`web_view.rs:2428`) → classify by the request's **runtime type** (WebKit subtypes are GObject classes, not an enum): `GeolocationPermissionRequest` → `Geolocation`; `NotificationPermissionRequest` → `Notifications`; `UserMediaPermissionRequest` → `Camera`/`Microphone` by `is_for_video_device`/`is_for_audio_device`; everything else (`DeviceInfo`, `MediaKeySystem`, `PointerLock`, `WebsiteDataAccess`, `InstallMissingMediaPlugins`, unknown) → `Other` → deny. `request.allow()`/`deny()` + return `true`. |

Same live `SharedNavPolicy`, same default-deny, same telemetry shape: `config.warn` on a
failed apply (`hardening.rs:191,216`), no per-denial event (none exists on Windows
either). **Divergence (stricter):** webkit2gtk-rs 2.0.2 has no clipboard permission
request type at all (checked: `src/auto/` contains no such binding), so
`Permissions::clipboard_read = true` is unsatisfiable on Linux — clipboard read is
always denied. Documented here and in the config schema's field doc; revisit only with a
bindings/floor bump.

The classifier is a pure `fn` over a small local enum mirroring the runtime-type check
(host-tested like `classify_permission_kind`), with the GObject downcasts confined to
the signal handler.

### Downloads — builder line, `scheme_guard.rs` stays a stub

`on_download` denies every `DownloadEvent::Requested` → `false`, emitting
`nav.blocked{download}` once (`REASON_DOWNLOAD`, `scheme_guard.rs:46`, made
`pub(crate)`). External schemes were already covered in A (`nav::decide`'s scheme
allowlist rides the nav guard); PDF stays unwired on both platforms (see Goal).
`scheme_guard.rs`'s `#[cfg(not(windows))]` stub message updates to say downloads are
covered by the builder hook and PDF is unwired-by-parity.

**A-filter re-derivation (discharging rev 2's recorded invariant in the A spec):** B
adds **no** `RESPONSE` policy subscription (PDF unwired) and `on_download` rides
`download-started` — a `WebContext` signal, not `decide-policy` — so A's stateless
`FrameLoadInterruptedByPolicyChange` drop-filter gains no new producer *class*; a
deny-cancelled download navigation that surfaces as that error is dropped exactly like
a guard block, preserving one-event-per-block. Whether it surfaces as `load-failed` at
all, and what the FSM sees, is pinned by smoke scenario 9 — including the comparison
question the spec cannot answer from bindings: it must **not** be looser than Windows
(where a cancelled download's `NavigationCompleted(IsSuccess=false)`, if it fires,
legitimately reaches the FSM). If smoke shows Linux swallowing an event Windows
delivers, the resolution is recorded there, not guessed here.

### Keep-awake — `systemd-inhibit` child

Windows asserts `ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED` once, for
process life, no undo (`main.rs:949-966`). Linux (`#[cfg(target_os = "linux")]`, same
`display.keep_awake` gate): spawn
`systemd-inhibit --what=idle:sleep --who=kiosk-browser --why="kiosk display" --mode=block cat`
with a piped stdin held by the process; the Child handle is kept for process life. Exit
symmetry comes free: kiosk-main dying EOFs the pipe → `cat` exits → logind releases the
inhibitor — the exact analogue of Windows resetting execution state on thread exit.
Spawn failure → `eprintln` + continue (best-effort, matching Windows' silent
`SetThreadExecutionState` failure mode). Rejected: `gtk::Application::inhibit` — tao
owns a real `gtk::Application` (`tao-0.35.3/src/platform_impl/linux/event_loop.rs:58`),
but GTK3's inhibit talks to a desktop session manager, which a cage/weston kiosk does
not run, so it silently no-ops exactly where we need it; `zbus` — a heavyweight
dependency for one D-Bus call (`ponytail:` revisit if P2-C/D acquire a D-Bus need of
their own). The *suspenders* are P2-G's image contract: no idle daemon,
`IdleAction=ignore` in `logind.conf` — recorded there, asserted at hardware validation.

## Smoke additions (extend A's harness; 8–11 blocking, 12 degrade-only)

8. **egress:** page from the allowlisted local httpd embeds (a) an `<img>` + `fetch()`
   to an off-allowlist host → blocked, `nav.blocked{egress}` spooled ≥1 (CSP path);
   (b) a subresource from a *second* allowlisted host with a path-scoped pattern —
   in-pattern path loads (belt-not-tighter check), off-pattern path is blocked
   *silently* (Layer 1, and its silence is asserted — no spurious CSP event);
   (c) a service-worker-initiated off-list fetch → blocked (pins the SW coverage
   question — if it passes the filter, the residual is recorded in the spec before
   merge, not discovered in the field).
9. **downloads:** click a `Content-Disposition: attachment` link → no file appears,
   exactly one `nav.blocked{download}`, kiosk stays on page; the load-event sequence
   is captured and recorded against the A-filter question above.
10. **dialog/chrome:** an `alert()`-loop page does not wedge the kiosk (budget
    semantics); right-click produces no context menu; `beforeunload` page navigates
    away without prompting.
11. **permissions:** `geolocation.getCurrentPosition` + `getUserMedia` probe page →
    denied (default-deny), allowed when the fixture config flips `permissions.camera`
    (live-policy check).
12. **keep-awake:** the container has no systemd, so the smoke asserts only the
    degrade path (spawn fails → eprintln, kiosk unaffected). The positive assertion
    (`systemd-inhibit --list` shows the hold) goes on the deferred hardware checklist
    with cage.

## Testing

- **Host tests (per-PR ubuntu CI):** filter-compiler — glob→`url-filter` regex over the
  documented content-rules subset, app/asset origins always admitted, block-rule-first
  ordering; CSP derivation — superset property (every allowlist pattern's origin
  present; content + app origins present; no path components survive); permission
  classifier — full mapping table incl. the `Other`-deny arm; `REASON_*` label pins.
- **Smoke:** scenarios 8–12 above, plus A's 1–7 re-run (the belt script and filter must
  not break A's pages — the app-origin admittance rules are what scenario 3/7 now also
  exercise).
- Existing gates unchanged (clippy `-D warnings` both platforms, Windows CI build).

## Error handling

Best-effort doctrine throughout, matching D2b: a failed setter, filter save, or script
swap logs `config.warn` and never blocks boot; the two egress layers degrade
independently; `try_send`/rate-capping via existing buckets only.

## Open decisions to resolve at plan time

- Exact sys-FFI shim shape (store lifetime, save-callback thread, `add_filter` pointer
  handling) against `webkit2gtk-sys-2.0.2` — and whether the shim can reuse the crate's
  own GObject wrappers via `glib::translate` rather than raw pointers end-to-end.
- The content-rules `url-filter` regex dialect limits (WebKit documents a restricted
  subset) — the glob→regex conversion must be *verified expressible*; patterns that
  cannot be expressed fail the compile loudly (`config.warn` naming the pattern), never
  silently drop to allow.
- Meta-CSP injection timing at document-start (documentElement-append idiom) and
  whether the violation listener needs `document` vs `window` attachment across
  same-document navigations.
- Whether a swapped user script applies to the *current* document or only subsequent
  loads (config-apply → belt refresh ordering; a home navigation follows config apply
  in the effect stream, which may make this moot — confirm against the driver).
- `zoom-text-only` interaction with `set_zoom_level` for full-content zoom parity.

## Scope / defer

Unchanged from A: launcher/systemd (P2-C), idle/gesture (P2-D), video soak (P2-E),
update+CI harness automation (P2-F), packaging/image/logind/hardware (P2-G). New
recorded ponytails: PDF wiring (both platforms, one design), `zbus` if a second D-Bus
consumer appears, clipboard-read on a future bindings/floor bump.
