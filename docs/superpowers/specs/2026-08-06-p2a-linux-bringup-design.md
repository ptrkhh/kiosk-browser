# P2-A — Linux Bring-up Spine + Nav Guard (Design)

> First sub-project of P2 (Linux/WebKitGTK port). Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.6 (WebKitGTK
> enforcement points), §4 (Linux paths). **Builds on P1 D2a/D2b** (FSM spine, `nav::decide`,
> the reviewed Windows `platform/windows/*` semantics) — this reimplements NO decision logic;
> it gives the existing `#[cfg(not(windows))]` stubs in `platform/linux/` real WebKitGTK
> bodies with 1:1 behavioral parity to the Windows modules.

**Status:** draft 2026-08-06 (awaiting review). Executes on a Linux host (WebKitGTK 2.40+,
Wayland). Pure additions (origin helper, event mappings) are host-tested; GTK signal wiring
is covered by an in-session headless-compositor smoke, which is the merge gate.

## Goal

`kiosk-main` boots on Linux/Wayland with the full P1 FSM spine live — nav guard, error/offline
paths, profile-clear completion, renderer-crash recovery, telemetry spooling — verified
headless in-session. Target floor: Debian 12 / Ubuntu 22.04, x86_64 (per parent spec §2;
specific hardware TBD — revisit at P2-G packaging).

## Scope

**In:** Linux bodies for `platform/linux/{nav,recovery,clear}.rs`; platform-conditional
app/asset origin helper; Linux `resolve_data_dir` (`/var/lib/kiosk/`) and `machine_id`
(`/etc/machine-id`); `offline.html` asset-origin portability; the headless smoke harness.

**Out (stubs unchanged):** `hardening.rs`, `egress.rs`, `scheme_guard.rs` downloads/PDF →
P2-B; heartbeat pipe transport → P2-C (kiosk-main runs standalone-mode in A, which the
existing `KIOSK_HEARTBEAT_PIPE`-absent path already supports); `idle.rs`, `gesture.rs` →
P2-D. Keep-awake stays Windows-only until P2-B (systemd-inhibit). All A changes are
`cfg`-gated or host-tested pure logic; the existing CI Windows build job keeps P1 green.

## Architecture — platform access layer

Where Windows uses `with_webview` → `webview2-com` raw COM, Linux uses `with_webview` →
`PlatformWebview::inner()` → `webkit2gtk::WebView`, with **webkit2gtk-rs 2.0.2 as a direct
`[target.'cfg(target_os = "linux")'.dependencies]`** — the exact version already in our lock
via wry, so zero version skew (same discipline as pinning `webview2-com` 0.38 to wry's
WebView2 SDK).

Threading: the `with_webview` closure runs on the GTK main thread, where signal connects are
legal; all handlers run there and talk outward only through the existing `Send + Clone`
handles (`mpsc::Sender<AppEvent>` via `try_send`, `Telemetry`) — the same
callbacks-feed-the-channel pattern as the Windows COM handlers. GTK objects never leave the
main thread.

Signal/API surface used — all stable since webkit2gtk 2.20, no floor risk on Debian 12's
2.40+: `decide-policy`, `load-changed`, `load-failed`, `web-process-terminated`,
`WebsiteDataManager::clear`.

## Origins & paths

One new pure helper (host-tested; lives beside `nav_policy`): `app_origin()` =
`http://tauri.localhost` (Windows) / `tauri://localhost` (Linux); `asset_origin()` =
`http://kioskasset.localhost` / `kioskasset://localhost` (Tauri custom-scheme origin forms
differ per platform). It replaces the `APP_ORIGIN` const and feeds:

- `bundled_url(page)` and every effect target in `main.rs`;
- `nav_policy`'s app-origin recognition (the D2a-C1 rule — app-origin loads never feed the
  FSM) — gains the Linux forms including `ipc://localhost`;
- `offline.html`, which picks the mp4 URL by `location.protocol` (tiny page-local JS — no
  serve-time templating).

`resolve_data_dir()` → `/var/lib/kiosk/` on Linux (parent spec §4, never
operator-overridden). `machine_id()` → `/etc/machine-id`, trimmed; feeds the existing
identity derivation unchanged (absent/unreadable degrades exactly as the Windows
no-machine-guid path does).

## Components — 1:1 mirrors of the reviewed Windows semantics

### `nav.rs` — decide-policy + load lifecycle

- `decide-policy` (`NAVIGATION_ACTION`): extract URI, call `nav::decide(url, allowlist,
  scheme_allowlist, is_remote_origin)` — the single combined entry point (P1-C
  carry-forward) — and `decision.ignore()` on deny → `nav.blocked` telemetry. Main-frame-only
  enforcement per parent §3.6; sub-frame navigations permitted.
- Because `nav::decide` already covers the scheme allowlist, external-scheme blocking lands
  here for free: `scheme_guard.rs`'s Linux stub becomes a documented "covered by nav.rs
  decide-policy on Linux" no-op. Downloads/PDF (`download-started`, `RESPONSE` decisions)
  stay P2-B.
- `NEW_WINDOW_ACTION`: `decision.ignore()` + `webview.load_uri(request uri)` — popups are
  redirected into the main window, where the load re-enters `decide-policy` and faces
  `nav::decide` like any navigation. This mirrors the Windows `NewWindowRequested` handler
  (`Handled = true` + `Navigate(uri)`), not a flat deny.
- `load-changed(COMMITTED)` → READY pulse on **first commit of any origin** (the E2
  decision), `AppEvent::NavigationCommitted` for remote origins only (D2a-C1).
- `load-failed` (and `load-failed-with-tls-errors`) → `AppEvent::NavigationFailed` for
  remote origins only, with the WebKit error mapped through a pure, host-tested
  `error → kind` table mirroring the Windows `WebErrorStatus` mapping.

### `recovery.rs` — renderer crash

`web-process-terminated(reason)` → `webview.crash` telemetry with a pure reason→kind map
(`crashed` / `exceeded-memory-limit` / `terminated-by-api`) + the same recovery action the
Windows `ProcessFailed` handler performs (reload policy home).

### `clear.rs` — profile clear with real completion

`WebsiteDataManager::clear(WEBKIT_WEBSITE_DATA_ALL, 0, …)` with the **async completion
callback** → `AppEvent::ProfileCleared`. Every failure path (no data manager, clear error)
still sends `ProfileCleared` — the FSM's Clearing state is never stranded (exact
`ClearBrowsingDataAsync` parity, including the privacy-gate ordering: gate releases only on
completion).

## Smoke harness (merge gate)

Headless Wayland compositor in-session: **weston headless** primary (cage-headless attempted
as a bonus; real-cage/hardware run stays on the deferred list). Fixtures: local HTTP server
serving a **signed** config via the P1 `kioskctl` signing harness; telemetry asserted from
the **on-disk spool** (the durable record — no fake-GCL endpoint needed).

Scenarios:

1. boot → splash → remote home `nav.committed` (local httpd allowlisted);
2. off-list navigation → `nav.blocked`, page unchanged;
3. config/network down → offline fallback page loads from app origin;
4. kill the `WebKitWebProcess` → `webview.crash` spooled + recovery reload;
5. iframe navigation NOT blocked (pins the main-frame discriminator, below);
6. profile clear: no app-path producer for `ClearProfile` until P2-D, so a dedicated
   harness binary (cargo example) creates a webview under the compositor, drives
   `clear::clear` directly, and asserts cookie-gone + `ProfileCleared` received.

Screenshots best-effort; assertions rest on spool contents + process behavior.
`WEBKIT_DISABLE_COMPOSITING_MODE=1` permitted in the smoke environment only.

## Testing

- **Host tests (run in the existing per-PR ubuntu CI job):** origin helper both forms;
  `nav_policy` origin-recognition with Linux origins; `load-failed` error→kind map;
  crash reason→kind map. Note the wiring itself also *compiles* per-PR on Linux CI —
  coverage the COM code never had.
- **Smoke (in-session, gate):** scenarios above.
- Existing gates unchanged: clippy `-D warnings` both platforms, CI Windows build, JWT/spool
  contract tests. The ~46 Linux dead-code warnings shrink as stubs gain bodies; any
  remainder stays `cfg`-annotated, not `allow`ed away.

## Error handling

Same doctrine as Windows: handler errors degrade to telemetry (`config.warn` /
`nav.blocked` / `webview.crash`), never panic in a signal handler; `try_send` drops on a
full channel (bounded, same as D2a); clear completion always releases the gate.

## Open decisions to resolve at plan time

- **Main-frame discriminator on `NAVIGATION_ACTION`:** webkit2gtk's frame-info surface
  changed across versions. Confirm the mechanism against webkit2gtk-rs 2.0.2 docs at plan
  time; if pre-request frame info is unavailable, fall back to enforcing at the `RESPONSE`
  decision's `is_main_frame_main_resource` (later than Windows' NavigationStarting, still
  pre-display). Scenario 5 pins whichever lands.
- **Wayland monitor placement:** Wayland reports dummy monitor positions; `display.monitor`
  selection may degrade to primary + `config.warn` (the code's existing fallback). Confirm
  tao's actual behavior under weston during implementation; document observed behavior.
- Exact weston invocation + compositor teardown discipline for CI-adjacent reuse (P2-F will
  want this harness).

## Scope / defer

Deferred to their owning sub-projects: hardening controls + egress + downloads (P2-B),
launcher/heartbeat/systemd (P2-C), native input + idle (P2-D), offline-video soak (P2-E),
update path (P2-F), packaging + OS image + hardware validation (P2-G). Offline-video in A
is wiring-only (asset serves, page renders headless); playback quality is P2-E's.
