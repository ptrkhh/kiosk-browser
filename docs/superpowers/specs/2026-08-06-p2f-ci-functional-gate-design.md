# P2-F — CI Functional Gate, Scheduled Endurance, Release Artifacts (Design)

> Sixth sub-project of P2. Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §10 (CI matrix:
> "Linux compile check (P0 → functional at P2)"; soak/endurance rows). **Builds on the
> A–E smoke harness** (weston headless + signed-config fixtures + spool assertions) and
> C's cross-platform RT-13. F writes no product code: it promotes the harness from
> human-run-in-session to CI-run, and wires artifact/release plumbing.

**Status:** draft, 2026-08-06 (awaiting review). Approach approved in-session: per-PR
fast-smoke subset + scheduled full/endurance runs; update path = install-the-new-package
parity (no auto-updater in P2, matching Windows).

## Goal

Every PR gets the Linux functional gate §10 promised at P2 — the real app under a real
compositor with real signed config, in minutes. Long-running validation (full matrix,
video soak) runs on schedule, on a target-faithful image. Tags produce installable
artifacts for both platforms.

## Current state (what F changes)

`ci.yml` today: `lint-test` (ubuntu-22.04: fmt, clippy `-D warnings` workspace, full
test suite, `cargo check -p kiosk-main`), `build-windows`, `build-linux` (release
binaries as artifacts). After C lands, `cargo test` already includes RT-13 — the
supervise loop is a per-PR gate with zero F work. F adds the webview-level gate on top.

## Components

### 1. Per-PR job: `smoke-linux`

ubuntu-22.04 (matching `lint-test`'s image and the webkit 4.1 dev deps already
installed there), plus `weston`, the four GStreamer packages (E §5), and the fixture
tooling. Runs the **fast subset**: A scenarios 1–3, 5, 7 (boot/nav/offline/iframe/safe),
B 8–11 (egress/downloads/dialog/permissions), D 16 with a seconds-scale idle threshold.
Excluded from per-PR, by design: A 4 (crash-kill timing-flaky candidate — scheduled
until it proves stable), C 13–15 (RT-13 covers the same FSM paths per-PR; the
cage-chain variants are scheduled), E 18 (soak is never per-PR). Wall-clock budget:
**under 10 minutes or the subset shrinks** — a gate developers route around is worse
than a smaller gate. Failure artifacts: spool + weston/compositor logs + (best-effort)
screenshots, always uploaded on failure.

### 2. Scheduled workflow: `endurance`

Nightly cron, two jobs: (a) **full matrix** — all A–D scenarios including the per-PR
exclusions, run in a **`debian:12` container** (target fidelity: the distro's actual
WebKitGTK/GStreamer, not Ubuntu's — the closest CI gets to the pinned image before
hardware); (b) **soak** — E's protocol at 8 h+, same container, RSS series retained as
an artifact even on pass (trend data is the point, per §10's "assert bounded RSS").
Scheduled failures notify via the repo's normal mechanisms; a red nightly does not
block PRs but is a release blocker (see 3).

### 3. Release-on-tag: `release`

Tag push `v*`: existing release builds, plus `.deb` assembly from P2-G's
`packaging/linux/` (dpkg-deb; the package *content* is G's spec, F only executes it),
plus the Windows MSI if/when F2's WiX build is CI-wired (F2 built it locally; wiring is
the same one-job shape — included here for symmetry, not new design), checksums, and a
draft GitHub release holding all artifacts. Gate: release requires the latest
`endurance` run green — the §10 soak rows exist precisely to stop a leaking build from
shipping to devices that reboot once a month.

### 4. Update path — the parity statement

Windows P1/P2 ships no auto-updater: update = install the new MSI. Linux matches:
update = install the new `.deb` (`dpkg -i` / operator tooling). Devices tolerate the
restart by construction (spool durability, launcher exit/restart semantics). Recorded
ponytails, deliberately not P2 scope: an apt repository, `unattended-upgrades` policy,
delta/A-B updates. The G runbook pins `unattended-upgrades` **off** so the fleet's
update timing stays operator-owned either way.

## Testing

F's product *is* test infrastructure; its own verification is meta: the smoke job must
fail when a scenario fails (one deliberately-broken fixture run recorded in the PR that
lands it — the classic does-the-gate-gate check), and the release job must refuse a tag
on a red endurance run (same negative-path proof). Both proofs are one-time,
documented in the landing PR, not permanent fixtures.

## Error handling / flake policy

A failed smoke scenario retries **once** within the job (compositor startup races are
the realistic flake class); a pass-on-retry is reported as pass **with a `flaky` line
in the job summary** — silent retry-laundering is how gates rot. Two flakes of the same
scenario in seven days → the scenario moves to `endurance` and a tracking issue is
opened (quarantine with a paper trail, never deletion). Runner-environment failures
(apt mirror down, weston won't start at all) fail the job distinctly from scenario
failures so the signal stays clean.

## Open decisions to resolve at plan time

- Whether `smoke-linux` shares `lint-test`'s job (serial, one env setup) or stands
  alone (parallel, duplicated setup) — decide on measured wall-clock of the first
  working version.
- Artifact retention windows (spools are small; RSS series smaller — likely defaults
  suffice, decide with real sizes).
- Cron time + whether the soak job and full-matrix job share the nightly or split
  across alternating nights (runner-minutes budget question — measure first).
- `debian:12` container + weston headless inside GH Actions: confirm no
  seat/device-node surprises (the A harness runs as root in-session; CI containers
  differ — pin with a probe run before building the whole job on it).

## Scope / defer

Package contents, lockdown runbook, image pinning, hardware checklist → P2-G. Fleet
update mechanics → recorded ponytail above. Android CI rows (§10) → P3.
