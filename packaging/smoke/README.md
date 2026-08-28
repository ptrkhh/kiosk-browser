# Linux smoke harness — weston/cage headless

Scenarios 1–17. The shell driver owns the executable scenario bodies; CI and
the Debian endurance job invoke them through the ignored tests in
`crates/kiosk-smoke` (F owns A 1–7 · B 8–12 · C 13–15 · D 16–17).

## Run it

```bash
KIOSK_CONFIG_PUBKEY_B64=<see below> cargo build --release -p kiosk-main
cargo build --release -p kiosk-core --example kioskctl
cargo test --release -p kiosk-smoke --no-run
SMOKE_TEST="$(find target/release/deps -maxdepth 1 -type f -name 'smoke_linux-*' -perm -111 | head -n 1)"
KIOSK_SMOKE_I_MEAN_IT=1 KIOSK_BIN=target/release/kiosk-main \
  KIOSKCTL_BIN=target/release/examples/kioskctl \
  KIOSK_SIGNING_KEY_B64=<ephemeral-seed> \
  KIOSK_SMOKE_DRIVER="$PWD/packaging/smoke/run-smoke.sh" \
  "$SMOKE_TEST" --ignored --exact scenario_1_boot_and_fullscreen
```

Scenarios 8–17 also require the release `kioskctl`, a matching ephemeral
`KIOSK_SIGNING_KEY_B64`, and the release `fixture-httpd`; CI supplies all three.
The harness deliberately wipes `/var/lib/kiosk` between scenarios, so the
explicit `KIOSK_SMOKE_I_MEAN_IT=1` acknowledgement is required.

`kiosk-main` must be built with the pinned public key that matches the signed
fixtures below baked in via `option_env!` at **compile time** — `signature::pinned_key()`
reads `KIOSK_CONFIG_PUBKEY_B64` through `option_env!`, not `std::env::var`, so
setting the variable only for the harness run (not the build) leaves the binary
rejecting every signed config fail-closed. The pinned key for this fixture set:

```
KIOSK_CONFIG_PUBKEY_B64=ZVW08teLiFV5pIQ7YKNrMZMP8EFqJHyHcHvKYQ9Pyeo=
```

The matching private signing seed was used once, locally, to sign the static
fixtures below and then discarded — it is not committed (same discipline as
`docs/testing/p1d2-signed-config-smoke.md`'s "Security" section). Probe
scenarios generate their signed variants at runtime with the ephemeral seed;
if a static fixture changes, regenerate a key pair, sign the fixture with
`kioskctl`, and rebuild `kiosk-main` with the new public key.

`KIOSK_BIN` (or the legacy `KIOSK_MAIN`) overrides the binary path.

## Media harness

The Linux smoke/soak environment installs the runtime decoder set used by the
deployed offline page:

\`\`\`text
gstreamer1.0-plugins-base
gstreamer1.0-plugins-good
gstreamer1.0-plugins-bad
gstreamer1.0-libav
\`\`\`

The deliberate missing-decoder variant is owned by
\`packaging/soak/fixtures/no-libav.sh\`. It must run in an environment that
does not contain \`avdec_h264\`; the wrapper refuses to remove packages from a
developer host, then records the page's enumerated \`media.error\` kind while
asserting the black fallback.

## What each scenario proves

| # | Proves | Mechanism |
|---|---|---|
| 1 | Boot → splash → remote home commits, on a genuinely signed+verified config, window reaches fullscreen | boots against `fixtures/config.json`; the fixture httpd's access log is the "the browser really fetched this" oracle, `config.applied{revision:1}` in the spool is the "the signature verified and the FSM applied it" oracle |
| 2 | Off-allowlist main-frame nav is blocked exactly once; target=_blank semantics | `fixtures/home.html` drives itself (see "Why the pages drive themselves") |
| 3 | Offline fallback: a failed reload falls to the bundled offline page | stops the fixture httpd, the page's own `location.reload()` hits a closed port |
| 4 | Renderer crash → `webview.crash` spooled, recovery navigates home | `pkill -f WebKitWebProcess` against the real subprocess (confirmed via `ps` before writing this: `/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess`) |
| 5 | Iframe main-frame-scope pin: an off-allowlist iframe is blocked with no leak into the top-level FSM | `fixtures/iframe-host.html`, loaded directly as `content.url` via `fixtures/config-iframe.json` |
| 6 | Profile clear completion: the privacy gate receives exactly one completion event | `kiosk-main --example clear_probe` uses the production `clear::clear` callback |
| 7 | Malformed bootstrap enters the safe renderer without a remote fetch | `fixtures/kiosk-malformed.ini`; device id is sourced from `/etc/machine-id` |
| 8 | Native egress filter plus CSP degraded path | `hardening.html` drives off-list image/CSS/fetch/beacon/service-worker requests; the regular-file filter variant must emit `config.error` |
| 9 | Attachment downloads are cancelled before persistence | `download.html` reaches the attachment response, while the spool records `nav.blocked{reason:download}` and `/var/lib/kiosk` stays clear |
| 10 | Dialog/chrome suppression, bundled keyboard, print and beforeunload | `controls.html` plus real XTest right-click on the Xwayland floor |
| 11 | Permission default-deny and signed camera capability | `permissions.html` records geolocation/camera outcomes in denied and capability-enabled boots |
| 12 | Bus-less keep-awake degradation is non-fatal | `systemd-inhibit` child failure becomes `config.warn{field:display.keep_awake}` |
| 13 | cage → launcher → main chain and child restart | the real release launcher runs under cage; killing its main child must produce a fresh home load |
| 14 | Technician exit through the real cage chain | Xwayland chord and pin pad drive exit status 86 through cage |
| 15 | Heartbeat hang recovery and orphan reap | SIGSTOP of the supervised main child must be detected, restarted and durably logged |
| 16 | Idle expiry clears profile data once | `idle.html` sets a cookie, then observes it absent after the real clear completion |
| 17 | Gesture/chord activity backstop | the Xwayland floor driver exercises corner taps, technician chord, ordinary input and idle suppression |

## Why the pages drive themselves (no xdotool)

The brief's scenario 2/5 language ("drive a link click") assumes real input.
This container has **no `/dev/input`** — GDK itself confirms there is no seat
(`gdk_seat_get_keyboard: assertion 'GDK_IS_SEAT (seat)' failed` on every launch).
The only input-injection tool present, `xdotool`, speaks X11/XTest via Xwayland,
and XTest input synthesis **reproducibly segfaults Xwayland** in this
environment (confirmed twice, under both `desktop-shell` and `kiosk-shell`, on
the very first `mousemove`/`click`/`windowactivate` call — see
the input note below for the root cause: no
input devices ⇒ weston's headless backend creates no seat capability ⇒
Xwayland's XTest code dereferences it anyway).

So the fixture pages drive their own navigation attempts via `.click()` /
`location.reload()` on a `setTimeout` schedule. That is a genuine
`decide-policy` request for ordinary top-level navigation (link clicks and
reloads are not gesture-gated in WebKit) — not a simulation of the guard's
logic — and the harness's `wait_until` polls for the resulting spool/httpd-log
evidence rather than assuming a fixed timing.

**The one thing this cannot reach**: `window.open()`/`target=_blank` new-window
creation IS gesture-gated (`WebKitSettings:javascript-can-open-windows-automatically`,
default off, untouched on Linux since `hardening.rs` is a no-op there in P2-A),
and a script-dispatched click can never carry a trusted gesture (`isTrusted` is
immutably `false` for any script-dispatched event, by browser design). Confirmed
empirically: a scripted `.click()` on a `target=_blank` anchor produces zero
`create`-signal activity — no httpd request, no `nav.blocked`, kiosk-main
undisturbed. `on_new_window`/`handle_user_message`'s double-borrow question
(the first carry-forward — see Concerns) is therefore **not settled** by this
run; it needs a real input device.

## The boot/fetch race (why bootstrap.url must match content.url per scenario)

`ConfigManager::boot()` (`crates/kiosk-core/src/config/mod.rs`) resolves the
FIRST navigation target synchronously, from local state only — last-good (none,
on a wiped `/var/lib/kiosk`) or `[bootstrap] url` — before any network fetch has
even started. That first `AppEvent::ConfigApplied` is sent from a task spawned
inside `.setup()`. Independently, `fetch::run` is spawned much earlier in
`main()`, before `tauri::Builder` even starts, and its `tokio::time::interval`
fires its first tick immediately — so against a local fixture httpd, the whole
fetch → JCS canonicalize → Ed25519 verify → apply → `tx.send` round trip was
observed completing in **~14ms**, plausibly before `.setup()` (real GTK/WebKit
window construction) has even run. Both `AppEvent::ConfigApplied` sends land in
the driver's channel, in whichever order won that race, and the driver
processes both — but when their URLs differ, that is *two* real
`window.navigate()` calls in quick succession, and the second cancels the
first's in-flight load before it ever reaches the network. Confirmed
empirically: pointing `content.url` at `iframe-host.html` while `[bootstrap] url`
stayed on `home.html` produced **zero** `GET /iframe-host.html` in the fixture
httpd's access log even after three minutes — the boot navigation to
`home.html` always won, and `home.html`'s own script (not any iframe) was what
produced the one `nav.blocked` that made this failure look, at a glance, like a
passing iframe test.

This is a fixture-specific race, not a `kiosk-main` bug: production points
`config_url` at a real remote host, where network latency alone would put the
fetch's completion safely after boot's own navigation; only a local, near-zero-
latency loopback fixture makes the two land close enough to interleave.
`kiosk-reload.ini`/`kiosk-iframe.ini` fix this the direct way — make
`[bootstrap] url` equal that scenario's signed `content.url`, so either the
first navigation already lands on the intended page, or a same-URL
`ConfigApplied` arriving later is a documented no-op (`state.rs`'s `Online +
ConfigApplied` arm) rather than a second, colliding navigate.

## Fixtures

- `fixtures/home.html` — scenario 2/3's content: self-drives an off-allowlist
  click, a target=_blank-allowed attempt, and a target=_blank-blocked attempt
  (`?probe=reload` query switches it to scenario 3's self-reload instead).
- `fixtures/allowed-target.html`, `fixtures/iframe-allowed.html` — trivial
  in-allowlist landing pages.
- `fixtures/iframe-host.html` — scenario 5's content: one in-allowlist iframe,
  one off-allowlist iframe (`http://evil.test/...`, never served — the guard
  must cancel before any DNS attempt, which it does: no external network is
  ever touched).
- `fixtures/kiosk.ini`, `fixtures/kiosk-reload.ini`, `fixtures/kiosk-iframe.ini`
  — three bootstrap configs; `config_url` always points at
  `http://localhost:8099/config.json`, which the harness serves from a
  refreshable staging copy of `fixtures/`, swapping in whichever signed variant
  the current scenario needs via `stage_config`. `[bootstrap] url` in each ini
  is **deliberately set to the same page as that scenario's signed
  `content.url`** — see "The boot/fetch race" below for why that has to match
  rather than differ.
- `fixtures/config.json`, `fixtures/config-reload.json`, `fixtures/config-iframe.json`
  — three genuinely signed configs (all `revision: 1`, all `device_id: smoke-01`
  matching each ini's `[kiosk] device_id`), differing only in `content.url`.
  **Three config variants and three ini variants, not one-of-each** — the extra
  variants keep the bootstrap URL aligned with each probe
  (no click mechanism exists to chain scenario progression through a single
  config the way the brief's design implicitly assumed, and the boot/fetch race
  below forced the ini variants specifically).
- `fixtures/kiosk-credential.json` — a syntactically well-formed but fake
  service-account JSON. `ServiceAccount::from_json` only checks the three
  fields are non-empty (no PEM validation at boot or at telemetry-build time —
  verified by reading `crates/kiosk-core/src/logging/auth.rs`); real RSA
  signing is only attempted at flush time, in the background, and fails
  harmlessly (this sandbox's outbound proxy may or may not even reach
  `oauth2.googleapis.com` — irrelevant either way, since every scenario
  assertion reads the on-disk spool, which `Logger::log` populates
  synchronously and independently of upload success).
- `fixtures/kiosk-offline.mp4` — a placeholder byte blob, not a real decodable
  video. Sufficient to exercise the wiring (the `kioskasset://` custom-protocol
  handler serving real bytes instead of 404) — playback quality is explicitly
  P2-E's scope, not P2-A's (design spec, Scope/defer).

## Compositor floor

`run-smoke.sh` uses weston's default `desktop-shell` for scenarios 1–12 and
16–17. The geometry check tolerates its panel offset and asserts only the
window's X coordinate and output width/height. Scenarios 13–15 stop weston and
run the real launcher under `cage --` with `WLR_BACKENDS=headless`; scenario 14
uses cage's Xwayland backend for the declared xdotool floor driver.

## tao's observed monitor behavior under weston (closes the spec's open decision)

Weston's headless backend advertises exactly one output at the configured
`--width`/`--height` (1280×720 here). `content.display.monitor` defaults to 0,
so `resolve_monitor_index(0, 1)` resolves to `Some(0)` and `main.rs` positions
the window on that one monitor — the `config.warn{display.monitor}` fallback
path (index ≥ count) is never exercised by this fixture set, since there is
only one output to be out-of-range against. Two observations on top of that:

- **This measurement was taken through Xwayland (`GDK_BACKEND=x11`), not GTK's
  native Wayland backend.** No native-Wayland window-geometry introspection
  tool was available in this container (no `wlr-randr`/similar; `weston-info`
  is not installed) — Xwayland was the only way to make the window
  externally inspectable at all, and it was used *only* for this read-only
  geometry query (never for input — see "Why the pages drive themselves").
  kiosk-main was also smoke-tested booting under genuinely native Wayland (no
  `DISPLAY`, no `GDK_BACKEND`) during this task's exploration and it boots and
  runs identically (same `app.start`/`config.applied` sequence) — but its window
  geometry could not be independently measured in that mode with the tools at
  hand. Xwayland's `available_monitors()` result comes from GDK's X11/XRandR
  backend as exposed by Xwayland-under-weston, which is not necessarily
  identical code inside tao to the native `wl_output`-enumeration path a real
  deployment (no Xwayland at all) would take — both report "one monitor" here,
  which is the only fact this fixture set can distinguish either way.
- No multi-monitor headless output was tested (weston's headless backend takes
  one `--width`/`--height` pair; multiple `--width`/`--height`-style outputs
  were not attempted in this task, since neither the brief nor the design spec
  asked for a specific multi-output configuration to verify the fallback path
  against). A future task that wants the `config.warn{display.monitor}`
  fallback genuinely exercised needs a headless setup with 2+ outputs.

## Mutation evidence (what proves these scenarios are not vacuous)

A green scenario only means something if it can go red. Two of the P2-A
scenarios were found, in adversarial review, to pass for reasons that had
nothing to do with what they were named for, and a third assertion turned out to
prove less than its own source comment claimed. Each was checked by mutating the
app and re-running; a scenario that stays green under its own mutation is not a
test.

| Mutation | Expected | Observed |
|---|---|---|
| delete `window.set_fullscreen(true)` (`kiosk-main/src/main.rs`) | scenario 1 RED | RED — window came up `800x600` at `(32,32)` against an asserted `1280x720` at `X=0`; three checks failed |
| `PageTarget::Offline => {}` — offline page never shown (`kiosk-main/src/main.rs`) | scenario 3 RED | RED — but **only** on the offline-page check below |
| force `is_policy_cancellation` to `false` (`kiosk-main/src/nav.rs`) | scenario 5 RED | **GREEN** — see below |

Mutations were reverted; `main.rs` and `nav.rs` are unmodified.

The third row is the interesting one, and it corrects a claim the source used to
make about itself. `nav.rs` documents an assumption its failure latch rests on —
that `load-changed`/`load-failed` track the main frame only — and said scenario 5
pinned it observationally, on the grounds that a blocked off-allowlist iframe
produces no `nav.error`. Disabling the policy-cancellation filter outright shows
that reasoning does not hold: scenario 5 stays green with `nav.error == 0`, and
so does scenario 2, whose block is a **main-frame** navigation. A guard-cancelled
load raises no `load-failed` on the WebView at all on WebKitGTK 2.52.3, in either
frame. "No `nav.error` from the blocked iframe" was therefore equally consistent
with "the signal fired and was filtered" — it pinned nothing. `nav.rs`'s module
doc now records what was actually measured, and marks the filter as defensive
rather than load-bearing.

**Scenario 1** previously ran weston with `--shell=kiosk-shell.so`, which forces
every top-level surface to the output's full extent whether or not the client
ever asked to be fullscreen. The geometry check therefore passed identically
with the app's fullscreen request deleted — it was measuring the compositor.
Running under the default `desktop-shell` (see "Compositor floor") is what makes
it measure the app; the panel offset that costs us is why `Y` is logged rather
than asserted.

**Scenario 3** is the more instructive one. With the offline page disabled
outright, every other assertion in the scenario still passed — `nav.error`,
`net.offline`, the process staying alive, and the rule-4 re-navigate on
reconnect. All of them are produced by paths that never touch the offline page,
so the scenario would have passed on a device that black-screens on network
loss. The check that closes it:

```
check "offline page rendered (its kioskasset:// video request reached the handler)"
```

`offline.html` computes its `<video>` src page-locally from `location.protocol`,
which on Linux resolves to `kioskasset://localhost/kiosk-offline.mp4`, served by
kiosk-main's own custom-scheme handler via `std::fs::read` of that exact file.
An inotify watch on the file is therefore external proof of **both** facts at
once: the FSM really navigated to the bundled offline page, and that page picked
the Linux origin — had it picked the Windows spelling
(`http://kioskasset.localhost/...`) WebKit would have treated it as an ordinary
HTTP host and the handler would never have run.

This check needs `inotify-tools`. Without it the scenario records the sub-check
as BLOCKED rather than passing silently — which matters, because without it
scenario 3 passes even when the offline page is never shown at all.

## Egress: two measured residuals (P2-B)

The P2-B design left one question open for runtime to answer and made one claim
about the degrade path. Both were measured on WebKitGTK 2.52.3; the answers are
recorded here because the spec asked for exactly that ("recorded in this spec
*before merge*, not discovered in the field").

**1. Host-scoped blocks are enforced but silent.** A resource the native content
filter blocks never reaches `resource-load-started`, so the observer that emits
`nav.blocked{egress}` (`egress.rs`, `connect_resource_load_started` →
`WebResource::connect_failed`) never runs for it. Measured by instrumenting both
callbacks: in the healthy arm the four off-list URLs never appear at all; with
the filter absent, all four appear and all four report. So that telemetry exists
only when enforcement is *absent* — the signal and the enforcement are mutually
exclusive.

Consequence for operators: **on a working Linux kiosk, blocked subresource egress
produces no telemetry.** Zero `nav.blocked{egress}` is the healthy reading, not
evidence that nothing was blocked. Scenario 8 therefore asserts enforcement from
outside the process (see below) and asserts the observer only in the degraded arm.

**2. The CSP belt does not block off-list egress.** The design's degrade path
states that with the native filter unavailable "an off-list `fetch()` is still
blocked by the belt". It is not. Scenario 8's degraded arm now loads an off-list
subresource from a *served* host and the fixture httpd records the GET — the
request goes out. The belt injects its policy as a `<meta http-equiv>` element
from a document-start user script, and a meta CSP only governs resources fetched
after that element is inserted; the page's own subresources are already in flight
by then, and re-assigning `.content` on an existing meta element has no effect.

Consequence: when `config.error{egress.filter_absent}` fires, Linux has **no**
subresource egress enforcement — not a weaker one. Scenario 8's last check is
deliberately left RED rather than downgraded, because closing this needs a real
mechanism (a response-header rewrite from a web-process extension), which is
design work rather than a patch.

**How enforcement is asserted instead.** Every other off-list URL in
`hardening.html` points at `evil.test`, which never resolves — its absence from
the access log proves nothing. `http://127.0.0.1:8099/off-list-probe` is
off-allowlist (the allowlist names host `localhost`) but *is* served, so its
absence is real evidence. The two arms falsify each other: with the filter on the
GET never lands, with the filter gone it does.

## Concerns / carry-forwards

The Xwayland/xdotool limitation is a declared floor-driver constraint; native
touch validation remains a P2-G hardware checklist item.
