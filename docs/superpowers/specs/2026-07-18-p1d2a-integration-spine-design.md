# P1-D2a — kiosk-main Integration Spine (Design)

> Sub-project of P1-D2 (the `kiosk-main` Tauri app). Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2). This
> document covers ONLY the D2a spine; D2b–D2e are separate specs/plans.

**Status:** approved 2026-07-18. Execution requires a Windows host (Tauri /
WebView2 build + run); the plan is authored on Linux, executed on Windows.

## Goal

A real kiosk that boots from `kiosk.ini`, shows the content site fullscreen,
survives loss of connectivity with the offline video loop, polls remote config
on a timer, and emits structured telemetry to Google Cloud Logging — the pure
`kiosk-core` FSM (P1-D1) wired to live I/O. This is the first P1-D2 sub-project
and is a demonstrable kiosk on its own.

## Non-goals (deferred to later P1-D2 sub-projects)

- **D2b — webview hardening (§7)** and the page-initiated navigation intercept
  wired to `nav::decide`. D2a performs only FSM-initiated navigations (home,
  bundled pages), which are trusted; it does NOT yet intercept or vet page-world
  navigations.
- **D2c — native idle timer, technician exit gesture, PIN pad**, and the
  `ClearProfile` webview-data-clear execution (`Profile.ClearBrowsingDataAsync`
  etc.). D2a never arms the idle timer, so the `IdleExpired`/`Clearing`/
  `ClearProfile` FSM path stays dormant.
- **D2d — polished bundled assets** (final offline video, splash, error page).
  D2a ships minimal placeholder HTML sufficient to prove the effect wiring.
- **D2e — panic-hook richness, `health.sample`, `display.monitor` topology
  check.** D2a ships a minimal panic hook (log + flush) only.

## Architecture — actor spine, I/O at the edges

The pure `kiosk_core::app::Machine` (`on(&mut self, Event) -> Vec<Effect>`) is
`&mut self` and single-threaded. Real-world inputs originate on many tasks
(connectivity probe, config poll, timers, webview callbacks). They are funnelled
into a single `Event` channel consumed by one **driver** that owns the `Machine`;
the `Vec<Effect>` it returns is dispatched to an **`EffectSink`**.

```
        ┌─────────── tokio mpsc<Event> ────────────┐
 probe task ─LinkChanged(Online/Offline)─▶│         │
 config-poll task ─ConfigApplied/ConfigUnavailable─▶│
 countdown task ─CountdownExpired──────────────────▶│
 webview callbacks ─NavigationCommitted/Failed─────▶│
                                                     ▼
                                        DRIVER task (owns Machine)
                                        e = recv(); fx = machine.on(e);
                                        for f in fx { sink.dispatch(f) }
                                                     │
                                   ┌─────────────────┴─────────────────┐
                              EffectSink (trait)                  Logger task
                              = TauriSink (production)            (owns Logger)
                              → webview ops marshalled            ← mpsc<LogReq>
                                to the main thread via AppHandle
```

**Concurrency substrate:** tokio tasks (Tauri v2 bundles a tokio runtime;
`reqwest` is async-native). The driver is one task with an `mpsc::Receiver<Event>`;
timers are `tokio::time` intervals; config fetch and probe are async `reqwest`.
Shutdown is a shared `CancellationToken`.

**The `EffectSink` seam.** Effects leave the FSM through a trait, not a concrete
Tauri call. The production `TauriSink` drives the webview; a recording fake sink
makes the driver **host-testable without Tauri**. This mirrors kiosk-core's
`Transport` (telemetry) and the pure `Prober`: push I/O to the edge behind a
trait, keep the logic testable.

**Task / owner map:**

| Task | Owns | Responsibility |
|------|------|----------------|
| main thread | Tauri event loop | webview ops (run on main thread via `AppHandle`) |
| driver | `Machine` | event → `Machine::on` → `EffectSink` |
| logger | `Logger` | drain `LogReq` channel, periodic `tick`/`flush` |
| probe | — | periodic GET `connectivity_check_url` → `Prober` → `LinkChanged`; harvest `Date` → `TrustedClock` |
| config-poll | — | periodic GET config → `ConfigManager::apply_fetched` → `ConfigApplied`/`ConfigUnavailable` |
| countdown | — | on `ShowErrorPage{retry_after_seconds}`, fire `CountdownExpired` after the interval |

## Components — `crates/kiosk-main/src/`

- **`main.rs`** — thin bootstrap: parse args, `ConfigManager::boot`, build the
  fullscreen webview, install the panic hook, spawn tasks, `tauri::…run`.
  Deletes the P0 `spike.rs`.
- **`driver.rs`** — owns `Machine`; the `recv → on → dispatch` loop. Depends only
  on `kiosk_core` types and the `EffectSink` trait. **Host-tested** against a
  recording fake sink with scripted event sequences.
- **`effect.rs`** — the `EffectSink` trait and `TauriSink`. Mapping:
  - `Navigate(url)` → webview navigates to `url`.
  - `ShowVideo` → navigate to bundled `offline.html`.
  - `ShowSplash` → navigate to bundled `splash.html`.
  - `ShowErrorPage{retry_after_seconds}` → navigate to bundled `error.html` and
    arm a countdown task that emits `CountdownExpired` after the interval.
  - `RefetchConfig` → signal the config-poll task to fetch immediately.
  - `ClearProfile{..}` → **deferred to D2c**; logs a warning and no-ops.
- **`fetch.rs`** — async config GET (reqwest; connect 5 s, total 10 s per
  parent-spec cfg-10) returning bytes; hands them to `ConfigManager::apply_fetched`.
  A timeout or transport error is treated as a failed fetch (no-config path,
  cfg-10). Response→outcome mapping is host-tested.
- **`probe.rs`** — async GET of the connectivity URL → feeds the `Prober` (which
  damps and returns a flip) → `LinkChanged`; harvests the `Date` header into the
  shared `TrustedClock`. Outcome→prober feeding is host-tested.
- **`telemetry.rs`** — spawns the logger task and exposes a cheaply-clonable
  `Send` handle other tasks use to emit events (net.online/offline, nav.*, app
  lifecycle). Wraps the `LogReq` channel.
- **`bundled/`** — minimal `offline.html` (a `<video loop autoplay muted
  playsinline>`), `splash.html`, `error.html`. Placeholder assets; D2d replaces.

## Data flow — boot

1. Parse args; read `kiosk.ini`; `ConfigManager::boot(...)` → last-good
   (signature re-verified) if present, else the bootstrap URL, else none.
2. Build the fullscreen webview (from the P0 skeleton's builder).
3. Spawn driver, logger, probe, config-poll.
4. If boot resolved config: send `ConfigApplied{ url: home_url }` → driver →
   `Navigate(home)`. If not: send `ConfigUnavailable` → `ShowVideo`.
5. Steady state: probe drives `LinkChanged`; config-poll drives `ConfigApplied`;
   `Reconnected`/`RefetchConfig` drive recovery per the FSM rules.

## Error handling

- Every I/O failure becomes an `Event` or a logged non-fatal — never a panic.
  Fetch timeout → no-config path (cfg-10). Probe failure → prober damping. GCL
  delivery failure → the Logger's crash-durable spool (P1-B).
- A global **panic hook** logs `app.panic` and flushes the logger spool before
  the process aborts, so the watchdog (P1-E) restarts a device that leaves a
  breadcrumb. (Richer panic/health telemetry is D2e.)
- Malformed config (`content.url` scheme, wildcard allowlist, etc.) cannot reach
  the driver — it is rejected upstream in `kiosk-core` validation (P1-A/P1-C).

## Testing

- **Host-testable (no Windows), the bulk of the logic:**
  - `driver.rs`: a recording fake `EffectSink` + scripted `Event` sequences →
    assert the exact `Effect` sequence and ordering (e.g. boot-with-config →
    `Navigate`; boot-no-config → `ShowVideo`; offline→reconnect→refetch).
  - `fetch.rs` / `probe.rs`: response/outcome mapping, timeout handling, the
    `Date`-harvest call, prober feeding.
  - `telemetry.rs`: event-name/field shaping for the emitted events.
- **Windows-host only (executed by the human):** the `TauriSink` webview
  binding, real HTTP against a scratch config/probe endpoint, the fullscreen
  window, and an end-to-end manual smoke: boot → site visible → pull network →
  offline video within the probe window → restore → back to the site; confirm
  telemetry lands in Cloud Logging. The plan carries this as an explicit
  checklist.

## Carry-forwards honored (from P1-D1 and P1-C reviews)

- `ConfigApplied` is emitted **at level** (every config poll re-sends the current
  home), never edge-triggered — required by the FSM's same-url no-op / recovery
  arms (P1-D1 module docs).
- Live page-initiated navigations are NOT vetted in D2a; that intercept is D2b
  and MUST route through `nav::decide` (never `Allowlist::allows` /
  `scheme_decision` directly). D2a only performs trusted FSM-initiated navs.
- `display.monitor` out-of-range → primary + WARNING is D2e (needs display
  enumeration; the layering rule keeps it out of kiosk-core).
