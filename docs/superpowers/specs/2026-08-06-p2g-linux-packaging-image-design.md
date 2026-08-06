# P2-G — Linux Packaging, OS Image Runbook, Hardware Validation (Design)

> Seventh and final sub-project of P2. Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §4 (paths), §7.2
> (Linux OS lockdown), §9, §10 (RT-05, escape-vector sweep). **Sibling precedent is
> P1-F2** (`2026-08-02-p1f2-packaging-deployment-design.md`): installer owns app-scoped
> setup, a lockdown *runbook* owns OS hardening, secrets never ship in the package.
> G consumes C's unit contract and B/D/E's accumulated hardware-checklist items.

**Status:** draft, 2026-08-06 (awaiting review). Approach approved in-session:
runbook-on-stock-pinned-Debian-12 (automated image build is a recorded ponytail) —
exact parity with how Windows shipped (MSI + `lockdown.md`, no golden image).

## Goal

An operator can take stock Debian 12, follow one runbook, install one `.deb`, drop one
credential + one `kiosk.ini`, and have a locked kiosk that survives reboot — with a
hardware validation checklist that retires every "deferred to hardware" item A–E
accumulated. Target hardware remains TBD (session decision: design to the spec floor —
Debian 12 / Ubuntu 22.04, x86_64); the checklist is hardware-parameterized, not
hardware-blocked.

## Components

### 1. `.deb` — `packaging/linux/`

Mirrors F2's MSI scope disciplines exactly:

- **Payload:** `kiosk-main` + `kiosk-launcher` → `/usr/libexec/kiosk/`; bundled assets
  alongside (the `bundled/` pages + optional `kiosk-offline.mp4` — user-replaceable per
  §3.4, marked conffile-adjacent so upgrades don't clobber a replaced video);
  `kiosk.service` (C's contract shape + G's values: `StartLimitIntervalSec`/
  `StartLimitBurst` chosen here, `RuntimeDirectory=kiosk`,
  `RestartPreventExitStatus=86`); `kioskctl` for the signing workflow.
- **Dependencies:** `libwebkit2gtk-4.1-0`, `libgtk-3-0`, `cage`,
  `gstreamer1.0-plugins-base`, `gstreamer1.0-plugins-good`,
  `gstreamer1.0-plugins-bad`, `gstreamer1.0-libav` — the GStreamer four are verbatim
  parent §3.4 (a missing element is a silent black video; the dependency line is the
  first defense, E's watchdog the second).
- **State dirs:** postinst creates `/var/lib/kiosk` (`0750 root:root`) — parent §4's
  path, A's `resolve_data_dir`.
- **Secrets discipline (F2 verbatim):** the package ships a `kiosk.ini` *template* and
  **no credential**. postinst pre-creates the credential path `0600 root:root` empty so
  the mode exists before the secret does — the package makes SEC-09's required mode the
  default state, exactly as the MSI makes the DACL the default (F2 §1). A/C's
  `credential_is_owner_only` then enforces what the package established.
- **Autostart:** `systemctl enable kiosk.service` in postinst (the Scheduled-Task
  analogue); `deb-systemd-invoke` conventions so policy-rc.d environments behave.
- **Versioning:** version from the workspace; `Conflicts`/`Replaces` unused (single
  package); upgrades preserve `kiosk.ini`, credential, and a replaced offline video.

### 2. Lockdown runbook — `packaging/linux/lockdown.md`

The §7.2 Linux row, expanded to runbook steps on stock Debian 12 (F2's `lockdown.md`
precedent — OS hardening is *documented and verified*, not postinst-automated, because
half of it is judgment calls an integrator must own):

- cage locked session as the supported secure config; **X11/openbox stays demo-only,
  documented as NOT app-enforced** (parent §7.2 verbatim — one appendix paragraph, no
  more).
- VT/console: `NAutoVTs=0`, `ReserveVT=0`, no getty on the kiosk seat, kernel
  `consoleblank=0`; the D spec's note lands here — chord *swallowing* is unnecessary
  under cage, VT switching is what actually needs killing, and it dies in logind, not
  in app code.
- Sleep/idle: mask `sleep.target suspend.target hibernate.target hybrid-sleep.target`;
  `IdleAction=ignore` (B's keep-awake suspenders — the `systemd-inhibit` child is the
  belt); no screensaver/idle daemon installed.
- Seat/session: the **service-user and seat-access decision is the runbook's one open
  fork** (below); both candidate recipes are written up, one gets promoted after
  hardware validation.
- Boot cosmetics: quiet boot (`quiet` + disabled cursor blink), journald size caps.
- Updates: `unattended-upgrades` off (F §4 — update timing is operator-owned);
  the WebKitGTK pin: hold `libwebkit2gtk-4.1-0` at the validated version
  (`apt-mark hold`), release the hold only through the runbook's revalidation loop
  (parent §9's pinned-image intent, expressed as a package hold on stock Debian).
- SSH: keyed-only if enabled; default recipe leaves it absent.

### 3. OS image position

Runbook-on-stock-Debian **is** the P2 image story (approved). The runbook ends with a
capture step: `dpkg -l` snapshot + config diffs archived per validated device class —
that snapshot is the "pinned image" §9 speaks of, in reproducible-enough form for a
small fleet. Automated image building (preseed/FAI/debos) is the recorded ponytail,
promoted only when fleet size makes hand-run runbooks the bottleneck.

### 4. Hardware validation checklist (retires the deferred list)

Parameterized on device class; every item cites its origin:

| # | Item | Origin |
|---|---|---|
| H1 | Real cage boot chain: service → cage → launcher → main; fullscreen on the physical display; monitor-placement behavior recorded | A (Wayland monitor semantics open item) |
| H2 | `RestartPreventExitStatus=86` end-to-end via systemctl (technician exit stays exited) | C smoke 14's systemd half |
| H3 | Keep-awake positive: `systemd-inhibit --list` shows the hold; display never blanks over 24 h | B smoke 12's deferred half |
| H4 | Touch: corner-tap gesture on real touch hardware; `GDK_TOUCH_CANCEL` behavior; on-screen keyboard decision (squeekboard/onboard per §7 table) exercised and chosen | D (smoke 17 if headless virtual input was unavailable; §7 keyboard row) |
| H5 | ≥72 h offline-video soak, RSS trend, loop count; visual black-frame check | E / RT-05 |
| H6 | §7.2 escape-vector sweep under the locked session (the §10 hardening list: chords, edges, dialogs, VT attempts) | B/D + §10 |
| H7 | Egress + nav guard against a real network (DNS failure modes, captive-portal-ish interference) | A/B |
| H8 | Runbook executed cold on the device class, timed; `dpkg -l` snapshot captured | G §2-3 |

Checklist results append to the runbook per device class — the validation *is* the
image pin.

## Testing

- `.deb` assembly + install/remove/upgrade cycle in a `debian:12` container (F's
  release job runs assembly; the install cycle test lives with G): postinst modes
  (`/var/lib/kiosk` 0750, credential path 0600), service enabled, template-not-secret
  invariant (package contains zero real keys — greppable assertion), upgrade preserves
  the three operator-owned files.
- Lintian clean (or documented overrides — a kiosk package legitimately does things
  lintian side-eyes).
- The runbook is testable prose: every step ends with a verify command; H8 is the
  integration test.

## Error handling / edge cases

Mirrors F2's section shape: install on a system with the service already running
(upgrade path: `deb-systemd-invoke try-restart` after unpack); missing WebKitGTK
runtime version vs the hold (dependency solver handles; the runbook's hold step
documents the downgrade-refusal case); credential present-but-wrong-mode after manual
operator edits (postinst re-asserts mode on upgrade; A/C's runtime gate is the real
enforcement); disk-full on `/var/lib/kiosk` (the spool's existing degradation, nothing
package-level to add).

## Open decisions to resolve at plan time (or at hardware, explicitly)

- **Service user + seat access (the real one):** root-service (simplest DRM access,
  weakest posture) vs dedicated `kiosk` user with logind seat semantics vs `seatd`.
  Both non-root recipes drafted in the runbook; hardware validation (H1) promotes one.
  The spec's lean: dedicated user + logind, root only if the device class forces it —
  but this is decided by evidence at H1, not by preference here.
- `--safe` boot chain interaction with `StartLimitBurst` numbers (the launcher already
  owns safe-mode; systemd's limits must be strictly looser so the FSM, not systemd, is
  always the authority that gives up first — numbers picked with C's timing constants
  side by side).
- Whether the offline mp4 ships in the `.deb` or the runbook places it (size vs
  completeness; lean: ship the default, conffile-protect the replacement).

## Scope / defer

Automated image build (ponytail, promoted by fleet size); apt repo/fleet update
mechanics (F's ponytail); Android packaging (P3); target-hardware selection (explicitly
TBD — the checklist is ready for whatever the answer is).
