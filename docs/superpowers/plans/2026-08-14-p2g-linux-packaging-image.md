# P2-G — Linux Packaging, Lockdown Runbook and Hardware Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST:** Debian 12 (or a `debian:12` container) for every packaging task. **G ships no change under `crates/`.**

**Goal:** An operator takes stock Debian 12, follows one runbook, installs one `.deb`, provisions three files the package deliberately does not ship, and has a locked kiosk that survives reboot — plus a hardware validation checklist that retires every "deferred to hardware" item A–E accumulated.

**Architecture:** Binaries and bundled assets under `/usr/lib/kiosk/`, operator files under `/etc/kiosk/`, joined by the already-shipped `--config` flag. **Zero conffiles** and all three operator files absent by default, each failing closed. Maintainer scripts are debhelper's canonical autoscripts **verbatim** — that single decision closes four separate defects at once.

**Tech Stack:** dpkg tooling (`dpkg-shlibdeps` → `dpkg-gencontrol` → `dpkg-deb -b`), lintian, systemd, cage, `deb-systemd-helper`/`deb-systemd-invoke`.

**Spec:** `docs/superpowers/specs/2026-08-06-p2g-linux-packaging-image-design.md` (rev 2 + rev 2.1 owner decision)

**Depends on:** P2-A, P2-B, P2-C, P2-D, P2-E. **G lands after C, B, D and E, and before F.**

## Global Constraints

- **Every cage claim is version-stamped.** cage **0.1.4-4** is the C7 floor (Debian 12); cage **0.1.5** is what was run in-session and what P2-C measured against. Where the two differ, give both.
- **`cage -v`, never `cage --version`** — the latter exits 1 with `invalid option -- '-'` and aborts a script under `set -e` (ruling R3).
- **Install layout (G1), as amended into parent §4 by ruling R1:**
  ```
  /usr/lib/kiosk/       kiosk-main, kiosk-launcher, bundled/*.html, kiosk-provision-credential
  /usr/share/kiosk/     kiosk.ini.example        (not a conffile, never operator-edited)
  /lib/systemd/system/  kiosk.service
  /etc/kiosk/           kiosk.ini, kiosk-credential.json, kiosk-offline.mp4   ← all three ABSENT from the package
  /var/lib/kiosk/       cache, spool, last-good  (0750, created first-install only)
  ```
  Not `/opt` — it trips lintian `dir-or-file-in-opt` at **severity: error**, which G's own `lintian --fail-on error` gate would fail. Not `/usr/libexec` — `grep -n libexec policy.txt` returns nothing, so it has no Policy standing at all.
- **Hard dependency on P2-C (INT-9):** C5's Linux `spawn_main` **must** carry `--config`. Without it `kiosk-main` resolves its config dir to `/usr/lib/kiosk`, finds none of the three operator files, and boots into safe mode. Declared on both sides.
- **Zero conffiles.** A conffile under `/usr` is `file-in-usr-marked-as-conffile` at severity error, and a *modified* conffile aborts a non-interactive `dpkg -i` — and `kiosk.ini` is the one file **100 % of devices must edit**.
- **The example goes in `/usr/share/kiosk/`, NOT `/usr/share/doc/`.** `/etc/dpkg/dpkg.cfg.d/excludes` carries `path-exclude=/usr/share/doc/*`, so a file placed there is **silently dropped on install while `dpkg -L` still lists it**.
- **`kioskctl` is withdrawn from the payload.** Its module doc names `KIOSK_SIGNING_KEY_B64`: it carries the **fleet private signing seed's** tool, which belongs on a CI/ops host, not on a device an attacker can physically remove.
- **Every runbook step ends with a verify command.** That is the runbook's discipline and **H8 is its integration test.**

## File Structure

| File | Responsibility |
|---|---|
| `packaging/linux/tree/` | the payload as it lands on disk |
| `packaging/linux/DEBIAN/{control.in,postinst,prerm,postrm}` | control template + debhelper autoscripts verbatim |
| `packaging/linux/kiosk.service` | the **installed** unit — values, `[Unit]`, `[Install]` |
| `packaging/linux/kiosk-provision-credential` | three lines that set the mode |
| `packaging/linux/kiosk.ini.example` | ships to `/usr/share/kiosk/` |
| `packaging/linux/debian/source/lintian-overrides` | one comment per override; **no `/opt` entry** |
| `packaging/linux/lockdown.md` | the runbook |
| `docs/testing/linux-hardening-checklist.md` | H1–H11 |
| `packaging/linux/build-deb.sh` | the three-tool assembly, invoked by F's release job |

---

### Task 1: The payload tree and the control file (G1, G2, G3, G9)

**Files:**
- Create: `packaging/linux/tree/**`, `packaging/linux/DEBIAN/control.in`, `packaging/linux/build-deb.sh`, `packaging/linux/debian/source/lintian-overrides`

**Interfaces:**
- Produces: a `.deb` whose assembly F's release job invokes as `dpkg-shlibdeps` → `dpkg-gencontrol` → `dpkg-deb -b`

- [ ] **Step 1: Lay out the payload**

Two binaries, the five bundled pages, the unit, the example config and the provisioning helper — exactly the layout above. **No `kioskctl`.**

- [ ] **Step 2: Write `control.in`**

```
Depends: ${shlibs:Depends}, cage, gstreamer1.0-plugins-base, gstreamer1.0-plugins-good,
         gstreamer1.0-plugins-bad, gstreamer1.0-libav
Conflicts: swayidle, xautolock, xscreensaver, light-locker, gnome-screensaver, xfce4-power-manager
```

The library half is **derived from the built ELF binaries by `dpkg-shlibdeps`**, which is per-floor-correct by construction and makes the `libgtk-3-0` / `libgtk-3-0t64` (Ubuntu 24.04 `time_t`) alternation moot — **that residual is withdrawn, it no longer exists.**

The four GStreamer names stay **hand-written** because they are runtime *plugin* packages no ELF scan can see, and because they are parent §3.4 verbatim. `cage` likewise.

**No keyboard `Depends:`, stated explicitly** — no installable OS package solves it under cage (no layer-shell, so nothing can display itself over the fullscreen client), and under B13 that absence is **unnecessary rather than a gap**: the keyboard ships inside the app.

`Conflicts:` is the **continuous enforcement** of the no-blanking rule, not a hint (Policy §7.4: dpkg refuses to unpack them at the same time). `xdg-desktop-portal` is deliberately **not** listed — it is a dependency of too much to conflict with and it does not itself blank.

Version comes from the workspace `Cargo.toml`. `Conflicts`/`Replaces` are unused for self-replacement.

- [ ] **Step 3: Write `build-deb.sh` naming all three tools**

```bash
# All three, in this order. `${shlibs:Depends}` is a substvar consumed by dpkg-gencontrol;
# `dpkg-deb -b` over a hand-written DEBIAN/control emits the LITERAL string
# `${shlibs:Depends}` into the package and the failure is SILENT.
dpkg-shlibdeps -O ... > debian/substvars
dpkg-gencontrol -p kiosk -P"$STAGE" ...
dpkg-deb -b "$STAGE" "$OUT"
```

- [ ] **Step 4: Verify with lintian**

Run: `bash packaging/linux/build-deb.sh && lintian --fail-on error kiosk_*.deb`
Expected: **zero error-severity tags** — that is the point of both G1 and G6.

Then assert the substvar trap cannot ship:

Run: `dpkg-deb -e kiosk_*.deb /tmp/ctl && grep -q '\${shlibs' /tmp/ctl/control && echo FAIL`
Expected: no FAIL.

- [ ] **Step 5: Commit**

```bash
git add packaging/linux
git commit -m "feat(deb): payload layout, derived shlibs, idle-daemon Conflicts"
```

---

### Task 2: The installed unit (G8)

**Files:**
- Create: `packaging/linux/kiosk.service`

**Interfaces:**
- Consumes: P2-C's C11 `[Service]` **directive set**; G owns the values, `[Unit]` and `[Install]`

- [ ] **Step 1: Write the unit**

```ini
[Unit]
Description=kiosk browser
StartLimitIntervalSec=0
After=systemd-user-sessions.service
# G16 flip: add  Wants=seatd.service / After=seatd.service

[Service]
Type=simple
ExecStart=cage -- /usr/lib/kiosk/kiosk-launcher --config /etc/kiosk
Restart=always
RestartSec=30
RestartPreventExitStatus=86
SuccessExitStatus=86
RuntimeDirectory=kiosk
RuntimeDirectoryMode=0700
KillMode=control-group

[Install]
WantedBy=multi-user.target
```

Four values are load-bearing and each has a reproduced failure behind it:

- **`[Install] WantedBy=multi-user.target` is the fix without which autostart was a total no-op** — arch-05, the headline reason the `.deb` exists. On systemd 255, `systemctl --root=… enable` on a section-less unit prints *"The unit files have no installation config"*, creates **no symlink**, and `is-enabled` returns **`static`**; `deb-systemd-invoke:131-141` matches against `/enabled/`, `static` does not match, so it prints *"is a disabled or a static unit, not starting it"* and skips the start. **`enable` created nothing and `start` refused.** `multi-user.target`, not `graphical.target`: the runbook installs no display manager.
- **`StartLimitIntervalSec` belongs in `[Unit]`.** In `[Service]`, systemd 255 reports *"Unknown key name … in section 'Service', ignoring"* and the value is **silently discarded**. Value `0`: systemd's limits must be strictly looser than the FSM's so the FSM is always the authority that gives up first, and `0` is that requirement's limit case in one token. `StartLimitBurst` is dropped entirely — moot once the interval is `0`.
- **`SuccessExitStatus=86` alongside `RestartPreventExitStatus=86`.** They do not conflict: the first reclassifies the exit, the second suppresses the restart `Restart=always` would otherwise perform even on success. Without it a technician exit lands the unit in `failed` with `status=86`, so every dashboard reports a healthy technician exit as a device fault.
- **`RestartSec=30`, not 5.** 2,880 starts/day instead of ~17k, still an order of magnitude looser than `WINDOW_S = 600`. This is the input to G12's journal-retention arithmetic.

- [ ] **Step 2: Verify statically**

Run: `systemd-analyze verify packaging/linux/kiosk.service`
Expected: exit 0, and no "Unknown key name" line. This is how the `StartLimitIntervalSec` placement defect was found and how a misplaced key stays found.

- [ ] **Step 3: Record the loudness correction**

Do **not** justify `StartLimitIntervalSec=0` on the launcher's `startup-degraded.txt` breadcrumb — that file is `File::create` + one `writeln!` + `sync_all` (truncating, last-writer-wins), written only if the launcher starts and reaches `load_bootstrap`, which the failure class the start limit exists for (cage cannot get DRM master, missing `.so`, wrong `ExecStart`) **never reaches**. The accurate statement, which belongs in the runbook: the operator-facing signal is `systemctl status kiosk.service`; pre-launcher failures are journal-only.

- [ ] **Step 4: Commit**

```bash
git add packaging/linux/kiosk.service
git commit -m "feat(deb): installed unit with [Install], correct StartLimit placement"
```

---

### Task 3: Maintainer scripts — debhelper autoscripts verbatim (G7, G4, G5)

**Files:**
- Create: `packaging/linux/DEBIAN/{postinst,prerm,postrm}`

**Use debhelper's canonical autoscripts verbatim; do not hand-write maintscripts.** That single decision closes four separate defects at once.

- [ ] **Step 1: Enable via `deb-systemd-helper`, never `systemctl enable`**

```sh
deb-systemd-helper --quiet was-enabled kiosk.service && deb-systemd-helper enable kiosk.service || deb-systemd-helper update-state kiosk.service
```

`systemctl enable` in postinst is the **wrong helper**: it re-enables on every upgrade and thereby **reverts an operator's deliberate `systemctl disable`**. `deb-systemd-helper`'s own DESCRIPTION says the enable action is performed *"only once (when first installing the package)"*, and its `was_enabled` walks the recorded state-file entries returning 0 if any recorded symlink is gone — so an operator `disable` sends the upgrade down the `update-state` branch, bookkeeping only.

- [ ] **Step 2: Start/restart, guarded**

Guard `postinst-systemd-start`/`-restart` with `[ -z "${DPKG_ROOT:-}" ] && [ -d /run/systemd/system ]`, with `|| true` so a failed start never fails the install, and `[ -n "$2" ]` discriminating upgrade (`restart`) from fresh (`start`).

**The `[ -d /run/systemd/system ]` guard is why no `policy-rc.d` shim is needed** in the container: without it, `deb-systemd-invoke`'s `system('systemctl', …) == 0 or die(…)` propagates a nonzero postinst and `dpkg -i` fails outright.

**The asymmetry is load-bearing and deliberate:** `postinst-systemd-enable` is **not** wrapped in that guard; only `-start`/`-restart` are. That is precisely what makes G15's CI/hardware split possible — a systemd-less container can still assert `is-enabled` → `enabled`. If the guard had been on the enable snippet the split would collapse.

- [ ] **Step 3: Directory creation, first-install only**

```sh
if [ "$1" = configure ] && [ -z "$2" ]; then
    install -d -m 0750 -o root -g root /etc/kiosk /var/lib/kiosk
fi
# every configure, upgrade included: modes only, never ownership
[ -e /etc/kiosk/kiosk-credential.json ] && chmod 0600 /etc/kiosk/kiosk-credential.json
```

Without `[ -z "$2" ]` the next upgrade after an operator has followed G16's non-root recipe reverts the `chown` and the service loses read access to its own credential — the exact failure class conceded for `systemctl enable`. **Mode-only on upgrade is uid-agnostic**, which is what makes G16's flip survive every upgrade.

- [ ] **Step 4: prerm/postrm**

`deb-systemd-invoke stop`; `deb-systemd-helper purge` on purge; `postrm-systemd-reload-only`.

**Do not use `deb-systemd-invoke try-restart`** — the disabled-and-not-running guard applies to `start` and `restart` **only**, and `try-restart` falls through to `exec('systemctl', @ARGV)`, **bypassing the very guard** it was wanted for. The documented verb is both in-contract and strictly better.

- [ ] **Step 5: Verify the full cycle non-interactively**

Run, in a `debian:12` container:
```bash
DEBIAN_FRONTEND=noninteractive dpkg -i kiosk_*.deb </dev/null; echo "rc=$?"
systemctl --root=/ is-enabled kiosk.service      # → enabled
# operator edits, then upgrade:
dpkg -i kiosk_*.deb </dev/null; echo "rc=$?"     # operator content intact, mode 640
```
Expected: `rc=0` both times, `is-enabled` → `enabled`, operator files untouched.

- [ ] **Step 6: Commit**

```bash
git add packaging/linux/DEBIAN
git commit -m "feat(deb): debhelper autoscripts, first-install-only dirs, mode-only upgrades"
```

---

### Task 4: Operator files and the credential provisioning helper (G5, G6, G9)

**Files:**
- Create: `packaging/linux/kiosk-provision-credential`, `packaging/linux/kiosk.ini.example`

**The package ships zero conffiles, all three operator files are absent by default, and all three fail closed.** One rule applied consistently, and the consistency is the design:

| File | Shipped? | Provisioned by | Behaviour when absent |
|---|---|---|---|
| `/etc/kiosk/kiosk.ini` | No — example at `/usr/share/kiosk/` | runbook `install -m 0640` then edit | launcher non-fatal (defaults + `startup-degraded.txt`); `boot::load` renders `safe.html` |
| `/etc/kiosk/kiosk-credential.json` | No | `kiosk-provision-credential` | `credential_is_owner_only` → `Err` → `RenderSafe{credential_permissions}`; fetch loop `config_error` + `break` |
| `/etc/kiosk/kiosk-offline.mp4` | No | runbook | asset 404, `offline.html` degrades to black splash → E's `media.error` bridge |

- [ ] **Step 1: Write the provisioning helper**

```sh
#!/bin/sh
# Directory mode governs traversal; `umask` governs the mode of files created inside — so a
# 0750 /etc/kiosk does NOT stop `cp` from landing a 0644 secret in it (reproduced). This
# command is the thing that SETS the mode, instead of a sentence asking the operator to
# remember flags.
set -eu
install -m 0600 -o root -g root "$1" /etc/kiosk/kiosk-credential.json
```

*Honest restatement:* **the app's fail-closed gate is what enforces the mode; the package supplies the traversal barrier and a provisioning command that sets it correctly.** `/etc/kiosk` at `0750` is a **traversal barrier only** — describe it as exactly that.

- [ ] **Step 2: Do NOT ship a placeholder credential**

`dist-template/kiosk-credential.json` has non-empty `client_email`, `private_key` and `token_uri`, and `ServiceAccount::from_json` rejects only *empty* fields — so P1-F2's placeholder at `0600` yields `BootOutcome::Ready`: **an unprovisioned device boots reporting healthy** and fails invisibly at token exchange. A pre-created empty `0600` credential is also rejected: it degrades the signal to `reason: None` and removes the fetch-loop `break`.

Absent is the only row that is deferred-visible with a named draining mechanism (`telemetry::spool_boot_config_error` writes to `<data>/spool/main` with no GCL client and no credential).

- [ ] **Step 3: Ship the mp4 nowhere; warn in postinst**

Four grounds, all recorded: (i) `dist-template/kiosk-offline.mp4` is **88 bytes of ASCII**, `OBVIOUSLY FAKE VIDEO PLACEHOLDER` — there is no asset to ship; (ii) under G1 the app reads `/etc/kiosk/kiosk-offline.mp4`, so any shipped default forces either `file-in-etc-not-marked-as-conffile` or a binary blob as a conffile; (iii) E's soak and F's per-PR subset supply their own fixtures; (iv) absence is caught (404 → degrade → `media.error`, gated by H5/H8).

postinst prints a warning if the path is absent — nearly worthless on an unattended install, one line, does no harm.

- [ ] **Step 4: Verify the `/usr/share/doc` trap is avoided**

Run, in the container: `dpkg -L kiosk | grep kiosk.ini.example && test -f /usr/share/kiosk/kiosk.ini.example`
Expected: both succeed. (Under `/usr/share/doc/` the file is silently dropped on install **while `dpkg -L` still lists it** — exactly the kind of thing a packaging test asserts on and is fooled by.)

- [ ] **Step 5: Commit**

```bash
git add packaging/linux
git commit -m "feat(deb): zero conffiles, absent-by-default operator files, mode-setting helper"
```

---

### Task 5: The lockdown runbook (G10, G11, G12)

**Files:**
- Create: `packaging/linux/lockdown.md`

**Every step ends with a verify command.**

- [ ] **Step 1: VT / console / seat (G10)**

`cage` is invoked **without `-s`**, and that is the strongest single step: cage 0.1.4-4's `seat.c:236-246` gates the only `wlr_session_change_vt` call in the tree behind `allow_vt_switch`, which `-s` sets. **VT switching is off by default in cage**, so §7.2's "disable VT switching and zap" is discharged mechanically.

```sh
sed -i 's/^#\?NAutoVTs=.*/NAutoVTs=0/;s/^#\?ReserveVT=.*/ReserveVT=0/' /etc/systemd/logind.conf
systemctl mask getty@.service ; systemctl disable getty.target
# verify
ls /dev/tty[1-9]* 2>/dev/null && echo FAIL
systemctl list-units 'getty@*' --all --no-legend | grep -q . && echo FAIL
loginctl seat-status seat0
```

Add D's one reserved sentence here: *the technician chord is the in-session escape under the locked cage session; the image intentionally leaves no VT/getty route.* Chord *swallowing* is unnecessary under cage — VT switching is what actually needs killing, and it dies in logind and in cage's own default, not in app code. X11's `DontVTSwitch`/`DontZap` goes in the **X11-is-demo-only appendix**; X11/openbox stays documented but **not app-enforced**.

- [ ] **Step 2: Display blanking (G11)**

**cage 0.1.4 has no idle timeout and no blanking at all** — verified three ways from source. So the parent's PRIMARY ("configuring cage/wlroots not to blank") **has nothing to configure**, and the requirement is discharged by **preventing anything else from supplying a blanker**: the dpkg `Conflicts:` line, which binds continuously at every `apt` transaction for the life of the device.

The grep survives as the **verify line**, because `Conflicts:` binds packages, not hand-started processes:

```sh
dpkg -l | grep -E 'swayidle|xautolock|xscreensaver|light-locker|gnome-screensaver' && echo FAIL
systemctl list-units --type=service --state=running | grep -iE 'idle|screensaver|power-manager' && echo FAIL
```

Record that **parent §11's row is answered NEGATIVELY**: cage's `zwp_idle_inhibit_v1` toggles cage's own `wlr_idle` notifier — which nothing on the device consumes — and has no relationship to logind inhibitor locks. The answer is not "cage ignores it" but **"there is nothing for it to inhibit."**

Two consequences, stated rather than left for the reader: **H3 loses `systemd-inhibit --list` as a keep-awake proof** (retained only as a regression check that B's spawn path ran), and **B's `systemd-inhibit` child is defence-in-depth with no current effect** under this runbook — kept because it is the only mechanism still acting if an operator unmasks a sleep target, and it costs one `cat`.

`consoleblank=0` and `IdleAction=ignore` are **demoted from keep-awake mechanisms to belts** — both are already the defaults; set them explicitly so an inherited cmdline or stray drop-in cannot un-default them. **Assert, do not rely.**

- [ ] **Step 3: Sleep, cosmetics, updates, SSH, recovery (G12)**

- `systemctl mask sleep.target suspend.target hibernate.target hybrid-sleep.target`; verify `is-enabled` → `masked` for all four.
- Quiet boot (`quiet`, cursor blink off); verify against `/proc/cmdline`.
- **journald `Storage=persistent`** plus `SystemMaxUse` as a **computed floor**: sized to retain ≥7 days at `RestartSec=30` (2,880 unit starts/day). *The arithmetic is fixed; the byte value is per-device-class* — compute and record it here. Verify with `journalctl --disk-usage` and `journalctl -b -1 -u kiosk.service | head`.
- `unattended-upgrades` **off**; verify `is-enabled` → `disabled`/`masked`.
- **WebKitGTK pin:** `apt-mark hold libwebkit2gtk-4.1-0`, released only through the runbook's revalidation loop; verify `apt-mark showhold`.
- SSH keyed-only if enabled; the default recipe leaves it **absent**; verify `systemctl list-unit-files 'ssh*'`.
- **Recovery step, documented rather than discovered:** *"If the screen is frozen and the device has not recovered within 60 s, power-cycle. On a conforming image there is no VT, no getty and no SSH by design; a power cycle is the supported recovery, not a workaround."* Observed by H11.
- Carry P1-F2's fourth error-handling bullet: *"on a freshly installed device stuck black, suspect `kiosk.ini` / the credential first — `safe.html` appearing is not a prerequisite for a config problem."*

- [ ] **Step 4: The `On-screen keyboard` section**

**This build ships a bundled on-screen keyboard — P2-B's B13.** G does not restate B13's design. Write four things:

1. **squeekboard ruled out, with the protocol evidence**, so nobody re-litigates it: cage's complete `*_create(` list on 0.1.4 has no layer-shell and no input-method; `zwlr_layer_shell_v1` is absent on **both 0.1.4 and 0.1.5**, so **no separate-process OSK can place itself over a fullscreen client on either version.** Carry the correction: `wlr_virtual_keyboard_manager_v1_create` **is** present on 0.1.5 — *display* is what is blocked, not *injection*.
2. **`onboard` + `GDK_BACKEND=x11` inside cage's Xwayland is withdrawn as a fallback**, surviving only as the recorded reason not to reach for it: it forfeits the Wayland input path P2-D is built on and the GDK event stream D's `ActivityClock` depends on. (The narrower true claim: an **XTEST-based** OSK produces no GDK events in our process; a `zwp_virtual_keyboard_v1` client would not, but has no way to display itself.)
3. **`Depends:` — none, stated explicitly.**
4. **The operator-facing prerequisite, verbatim:** *"This build ships a bundled on-screen keyboard. It is injected into the deployed page at document start and serves that page's own text inputs. It cannot serve anything outside the page: there is no native UI and no browser chrome on this device, and no OS keyboard can display itself under cage. It is page-world code, so it is a **usability feature and not a security control** — a compromised page can see it and alter it, as it can any injected control, and nothing about the keyboard should be read as a boundary. Walk the site's text-entry surfaces on the device before sign-off: the keyboard is bundled, but its fit to a given site's forms is per-deployment."* Verify: type into every input of the deployed site on the device's own touch panel; record in **H4b**.

- [ ] **Step 5: The service-user section (G16)**

**Shipped default: root** — the unit carries no `User=`; `/etc/kiosk` and `/var/lib/kiosk` are `root:root`, the credential `0600 root:root`.

State the **Windows-parity divergence** plainly: Windows ships an *unprivileged* kiosk account; Linux ships **root** by default because root is the only variant needing no additional package for DRM/seat access. **This is a weaker posture than Windows and is not the project's normal posture** — it is a shipped default pending hardware evidence.

*Scope note:* the non-root variant does **not** close the renderer→credential path — `WebKitWebProcess` runs as the same uid either way. The delta is everything else root can do, which the escape-mitigation list does not address.

Write the hardened recipe fully, so H1 is a **promotion and not a redesign**:

```sh
adduser --system --group kiosk
usermod -aG _seatd,video,input,render kiosk
systemctl enable --now seatd ; loginctl seat-status seat0     # verify
chown -R kiosk:kiosk /etc/kiosk /var/lib/kiosk
stat -c '%a %U:%G' /etc/kiosk/kiosk-credential.json           # verify → 600 kiosk:kiosk
```

**Flip-list — exactly what changes if H1 promotes it:** the two `chown`s; `User=`; `SupplementaryGroups=video,input,render,_seatd`; `Wants=`/`After=seatd.service` in `[Unit]`; `Depends: seatd` in control; and **P2-A's `ponytail:` uid check activates (H9)** — A's mode-bits check is uid-agnostic, so once owner and reader can differ, mode bits stop proving anything.

- [ ] **Step 6: The image-capture step (G13)**

```sh
dpkg -l > snapshot-$(date +%F).txt                                     # human reading
dpkg-query -W -f='${binary:Package}\t${Version}\t${db:Status-Abbrev}\n' \
  > snapshot-$(date +%F).tsv                                           # machine-diffable
cage -v >> snapshot-$(date +%F).tsv                                    # the floor, recorded
```

Runbook-on-stock-Debian-12 **is** the P2 image story — exact parity with how Windows shipped (MSI + `lockdown.md`, no golden image). **The validation is the image pin.** Automated image building (preseed/FAI/debos) is the recorded ponytail.

- [ ] **Step 7: Commit**

```bash
git add packaging/linux/lockdown.md
git commit -m "docs(runbook): lockdown steps with a verify command on every one"
```

---

### Task 6: The hardware validation checklist (G14)

**Files:**
- Create: `docs/testing/linux-hardening-checklist.md`

> **Corrected:** parent §10 does not enumerate a hardening list, it points at `docs/testing.md`, **which does not exist**. The file G creates is `docs/testing/linux-hardening-checklist.md`, matching the existing directory convention.

- [ ] **Step 1: Write H1–H11, each citing its origin**

| # | Item |
|---|---|
| **H1** | Real cage boot chain: unit → cage → launcher → main; fullscreen on the physical display; `-m last`/`-m extend` behaviour recorded. **Promotes or rejects the G16 service-user fork.** |
| **H2** | `RestartPreventExitStatus=86` **and `SuccessExitStatus=86`** end-to-end via systemctl: technician exit stays exited **and** `systemctl status` reads `inactive (dead)`, not `failed`. Plus **`is-active` → `active` after boot** — the assertion no container can make |
| **H3** | Keep-awake positive: G11's no-idle-consumer gate re-verified on the device; display never blanks over 24 h; panel OSD sleep timer observed. `systemd-inhibit --list` is a **regression check that B's spawn path ran**, not a keep-awake proof |
| **H4a** | **Touch:** corner-tap opens the pin pad on the device's own touch panel. Record: taps counted per single-finger tap; taps per N-finger tap (Windows counts 1 — D's declared divergence); whether `GDK_TOUCH_CANCEL` is emitted at all on this panel |
| **H4b** | **Text entry:** validate **B13's bundled keyboard** on the device class's own touch panel — every text-entry surface of the deployed site reaches it, types into it, and dismisses it; record any input it fails to fit, cover or restore scroll position for |
| **H5** | ≥72 h offline-video soak, RSS trend, loop count; visual black-frame check |
| **H6** | Escape-vector sweep under the locked session: parent §7.2's vectors (VT chords, zap, sleep) plus §7's shortcut/dialog/edge rows |
| **H7** | Egress + nav guard against a real network: DNS failure modes, captive-portal interference (B's smoke runs against a local httpd only) |
| **H8** | Runbook executed cold on the device class, timed; both snapshots captured; mp4 and credential provisioning verified. **And: after first boot and before sign-off, read the local spool and assert it contains no `egress.filter_absent` and no `egress.csp_absent`** |
| **H9** | Under the promoted service user: credential readable, `/var/lib/kiosk` writable, spool drains, `/run/kiosk` socket reachable |
| **H10** | **Pinch-zoom does not scale the page on touch hardware; two-finger pan/scroll still works.** Second clause failing ⇒ D13's recorded `scale_delta()` deadband is the fix |
| **H11** | **Wedged-compositor recovery:** with cage `SIGSTOP`ped, confirm the device does **not** self-recover and that the documented power-cycle step restores service. Record time-to-detect by an on-site observer |

- [ ] **Step 2: Record why H4 split**

It began as the touch row and eroded into the text-entry row across two revisions while a sibling kept routing touch deferrals into it. **H4a restores the touch content verbatim; H4b keeps the text-entry text, now aimed at a control that exists rather than at a gap's discoverability.** Both IDs are cited by name from P2-D; H4b is additionally cited from P2-B as B13's hardware gate.

- [ ] **Step 3: Make the checklist hardware-parameterized, not hardware-blocked**

Target hardware is explicitly TBD. Every row takes the device class as a parameter and records results per class.

- [ ] **Step 4: Commit**

```bash
git add docs/testing/linux-hardening-checklist.md
git commit -m "docs(testing): H1-H11 hardware validation checklist"
```

---

### Task 7: The CI assertions G declares to F (G15)

**Files:**
- Create: `packaging/linux/container-assertions.sh` (invoked by F's nightly `debian:12` job)

> **The split is drawn where the mechanism actually falls**, measured: `systemctl --root=… is-enabled` answers in a systemd-less container — it is a filesystem query, rc=0. **`is-active` does not**: *"System has not been booted with systemd as init system (PID 1)."* That line is exactly where the missing-`[Install]` regression lives, which is what makes the split worth drawing rather than deferring everything. **`active` is H2's, not the container's.**

- [ ] **Step 1: Write the container assertions**

- install / remove / purge / upgrade cycle completes, rc=0, non-interactive, stdin closed.
- `test -x /usr/lib/kiosk/kiosk-launcher` **and** `test -x /usr/lib/kiosk/kiosk-main`. **Required:** with `cage` as the `ExecStart` command the launcher is an *argument*, so `systemd-analyze verify` never checks its path (verified: rc=0 with the launcher absent, versus rc=1 when the launcher is the command). This is the cheapest cover for the exact hole G1's path change opens.
- Modes: `/etc/kiosk` and `/var/lib/kiosk` `0750`; `/usr/share/kiosk/kiosk.ini.example` present.
- **All three operator files absent on a fresh install** — that is the provisioning contract, asserted rather than implied.
- Upgrade preserves an operator-written `kiosk.ini`, an operator-placed credential and mp4; and `deb-systemd-helper` does **not** re-enable a unit the test disabled first.
- `systemctl --root=/ is-enabled kiosk.service` → **`enabled`** (the missing-`[Install]` guard).
- `systemd-analyze verify` on the shipped unit.
- `grep -R` over the built `.deb` finds **zero** `BEGIN PRIVATE KEY` and **no `kioskctl`**.
- `grep -q '\${shlibs' <extracted control> && FAIL`.
- **`pkill -9 kiosk-launcher; sleep 2; ! pgrep kiosk-main`** — C12's orphan-kill gate, closing verification finding V4. It belongs here, not in an H row: H rows are human hardware checks; this one is mechanical and runs where a `.deb` is installed and processes actually exist.
- **`cage -v` emitted and asserted equal to the recorded floor `0.1.4`.**

- [ ] **Step 2: Declare the two additions to F**

F's **nightly** `debian:12` job runs this script by reference (F asserts `is-enabled`, **not** `is-active`). F's **release** job runs `dpkg-shlibdeps` → `dpkg-gencontrol` → `dpkg-deb -b`, then `lintian --fail-on error`. Overrides live in `debian/source/lintian-overrides` with a comment each; after G1 and G6, **zero error-severity tags are expected**.

- [ ] **Step 3: Run the assertions in a container**

Run: `docker run --rm -v "$PWD:/w" -w /w debian:12 bash packaging/linux/container-assertions.sh`
Expected: all pass, rc=0.

- [ ] **Step 4: Commit**

```bash
git add packaging/linux/container-assertions.sh
git commit -m "test(deb): container assertions for F's nightly job"
```

---

## Self-Review

**Spec coverage:** G1/G2/G3/G9 → T1; G8 → T2; G7/G4 → T3; G5/G6 → T4; G10/G11/G12/G13/G16 → T5; G14 → T6; G15 → T7.

**Open decisions to resolve during execution (values and layout only):** the exact `SystemMaxUse` byte value satisfying the ≥7-day floor at `RestartSec=30` on the chosen device class's journal volume — **the arithmetic is fixed, the input is per-class** (T5 Step 3); and whether `packaging/linux/` assembles via `debhelper`/`dh` or a hand-rolled tree feeding the three named tools — **the three tools and their order are fixed either way** (T1 Step 3).

**Not assertable before hardware, and owned:** the unit reaching `active`, cage obtaining DRM master, seat/session availability, `RuntimeDirectory` under a non-root uid, library resolution at exec time. All of those are **H1/H2**.

**Residual risks, each with a named carrier:** root by default is looser than Windows' unprivileged account → **H1**; `Conflicts:` removal risk (`apt -y` proposing to remove `kiosk`) → loud, not silent, bounded by G12's update discipline; `StartLimitIntervalSec=0` looping forever on a permanently broken install → persistent journal with a computed floor, **H2**; an unprovisioned device invisible to the fleet outside the spool retention window → on-screen `safe.html` + **H8**; panel-OSD sleep timer → **H3**; a future device class forcing a non-cage compositor ⇒ G11's analysis does not transfer → **H1**; wedged cage → **H11** + the runbook power-cycle line.
