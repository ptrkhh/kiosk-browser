# P2-E — Offline Video on WebKitGTK: Proof, Soak, and the Endurance Set (Design)

> Fifth sub-project of P2. Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.4 (offline
> video, arch-09/arch-10), §10 (soak/endurance, PF-05). **Builds on P2-A** (the offline
> page renders, asset origin serves) and **P2-B** (egress admits the asset origin by
> construction). E builds almost no player logic — `bundled/offline.html` already carries
> arch-09's full failure wiring (`stalled`/`emptied` handlers `:52-56`, `play()` rejection
> `:59-62`, the 3 s progress watchdog `:65-67`, static-splash degrade). E *proves* that
> path on WebKitGTK, closes its one recorded gap, and picks up the parent roadmap's P2
> endurance items.

**Status:** draft, 2026-08-06 (awaiting review).

## Goal

The offline video loops for hours on WebKitGTK with zero silent failure modes: decode
failures degrade visibly and *observably* (spooled, not console-only), loop boundaries
don't stall (Debian #1062012 / PF-05 is the named risk), and memory stays bounded — with
the parent-approved contingency designed up front rather than improvised mid-soak.

## Scope

**In:** the `media.error` IPC bridge (closing `offline.html:44-47`'s recorded gap);
the soak harness + pass criteria; GStreamer decode chain in the harness environment;
**memory-cap restart + health-sampled RSS** — the parent roadmap assigns these to P2
verbatim (§10: "P1 nightly reload; P2 adds memory-cap restart + health-sampled RSS;
validated by scheduled soak") and E is their natural owner, being the endurance
sub-project; the double-buffered-loop contingency *design*. **Out:** the ≥72 h hardware
soak (RT-05 — P2-G's checklist), scheduled-CI wiring of the soak (P2-F), `.deb` GStreamer
dependency declaration (P2-G, list fixed by parent §3.4).

**Two deliberate cross-platform changes, flagged as such** (everything else in P2 is
Linux-gated): the `media.error` bridge and the RSS/memory-cap feature land on Windows
too. Both are additive completions the parent spec assigns — arch-09's "emit a
`media.error` log" was never fully delivered on any platform, and the memory-cap is a
named P2 roadmap row — not scope creep smuggled through a port.

## Components

### 1. `media.error` bridge

A Tauri command (same page→telemetry pattern as B's CSP-violation reporter; registered
cross-platform — the page and its failure modes exist on both platforms) through which
`offline.html`'s existing `fallback(why)` reports before degrading. Telemetry event
`media.error` (WARNING) per parent §3.4/§6; rate-capped by the standard Logger bucket.
`offline.html`'s `:44-47` comment is discharged in the same change. The page keeps
degrading even if the invoke fails — telemetry is observation, never a dependency
(the A/B doctrine).

### 2. Loop-boundary self-monitoring

The 3 s watchdog covers startup only. E extends the page's monitor: every few seconds,
assert `currentTime` advanced since the last check (except while intentionally paused
by degrade); a stall → one `media.error{kind: "stall", at: currentTime}` + degrade per
arch-09. This makes the soak's core assertion *self-reporting*: the harness asserts
**zero `media.error` in the spool** over the soak window, rather than screen-scraping.
Black-frame detection via screenshot stays best-effort (recorded limit of the headless
harness — the visual check is a hardware-checklist item).

### 3. Health-sampled RSS + memory-cap restart

`health.rs` gains an RSS sample per existing poll tick (`/proc/self/status` `VmRSS` on
Linux; `GetProcessMemoryInfo` on Windows), surfaced through the existing metrics
pipeline (D2e). New config key `memory_max_mb` (schema section placed at plan time
beside its consumers; default **0 = off** — Windows fleets see zero behavior change
until an operator opts in). The cap decision is a pure,
host-tested latch: N consecutive samples over the cap → one `health.memory_cap` event →
clean process exit with a dedicated restart exit code — **never 86** (C's signal-death
invariant extends: no self-restart may read as a technician exit) — and the launcher's
existing FSM restarts it. No launcher change at all: a memory-cap exit is just a
`ChildExited` with a greppable code.

### 4. The contingency, designed now (PF-05)

If the soak shows loop-boundary stalls (the `#1062012` class — seek-to-0 on `loop`),
the parent-approved fallback is built as a conditional task: **seamless double-buffered
loop** — two stacked `<video>` elements, both preloaded; on `ended` (or
`timeupdate` ≥ duration−ε) the hidden one — already at 0 and paused-ready — plays and
swaps to front; the other resets in background. No visible element ever seeks. Trigger
rule is mechanical, not judgment: any stall `media.error` at a loop boundary during the
soak → the contingency task activates in the plan. The native-GL path (parent §3.4's
second fallback) stays out unless the double-buffer also fails on hardware — it
forfeits the one-HTML-path property and needs its own design round.

### 5. Harness environment

The four GStreamer packages parent §3.4 names for the `.deb`
(`gstreamer1.0-plugins-{base,good,bad}`, `gstreamer1.0-libav`) are installed in the
smoke/soak environment — a *missing* element must also be exercised once (remove
`libav`, assert the silent-black case is caught by the progress watchdog and spooled,
not silent — that assertion is the whole reason arch-09 exists).

## Soak protocol (scenario 18)

Fixture: config-down boot → offline video (A scenario 3 entry path). Durations:
**in-session** ~2 h minimum during execution (background task while other work
proceeds); **scheduled CI** 8 h+ (wired by F, run in a `debian:12` container for
target fidelity); **hardware** ≥72 h (G checklist, RT-05). Pass: zero `media.error`,
process alive, no launcher restarts, RSS delta over the window under a declared bound
(number picked at plan time from first-run baseline, then pinned), loop count consistent
with wall-clock (catches a silent freeze the page monitor somehow missed). Fail
artifacts: full spool + weston log retained.

## Testing

- **Host tests:** memory-cap latch (consecutive-sample semantics, 0-disables, never-86
  code); `VmRSS` parse; the page monitor's stall predicate extracted pure if practical
  (else covered by smoke 18's deliberate-stall fixture: a truncated mp4 must produce
  exactly one stall report + degrade).
- **Smoke:** 18 (short form per-PR is NOT run — soak is scheduled/in-session only; the
  per-PR video assertion stays A's scenario 3 render check + the missing-decoder case).

## Error handling

arch-09 is the doctrine: every media failure degrades to the static splash + one spooled
event; the bridge failing degrades to console (today's behavior) — never a dependency;
memory-cap exit is clean (spool flushed by the existing shutdown path) so the restart
never loses the event that explains it.

## Open decisions to resolve at plan time

- Restart exit code value for the memory cap (coordinate with C's signal-mapping table
  so the launcher's `watchdog.*` log renders both distinctly).
- RSS bound derivation for pass/fail (baseline + margin vs absolute cap — pick after the
  first in-session soak's numbers exist).
- Whether the loop-boundary monitor uses `ended`+`timeupdate` or `requestVideoFrameCallback`
  (WebKitGTK support for the latter — check at plan time, don't assume).

## Scope / defer

`.deb` gst deps + hardware soak + visual checks → P2-G; scheduled-CI soak job → P2-F;
native-GL fallback → only on double-buffer failure, own design round.
