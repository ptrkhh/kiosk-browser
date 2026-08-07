# P2-G — Linux Packaging, OS Image Runbook, Hardware Validation (Design)

> Seventh and final sub-project of P2. Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.4 (GStreamer
> element set, the pinned image), §4 (paths), §7 (keyboard, keep-awake), §7.2 (Linux OS
> lockdown), §9, §10 (RT-05, escape-vector sweep), §11. **Sibling precedent is P1-F2**
> (`2026-08-02-p1f2-packaging-deployment-design.md`): the installer owns app-scoped setup, a
> lockdown *runbook* owns OS hardening, secrets never ship in the package. G consumes C's
> unit shape and `--config` forwarding, D's chord note, B's relabelled inhibitor, E's mp4
> path, and B/C/D/E's accumulated hardware deferrals.

**Status:** rev 2, 2026-08-07 — adversarial design review; see
`docs/superpowers/reviews/2026-08-07-p2b-p2g-adversarial-review/`.

## Goal

An operator takes stock Debian 12, follows one runbook, installs one `.deb`, provisions three
files that the package deliberately does not ship, and has a locked kiosk that survives
reboot — plus a hardware validation checklist that retires every "deferred to hardware" item
A–E accumulated and gives five sibling deferrals a real row to land on. Target hardware
remains TBD (design to the C7 floor: Debian 12 / Ubuntu 22.04, x86_64); the checklist is
hardware-parameterized, not hardware-blocked.

## Scope

**In:** `packaging/linux/` — the `.deb` (payload, control, maintscripts), the installed
`kiosk.service` (values, `[Unit]`, `[Install]`), the credential provisioning helper, the
`packaging/linux/lockdown.md` runbook, `docs/testing/linux-hardening-checklist.md`, the
hardware checklist H1–H11, and the container/release CI assertions that gate all of it.

**Out:** product code. G ships no change under `crates/`. The unit's *directive set* is
P2-C's (C11); nav/egress is P2-B's; input is P2-D's; the video asset is the operator's; the
CI jobs themselves are P2-F's and G declares its additions to them.

**Change register:** G1–G16. Cross-spec edges are tabulated at the end; every one is declared
in both directions. One obligation G does **not** discharge — the Linux touch keyboard — is
ledger item **I1** and is stated as such rather than resolved here.

**Every cage claim in this spec is version-stamped.** cage **0.1.4-4** is the C7 floor
(Debian 12, `sources.debian.org`); cage **0.1.5** is what was run in-session and what P2-C
measured against. Where the two differ, both are given (see the keyboard section, where the
difference is load-bearing).

## Architecture — the install layout (G1)

```
/usr/lib/kiosk/            kiosk-main, kiosk-launcher, bundled/{error,offline,pinpad,safe,splash}.html,
                           kiosk-provision-credential
/usr/share/kiosk/          kiosk.ini.example          (not a conffile, never operator-edited)
/lib/systemd/system/       kiosk.service
/etc/kiosk/                kiosk.ini, kiosk-credential.json, kiosk-offline.mp4   ← all three absent from the package
/var/lib/kiosk/            cache, spool, last-good     (0750, created first-install only)
```

The two trees are joined by the already-shipped `--config` flag:

```ini
ExecStart=cage -- /usr/lib/kiosk/kiosk-launcher --config /etc/kiosk
```

**Parent §4's `/opt/kiosk/` cell is recorded as an erratum requiring an owner-level
amendment to the spec of record.** The conflict is not preference against requirement, it is
requirement against a verified external constraint:

- `/opt` trips lintian `dir-or-file-in-opt` at **severity: error** (lintian 2.139.0, tag page
  fetched by both roles) — *"Debian packages should not install into /opt, because it is
  reserved for add-on software."* G's own §Testing gate is `lintian --fail-on error`, so
  conforming to the literal cell fails G's own gate. It is the same severity as the conffile
  tag that killed the shipped-mp4 plan (G6).
- Debian Policy 4.7.4.1 §9.1.1, exception 1, verbatim: *"a subdirectory of `/usr/lib` may be
  used by a package (or a collection of packages) to hold a mixture of
  architecture-independent and architecture-dependent files."* Our payload is exactly that
  mixture — two ELF binaries plus five HTML pages. `grep -n libexec policy.txt` returns
  **nothing**: `/usr/libexec` has no Policy standing at all, so the draft's original choice
  was the weaker of the two.
- Policy 10.7.2 requires configuration in `/etc`. Under the parent's literal row 2 ("same" —
  next to the binaries) `kiosk.ini`, the credential and the mp4 all land under `/usr`.

**Divergence statement (C3), both directions.** *Stricter:* the operator files move out of
the install dir into `/etc/kiosk`, so `/usr` can be mounted read-only and the config is where
Policy says it is. *Looser:* the install directory is not `/opt/kiosk/`. **What must change,
named:** parent §4 table row 1 Linux cell (`/opt/kiosk/` → `/usr/lib/kiosk/`) and row 2 Linux
cell (`same` → `/etc/kiosk/ (--config)`).

**The amendment is the owner's call, not this spec's.** G asserts an erratum and escalates it;
it does not overrule a tier-1 document by fiat. **Survivable fallback if the owner refuses:**
ship the binaries under `/opt/kiosk/` with a documented `dir-or-file-in-opt` override in
`debian/source/lintian-overrides`, carrying a comment. Either way the config, credential and
mp4 move to `/etc/kiosk` — that half is not optional, because it is Policy 10.7.2 and because
it is what makes a read-only `/usr` possible.

**The split costs zero product code, on one condition.** `crates/kiosk-launcher/src/main.rs:27-42`
already parses `--config <dir>`; `resolve_main_exe` (`main.rs:56-62`) resolves the child from
`current_exe()`'s directory, not from `config_dir`; `crates/kiosk-main/src/main.rs:423-431`
`resolve_config_dir` honours the flag, and all three consumers follow it — `:655` (`kiosk.ini`),
`:730` (`config_dir.join(&bootstrap.credential)`), `:999` (`kiosk-offline.mp4`).
`resolve_data_dir` is independent and never operator-overridden.

**The condition, declared (INT-9):** the *Windows* `spawn_main` appends
`cmd.arg("--config").arg(config_dir)` at `spawn.rs:121`; the `#[cfg(not(windows))]` stub at
`spawn.rs:198-210` takes `_config_dir` and drops it. **P2-C's C5 must carry `--config` through
the Linux `spawn_main`.** If it does not, `kiosk-main` resolves its config dir to
`/usr/lib/kiosk`, finds none of the three operator files, and boots into safe mode —
fail-closed and loud, but a real dependency and it is declared on both sides.

*Withdrawn, recorded:* the draft's *"binaries-in-one-place, config-in-another is the shape the
code already supports — nothing to implement."* True of Windows only.

## Components

### 1. `.deb` — `packaging/linux/`

#### Payload (G2)

Two binaries, the five bundled pages, the unit, the example config and the provisioning
helper — the layout above. **`kioskctl` is withdrawn from the payload.** It is
`crates/kiosk-core/examples/kioskctl.rs`, a cargo example and not a workspace binary, and its
module doc names `KIOSK_SIGNING_KEY_B64`: it carries the **fleet private signing seed's**
tool. That belongs on a CI/ops host, not on a device an attacker can physically remove
(§8/SEC-08). Nothing breaks: P2-A cites it as the signing *harness* for smoke fixtures and
`docs/testing/p1d2-signed-config-smoke.md` invokes it as `cargo run -p kiosk-core --example
kioskctl` — repo-side, never from the device.

#### Dependencies (G3)

```
Depends: ${shlibs:Depends}, cage, gstreamer1.0-plugins-base, gstreamer1.0-plugins-good,
         gstreamer1.0-plugins-bad, gstreamer1.0-libav
Conflicts: swayidle, xautolock, xscreensaver, light-locker, gnome-screensaver, xfce4-power-manager
```

The seven runtime names the draft listed are all verified present in bookworm main
(`libwebkit2gtk-4.1-0`, `libgtk-3-0`, `cage 0.1.4-4`, and the four GStreamer packages), but
they are **not** hand-written any more. The library half is derived from the built ELF
binaries by `dpkg-shlibdeps`, which is per-floor-correct by construction and makes the
`libgtk-3-0` / `libgtk-3-0t64` (Ubuntu 24.04 `time_t`) alternation moot — **that residual is
withdrawn, it no longer exists.** The four GStreamer names stay hand-written because they are
runtime *plugin* packages no ELF scan can see and because they are parent §3.4 verbatim
(a missing element is a silent black video; the dependency line is the first defence, E's
watchdog the second). `cage` likewise.

**The assembly pipeline is `dpkg-shlibdeps` → `dpkg-gencontrol` → `dpkg-deb -b`, all three
named.** `${shlibs:Depends}` is a substvar consumed by `dpkg-gencontrol`; `dpkg-deb -b` over a
hand-written `DEBIAN/control` emits the literal string `${shlibs:Depends}` into the package
and the failure is silent. G15 asserts `grep -q '\${shlibs' <extracted control> && FAIL` so it
cannot ship.

**No keyboard `Depends:`, stated explicitly** — see the keyboard section; no installable OS
package solves it under cage, and declaring one would ship a broken dependency.

`Conflicts:` is the continuous enforcement of the no-blanking rule (G11), not a hint. Policy
§7.4: *"`dpkg` will refuse to allow them to be unpacked on the system at the same time …
Otherwise, it will halt the installation of the new package with an error."*
`xdg-desktop-portal` is deliberately **not** listed — it is a dependency of too much to
conflict with and it does not itself blank.

#### The installed unit (G8)

C11 owns the directive *set*; G owns the values, the `[Unit]` section C's shape lacks, and
`[Install]` — C:80 hands "values **and installation**" to G, and `[Install]` is installation.

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

**`[Install] WantedBy=multi-user.target` is the fix without which autostart was a total
no-op** — arch-05, the headline reason the `.deb` exists. Reproduced on systemd 255 by both
roles: `systemctl --root=… enable` on the section-less unit prints *"The unit files have no
installation config"*, creates **no symlink**, and `is-enabled` returns **`static`**;
`/usr/bin/deb-systemd-invoke:131-141` matches `is-enabled` output against `/enabled/`, and
`static` does not match, so it prints *"$unit is a disabled or a static unit, not starting
it"* and skips the start. `enable` created nothing **and** `start` refused. Adding the section
yields `Created symlink …/multi-user.target.wants/kiosk.service` and `is-enabled` → `enabled`.
`multi-user.target`, not `graphical.target`: the runbook installs no display manager.

**`StartLimitIntervalSec` belongs in `[Unit]`.** In `[Service]` systemd 255 reports
*"Unknown key name 'StartLimitIntervalSec' in section 'Service', ignoring"* and the value is
**silently discarded** — reproduced independently by both roles, and the start-limit decision
depends on it. Value `0`: G's own requirement is that systemd's limits be strictly looser than
the FSM's, so the FSM is always the authority that gives up first, and `0` is that
requirement's limit case in one token. The FSM never hands systemd a decision in the normal
path — `watchdog.rs:80` `WINDOW_S = 600`, `:81` `SAFE_FAIL_LIMIT = 3`, `:159-162` backoff
doubles to a 60 s ceiling and **holds there**; the launcher does not exit when it gives up, it
emits `watchdog.safe_mode_failed` and keeps looping. So the start limit only ever governs the
launcher process crash-looping, and there a systemd-enforced permanent stop is a black screen
forever — strictly worse than parent §3.1/arch-14's bounded-loud-loop doctrine.
`StartLimitBurst` is dropped entirely: moot once the interval is `0`.

**`SuccessExitStatus=86` alongside `RestartPreventExitStatus=86`.** Parent §3.1:170-173 names
both in one sentence and neither C nor the draft carried the first. They do not conflict:
`SuccessExitStatus` reclassifies the exit, `RestartPreventExitStatus` suppresses the restart
`Restart=always` would otherwise perform even on a success. Without it a technician exit lands
the unit in `failed` with `status=86`, so `systemctl is-failed` and every dashboard above it
report a healthy technician exit as a device fault — a Q3 defect on the one flow a field
technician uses. `systemd-analyze verify` accepts both directives together, exit 0.

**`RestartSec=30`, not 5.** 2,880 starts/day instead of ~17k, still an order of magnitude
looser than the FSM's `WINDOW_S = 600` / 60 s backoff ceiling. This is the input to G12's
journal-retention arithmetic.

*Loudness claim withdrawn, recorded.* The draft justified `StartLimitIntervalSec=0` on the
launcher's `startup-degraded.txt` breadcrumb. `crates/kiosk-launcher/src/sink.rs:163-179` is
`File::create` + one `writeln!` + `sync_all` — truncating, single line, last-writer-wins — and
its own doc says *"Presence therefore means 'the LAST boot was degraded', not 'some boot,
once, was degraded'"*. It is written only if the launcher starts and reaches `load_bootstrap`,
which the failure class the start limit exists for (cage cannot get DRM master, missing `.so`,
wrong `ExecStart`) never reaches. **The accurate statement:** the operator-facing signal is
`systemctl status kiosk.service`; the breadcrumb covers only failures that reach
`load_bootstrap`; pre-launcher failures are journal-only. For a *permanently* broken install
the Nth failure is byte-identical to the first, so rotation discards duplicates, not evidence;
the real evidence-loss case is a transient first failure followed by a different persistent
one, and G12's persistent journal with a computed floor is what covers it.

#### Maintainer scripts (G7, G4, G5)

**Use debhelper's canonical autoscripts verbatim; do not hand-write maintscripts.** That
single decision closes four separate defects at once and is the Q2 answer.

- **Enable — `postinst-systemd-enable`.**
  `deb-systemd-helper --quiet was-enabled … && deb-systemd-helper enable … || … update-state`.
  A raw `systemctl enable` is the **wrong helper**: `/usr/bin/deb-systemd-helper`'s own
  DESCRIPTION says the *"enable action will only be performed once (when first installing the
  package)"*, whereas `systemctl enable` in postinst re-enables on every upgrade and thereby
  **reverts an operator's deliberate `systemctl disable`**. The `was_enabled` implementation
  (`deb-systemd-helper:418-434`) walks the recorded state-file entries and returns 0 if any
  recorded symlink is gone — so an operator `disable` sends the upgrade down the
  `update-state` branch, bookkeeping only, no re-enable. On a unit with no state file it
  returns true, which is the first-install path.
- **Start — `postinst-systemd-start` / `-restart`,** guarded by
  `[ -z "${DPKG_ROOT:-}" ] && [ -d /run/systemd/system ]`, with `|| true` so a failed start
  never fails the install, and `[ -n "$2" ]` discriminating upgrade (`restart`) from fresh
  (`start`). **The `[ -d /run/systemd/system ]` guard is why no `policy-rc.d` shim is needed**
  in the container: without it, `deb-systemd-invoke`'s
  `system('systemctl', …) == 0 or die(…)` at `:148` propagates a nonzero postinst and `dpkg -i`
  fails outright.
- **The asymmetry is load-bearing and deliberate.** `postinst-systemd-enable` is **not**
  wrapped in that guard; only `-start`/`-restart` are. That is precisely what makes G15's
  CI/hardware split possible — a systemd-less container can still assert
  `is-enabled` → `enabled`. If the guard had been on the enable snippet the split would
  collapse.
- **Directory creation is first-install only:**

  ```sh
  if [ "$1" = configure ] && [ -z "$2" ]; then
      install -d -m 0750 -o root -g root /etc/kiosk /var/lib/kiosk
  fi
  # every configure, upgrade included: modes only, never ownership
  [ -e /etc/kiosk/kiosk-credential.json ] && chmod 0600 /etc/kiosk/kiosk-credential.json
  ```

  Without `[ -z "$2" ]` the next upgrade after an operator has followed G16's non-root recipe
  reverts the `chown` and the service loses read access to its own credential and write access
  to `/var/lib/kiosk` — the exact failure class conceded for `systemctl enable`. Mode-only on
  upgrade is uid-agnostic, so G16's `chown -R kiosk:kiosk` survives every upgrade.
- **prerm/postrm:** `deb-systemd-invoke stop`; `deb-systemd-helper purge` on purge;
  `postrm-systemd-reload-only`.

*Withdrawn, recorded:* `deb-systemd-invoke try-restart`. `/usr/bin/deb-systemd-invoke:38`
documents `start|stop|restart`; the disabled-and-not-running guard at `:115-146` applies to
`start` and `restart` **only**; `:186`'s `exec('systemctl', @ARGV)` is the fall-through
`try-restart` would have taken, **bypassing the very guard** the draft wanted from it. The
documented verb is both in-contract and strictly better.

#### Operator files — zero conffiles (G5, G6, G9)

**The package ships zero conffiles and all three operator files are absent by default, and
all three fail closed.** This is one rule applied consistently, and the consistency is the
design:

| File | Shipped? | Provisioned by | Behaviour when absent |
|---|---|---|---|
| `/etc/kiosk/kiosk.ini` | No — example at `/usr/share/kiosk/kiosk.ini.example` | runbook `install -m 0640` then edit | launcher non-fatal (defaults + `startup-degraded.txt`, `kiosk-launcher/src/main.rs:64-80`); `boot::load` renders `safe.html` |
| `/etc/kiosk/kiosk-credential.json` | No | `kiosk-provision-credential` | `credential_is_owner_only` → `Err` → `RenderSafe{reason: Some(CREDENTIAL_PERMISSIONS_REASON)}` (`boot.rs:161-190`); fetch loop `config_error` + `break` (`fetch.rs:100-106`) |
| `/etc/kiosk/kiosk-offline.mp4` | No | runbook | asset 404 (`main.rs:998-1012`), `offline.html` degrades to black splash → E's `media.error` bridge |

**Why no conffile — two verified traps.**

1. **A conffile under `/usr` is `file-in-usr-marked-as-conffile`, severity: error.** That
   fails G's own lintian gate. It is what killed the draft's "conffile-adjacent" mp4 (not a
   dpkg concept — withdrawn) and it applies to any shipped default asset.
2. **A modified conffile aborts a non-interactive `dpkg -i`.** Reproduced by both roles with
   `DEBIAN_FRONTEND=noninteractive … </dev/null`:

   ```
   *** kiosk.ini (Y/I/N/O/D/Z) [default=N] ? dpkg: error processing package cftest (--install):
    end of file on stdin at conffile prompt          rc=1
   ```

   `kiosk.ini` is the one file **100 % of devices must edit** (`dist-template/kiosk.ini` ships
   an empty `device_id =` and `replace-me` in `site`/`project_id`/`config_url`), so leaving it
   a conffile puts the guaranteed-modified file under dpkg's interactive machinery. The
   zero-conffile upgrade was reproduced end to end: `rc=0`, operator content intact at mode
   `640`. Upgrade preservation becomes total **by construction** rather than by a
   `--force-confold` an operator must remember.

**The example goes in `/usr/share/kiosk/`, not `/usr/share/doc/`.** Both roles reproduced the
trap: `/etc/dpkg/dpkg.cfg.d/excludes` carries `path-exclude=/usr/share/doc/*`, so a file
placed under `/usr/share/doc/kiosk/` is **silently dropped on install while `dpkg -L` still
lists it** — exactly the kind of thing a packaging test asserts on and is fooled by.
`/usr/share/kiosk/` is not excluded and the same test finds the file present. (The
`path-exclude` was verified on a minimized host; whether the `debian:12` CI image carries the
identical `dpkg.cfg.d` is tier 5 and nothing rests on it — `/usr/share/kiosk/` is correct
under either configuration.)

**Credential provisioning — the actual mechanism (G5).** The draft claimed
*"a directory an operator cannot `cp` a world-readable secret through."* **That is false and
is withdrawn.** Directory mode governs traversal; `umask` governs the mode of files created
inside. Reproduced:

```
$ chmod 0750 etckiosk ; chmod 0644 cred.json ; cp cred.json etckiosk/
750 etckiosk
644 etckiosk/cred.json          ← world-readable file inside the 0750 directory
$ install -m 0600 cred.json etckiosk/c2.json   → 600
```

So the package ships a thing that *sets* the mode rather than a sentence asking the operator
to remember flags: **`/usr/lib/kiosk/kiosk-provision-credential`**, three lines —
`install -m 0600 -o root -g root "$1" /etc/kiosk/kiosk-credential.json` — plus the
upgrade-time `chmod 0600` re-assert above. `/etc/kiosk` at `0750` is described as what it is:
a **traversal barrier only**.

*Honest restatement, adopted:* **the app's fail-closed gate is what enforces the mode; the
package supplies the traversal barrier and a provisioning command that sets it correctly.**
*C3 asymmetry, declared:* on Windows the MSI ships the file and sets its ACL
(`util:PermissionEx`); on Linux the installer ships no file and cannot set a mode on a path
that does not exist yet. Consequence: a mis-provisioned Linux device **sits in safe mode**
rather than never having been mis-provisioned.

*Rejected, recorded — F2's own mechanism must not be ported.* `dist-template/kiosk-credential.json`
has non-empty `client_email`, `private_key` and `token_uri`, and
`crates/kiosk-core/src/logging/auth.rs` `ServiceAccount::from_json` rejects only *empty*
fields. So F2's placeholder at `0600` yields `BootOutcome::Ready` (`boot.rs:186`) — **an
unprovisioned device boots reporting healthy** and fails invisibly at token exchange. Also
rejected: the draft's pre-created empty `0600` credential, which degrades the signal to
`reason: None` and removes the fetch-loop `break`. Ranked:

| Path state | `credential_is_owner_only` | Boot | Fleet visibility | Fetch loop |
|---|---|---|---|---|
| **absent (this design)** | `Err` | `RenderSafe{credential_permissions}` | spooled locally; uploaded retroactively **if the device is provisioned within the spool's retention window** (`SpoolConfig::from_logging`; `spool.dropped_expired` is the aging signal, parent §6). Otherwise on-screen `safe.html` and H8's cold-install step are the only signal | `break` |
| pre-created empty `0600` | `Ok(true)` | `RenderSafe`, `reason: None` | **permanently none** | keeps polling |
| F2 placeholder `0600` | `Ok(true)` | **`Ready`** | **permanently none** | keeps polling |

The absent-file row is deferred-visible with a *named draining mechanism*: `main.rs:808-825`
writes the same event directly to the local spool when `boot_fault_reason` is set — the
comment there says *"this needs no GCL client and no credential"* —
`telemetry::spool_boot_config_error` (`telemetry.rs:291-320`) appends to `<data>/spool/main`,
the same path `build` opens at `:224`, and the durability is tested
(`telemetry.rs:697 spool_boot_config_error_writes_a_durable_config_error_entry`). Rows 2 and 3
trip no boot fault and therefore spool nothing about the credential at all.

**mp4 not shipped (G6), four grounds recorded.** (i) `dist-template/kiosk-offline.mp4` is
**88 bytes of ASCII**, `OBVIOUSLY FAKE VIDEO PLACEHOLDER` — there is no asset to ship, and the
draft's "size vs completeness" open decision was framed around a file that does not exist.
(ii) Under G1 the app reads `/etc/kiosk/kiosk-offline.mp4`, so any shipped default lands in
`/etc` and forces either `file-in-etc-not-marked-as-conffile` or a binary blob as a conffile —
the same error-severity trap in a new place; Policy 10.7.3's second method ("created by
maintainer scripts", or here by the runbook) is the clean branch and upgrade preservation is
then free, since dpkg never touches a path it does not own. (iii) E's soak and F's per-PR
subset supply their own fixtures and never consume a package-provided mp4 — no cross-spec
break. (iv) Absence is caught: `main.rs:998-1012` 404s and `offline.html` degrades, which E's
`media.error` bridge spools, and H5/H8 gate it on hardware. postinst prints a warning if the
path is absent — nearly worthless on an unattended install, one line, does no harm.

#### Versioning and upgrade (G9)

Version from the workspace (`Cargo.toml` `version = "0.1.0"`); `Conflicts`/`Replaces` unused
for self-replacement (single package; the `Conflicts:` line above is the idle-daemon gate,
a different mechanism). With zero conffiles and three package-unknown paths, upgrade
preservation is a construction property, not a flag.

### 2. Lockdown runbook — `packaging/linux/lockdown.md`

The §7.2 Linux row expanded to runbook steps on stock Debian 12, per F2's `lockdown.md`
precedent: OS hardening is *documented and verified*, not postinst-automated, because half of
it is judgement calls an integrator must own. **Every step ends with a verify command** — that
is the runbook's discipline and H8 is its integration test.

#### VT / console / seat (G10)

`cage` is invoked **without `-s`**, and that is the strongest single step. cage 0.1.4-4
source: `cage.1.scd` *"`-s`  Allow VT switching"*; `cage.c:196` help text, `:238`
`server->allow_vt_switch = true;`; `seat.c:236-246`
`if (server->allow_vt_switch && sym >= XKB_KEY_XF86Switch_VT_1 …) { wlr_session_change_vt(…) }
else { return false; }` — the **only** `wlr_session_change_vt` call in the tree. **VT
switching is off by default in cage**, so §7.2's "disable VT switching and zap" is discharged
in-session and mechanically, not merely by logind settings.

*"Dedicated seat with no other TTYs" is a gate step, not an open fork.* The parent states it
as a deployment-gate requirement (§7.2:716-721) and the draft demoted it by conflating two
questions. Separated: **no other TTYs** is decidable now and independent of the service user;
**which seat / which user** is G16.

```sh
# logind: no auto VTs, no reserved VT, no getty on the kiosk seat
sed -i 's/^#\?NAutoVTs=.*/NAutoVTs=0/;s/^#\?ReserveVT=.*/ReserveVT=0/' /etc/systemd/logind.conf
systemctl mask getty@.service ; systemctl disable getty.target
# verify
ls /dev/tty[1-9]* 2>/dev/null && echo FAIL
systemctl list-units 'getty@*' --all --no-legend | grep -q . && echo FAIL
loginctl seat-status seat0
```

D's chord note lands here: chord *swallowing* is unnecessary under cage — VT switching is what
actually needs killing, and it dies in logind and in cage's own default, not in app code.
X11's `DontVTSwitch`/`DontZap` (the parent's parenthetical alternative) is one line in the
X11-is-demo-only appendix, for completeness; X11/openbox stays **documented but NOT
app-enforced**, parent §7.2 verbatim.

#### Display blanking — PF-07 / M8 / H5 (G11)

**cage 0.1.4 has no idle timeout and no blanking at all.** Verified from source, three ways:
the only `wlr_output_enable(…, false)` is `output.c:255-268` `output_disable()`, called only
from `:481` (multi-monitor layout handling, the `-m last` path), never from a timer; cage
creates `wlr_idle` (`cage.c:372`) and `wlr_idle_inhibit_v1` (`:379-386`) and calls
`wlr_idle_notify_activity` on input, but `wlr_idle` is a **notification** protocol
(`org_kde_kwin_idle`) that merely tells registered clients that N ms of idle elapsed;
`grep -i 'timeout\|timer'` over `cage.c`/`seat.c`/`output.c` returns nothing. Both roles
reproduced all three independently.

**So the parent's PRIMARY — "configuring cage/wlroots not to blank" — has nothing to
configure, and the requirement is discharged by preventing anything else from supplying a
blanker.** The enforcement is the dpkg `Conflicts:` line in G3: it binds continuously, at
every `apt` transaction, for the life of the device. *The draft's provisioning-time grep was
not durable and was not, as it claimed, "the only mechanism the source admits" — both
withdrawn.* The hazard is a package appearing **later**: `apt-mark hold` covers only
`libwebkit2gtk-4.1-0`, `unattended-upgrades` off stops automatic drift but not an operator's
`apt install` of something that pulls an idle daemon transitively, and H3 is a 24 h window at
validation time, before the hazard exists.

The grep survives as the runbook's **verify line**, because `Conflicts:` binds packages, not
hand-started processes:

```sh
dpkg -l | grep -E 'swayidle|xautolock|xscreensaver|light-locker|gnome-screensaver' && echo FAIL
systemctl list-units --type=service --state=running | grep -iE 'idle|screensaver|power-manager' && echo FAIL
```

*Residual, declared:* `Conflicts:` means `apt install <something that pulls swayidle>` will
propose **removing `kiosk`**, and `apt -y` would proceed. That is loud, it is the refusal
signal, and it is bounded by G12's update discipline (`unattended-upgrades` off, SSH absent by
default).

**Parent §11's row — "confirm cage honours idle-inhibit before relying on it" — is answered
NEGATIVELY and recorded here.** cage's `zwp_idle_inhibit_v1` does exactly one thing
(`idle_inhibit_v1.c:34-35`):

```c
bool inhibited = !wl_list_empty(&server->inhibitors);
wlr_idle_set_enabled(server->idle, NULL, !inhibited);
```

It toggles **cage's own `wlr_idle` notifier** — the notifier nothing on the device consumes —
and has no relationship to logind inhibitor locks. The answer is not "cage ignores it" but
**"there is nothing for it to inhibit."** G produced the evidence and G closes the row.

**Consequences, stated rather than left for the reader.** (a) **H3 loses
`systemd-inhibit --list` as a keep-awake proof.** It proves a lock is held, not that the lock
does anything; it is retained only as a regression check that B's spawn path ran, relabelled
as such. H3's keep-awake evidence is the 24 h observation plus the `Conflicts:` gate. (b) **B's
`systemd-inhibit --what=idle:sleep --mode=block cat` child is defence-in-depth with no current
effect** under this runbook: `sleep` because G12 masks all four sleep targets (a masked target
cannot be entered, lock or no lock), `idle` because a `what=idle` lock blocks logind's
`IdleAction`, which is at its stock `ignore`, and because cage never raises the idle hint that
would reach logind. **Keep the child** — one `cat`, and it is the only mechanism still acting
if an operator unmasks a sleep target. B's belt/suspenders framing is inverted relative to
parent §7/§11, which name the compositor configuration PRIMARY; that correction is recorded in
P2-B and carries no code change.

`consoleblank=0` and `IdleAction=ignore` are **demoted from keep-awake mechanisms to belts**:
kernel 6.1 documents `consoleblank=` as defaulting to 0 and scopes it to the VT console, and
`/etc/systemd/logind.conf` ships `#IdleAction=ignore` — already the default. Set both
explicitly so an inherited cmdline or a stray drop-in cannot un-default them; assert, do not
rely.

*Residuals, declared:* panel-side power management (the monitor's own OSD sleep timer) is
outside every software layer — hardware, H3's job to observe. And if a device class forces a
compositor other than cage 0.1.4/0.1.5, none of this analysis transfers and the step must be
re-derived; owner = H1.

#### Sleep / idle, cosmetics, updates, SSH (G12)

- `systemctl mask sleep.target suspend.target hibernate.target hybrid-sleep.target`;
  verify `systemctl is-enabled sleep.target` → `masked` for all four.
- Quiet boot (`quiet`, cursor blink off); verify against `/proc/cmdline`.
- **journald `Storage=persistent`** plus a `SystemMaxUse` stated as a **computed floor**, not a
  round number: sized to retain ≥7 days at `RestartSec=30` (2,880 unit starts/day). Persistent
  storage is what makes the first boot's failure survive a reboot; the floor is what makes it
  survive the loop. Verify with `journalctl --disk-usage` and
  `journalctl -b -1 -u kiosk.service | head`.
- `unattended-upgrades` **off** — update timing is operator-owned (F §4). Verify:
  `systemctl is-enabled unattended-upgrades` → `disabled`/`masked`.
- **WebKitGTK pin:** `apt-mark hold libwebkit2gtk-4.1-0`, released only through the runbook's
  revalidation loop. Verify: `apt-mark showhold`. This is the parent's pinned-image intent
  (§3.4:289 and §10:874 — *not* §9, whose only Linux text is the platform floor; the draft's
  citation was wrong) expressed as a package hold on stock Debian.
- SSH keyed-only if enabled; the default recipe leaves it **absent**. Verify:
  `systemctl list-unit-files 'ssh*'`.
- **Recovery step (INT-4), documented rather than discovered:** *"If the screen is frozen and
  the device has not recovered within 60 s, power-cycle. On a conforming image there is no VT,
  no getty and no SSH by design (G10/G12); a power cycle is the supported recovery, not a
  workaround."* Observed by H11.
- F2's fourth error-handling bullet, carried forward: *"on a freshly installed device stuck
  black, suspect `kiosk.ini` / the credential first — `safe.html` appearing is not a
  prerequisite for a config problem."*

#### On-screen keyboard — ruling R2, and ledger item I1

**This build ships no on-screen keyboard, and G does not discharge the parent §7 Linux
keyboard obligation.** Stated unhedged, because the achievable fraction of "deployment docs"
is precisely a hard constraint an integrator must check before deployment.

**Erratum — parent §7 keyboard row, Linux cell.** The cell reads *"Linux:
squeekboard/onboard deployment docs"*. Both named mechanisms are non-viable under the
compositor parent §7.2 mandates, and Moderator ruling **R2** records this as an erratum on the
same standard as R1.

- **squeekboard cannot run.** cage's complete protocol surface is its `*_create(` list —
  enumerated exhaustively on **cage 0.1.4** at `cage.c:297-455`: `wl_display`,
  `output_layout`, `compositor`, `data_device_manager`, `seat`, `wlr_idle`,
  `wlr_idle_inhibit_v1`, `xdg_shell`, `xdg_decoration_manager_v1`,
  `server_decoration_manager`, `export_dmabuf_manager_v1`, `screencopy_manager_v1`,
  `xdg_output_manager_v1`, `gamma_control_manager_v1`, `xwayland`, `xcursor_manager`. The
  upstream file list has no `layer_shell.c` and no `input_method*.c`. squeekboard needs
  `zwlr_layer_shell_v1` to place itself and `zwp_input_method_v2` to type.
- **The load-bearing limb is layer-shell, and only layer-shell (INT-5).**
  `zwlr_layer_shell_v1` is absent on **cage 0.1.4 and cage 0.1.5** —
  `grep -iE 'layer_shell|input_method|text_input'` returns nothing against either — so **no
  separate-process OSK can place itself over a fullscreen client on either version.**
  **Correction, carried:** `wlr_virtual_keyboard_manager_v1_create` **is present in cage
  0.1.5** (`strings /usr/bin/cage | grep virtual` → `wlr_virtual_keyboard_manager_v1_create`,
  `wlr_virtual_pointer_manager_v1_create`); it is the `zwp_virtual_keyboard_manager_v1`
  global. The virtual-keyboard limb is therefore **withdrawn from the ruling's evidence base**
  and the input-method / text-input limbs are scoped to 0.1.4 (the C7 floor). **R2's
  conclusion is unchanged; its evidence narrows to one limb.** *Display* is blocked on both
  versions; *injection* is not blocked on 0.1.5.
- **The derived ActivityClock claim is withdrawn.** The draft said *"any separate-process OSK
  produces no GDK events in our process and would break D's `ActivityClock`."* True of the
  **XTEST/Xwayland route only** — `onboard` runs under cage's Xwayland (`cage.c:446`
  `wlr_xwayland_create`, present on both versions), but XTEST events injected into Xwayland do
  not reach our native Wayland client. It is **false** for a `zwp_virtual_keyboard_v1` client,
  which injects at the seat, so the compositor delivers real `wl_keyboard` events to the
  focused client and D's handlers see real GDK events. The correct sentence: *an XTEST-based
  OSK (onboard under Xwayland) produces no GDK events in our process and would break D's
  `ActivityClock`; a virtual-keyboard client would not, but has no way to display itself under
  cage on either version.*

**The `inject_js` route is withdrawn entirely** — not the default, not the fallback, not
mentioned as an option. The draft called it *"a named P2-row deliverable … so the mechanism
has an owner"*; being named in the parent's P2 row is not the same as being owned by a spec.
`grep -rniE "inject_css|inject_js|RT-16"` over all seven P2 specs returns zero, and
`crates/kiosk-core/src/config/validate.rs:15-21` still carries
`("content.inject_css","P2"), ("content.inject_js","P2")` in `UNIMPLEMENTED`, so an operator
who sets `inject_js` today gets an RT-08 `config.warn` and **no behaviour**. Discharging G's
obligation with it would have moved the gap one indirection further from view.

**The obligation is ledger item I1**, not G's prose, and not a second unowned row: the
keyboard and RT-16's knobs are **the same gap in the same file**,
`crates/kiosk-main/src/inject.rs` (shipping and host-tested, wired at `main.rs:1041-1046`).
A **bundled, always-on** keyboard needs no live reinjection and therefore does **not** depend
on RT-16 landing — `inject.rs:12-18` records that `initialization_script` *"may be called only
ONCE per webview … there is no live-reinjection path, by design"*, which is why the operator
knob is `UNIMPLEMENTED` while a bundled control (the cursor-autohide timer already inside
`build_injection`) is not. Owner: whoever picks up RT-16; fallback, a new `inject.rs`-scoped
sub-project. Phase P2 unless the parent defers RT-16. **G is not the owner** — this is code in
`kiosk-main`, and a usable OSK is layout + shift/symbols + focus tracking + viewport shift, a
feature and not a line; P2-D disclaims it explicitly (`p2d:26`, `:162`).

**Scope finding that bounds the gap.** `grep -n '<input\|<textarea\|contenteditable'
crates/kiosk-main/bundled/*.html` → **zero hits across all five pages**; `pinpad.html` is a
`<button>` grid writing into a `<div>`, not a text field. **No app-owned surface in this
product has a text input**, so nothing P2-G installs is broken today; the gap is confined to a
**deployed site** that renders text inputs on touch hardware. Parity note (C3): `grep -rn -i
"tabtip\|InputPane"` over `crates/` returns nothing — Windows shipped P1 with PF-02 open too,
so Linux is not diverging downward.

**Runbook section `On-screen keyboard`, shipped regardless of the ruling:**

1. **squeekboard ruled out, with the protocol evidence above**, so nobody re-litigates it.
2. **`onboard` + `GDK_BACKEND=x11` inside cage's Xwayland — the fallback-if-the-§7-cell-binds**,
   labelled as such and not as an appendix curiosity, with its cost stated: it forfeits the
   Wayland input path P2-D is built on and the GDK event stream D's `ActivityClock` depends
   on.
3. **`Depends:` — none, stated explicitly.** No installable package solves this under cage.
4. **The operator-facing prerequisite, unhedged:** *"This build ships no on-screen keyboard. A
   deployed site that requires text entry on a touch device must render its own input UI.
   Verify this against the site before deployment — it is a deployment prerequisite, not an
   app capability."* Verify: walk the site's forms on the device before sign-off; record the
   result in **H4b**.

#### Service user and seat access (G16)

**Shipped default: root** — the unit carries no `User=`; `/etc/kiosk` and `/var/lib/kiosk` are
`root:root`, the credential `0600 root:root`. That is parent §4:418-420 and §8:740's literal
Linux wording (*"`root:root 0600` or the keyring"*), it is the least mechanism, and it is the
only variant needing no additional package for DRM/seat access.

> **Windows-parity divergence (C3), looser direction.** Windows ships an *unprivileged* kiosk
> account (§7.2: *"Autologon to a locked, unprivileged kiosk local account"*). Linux ships
> **root** by default. Justification: root is the only variant needing no additional package
> for DRM/seat access, and it is the form parent §4 and §8 name for the credential. The
> non-root `seatd` recipe is written below and H1 promotes it. **This is a weaker posture than
> Windows and is not the project's normal posture** — it is a shipped default pending hardware
> evidence, not a decision that root is acceptable.
>
> *Scope note:* the non-root variant does **not** close the renderer→credential path.
> `WebKitWebProcess` runs as the same uid either way, so a compromised renderer reads a
> `0600 kiosk:kiosk` credential exactly as it reads `0600 root:root`. The delta is everything
> else root can do — write `/usr`, load modules, `/dev/mem` — which the mitigation list (cage
> input ownership, SEC-10 egress, signed config with a pinned key, SEC-08 physical
> prerequisites) does **not** address: those are escape mitigations, not privilege
> mitigations.

**The hardened recipe is fully specified, so H1 is a promotion and not a redesign.** `seatd`
exists in Debian 12 (`0.7.0-6`), and cage builds against `libwlroots-dev (>= 0.14.0)` (its
`debian/control`) — i.e. the libseat-based wlroots, so a non-logind seat backend is available
without a login session.

```sh
adduser --system --group kiosk
usermod -aG _seatd,video,input,render kiosk
systemctl enable --now seatd ; loginctl seat-status seat0     # verify
chown -R kiosk:kiosk /etc/kiosk /var/lib/kiosk
stat -c '%a %U:%G' /etc/kiosk/kiosk-credential.json           # verify → 600 kiosk:kiosk
```

**Flip-list — exactly what changes if H1 promotes it**, named now: the two `chown`s; `User=`;
`SupplementaryGroups=video,input,render,_seatd`; `Wants=seatd.service` / `After=seatd.service`
in `[Unit]`; `Depends: seatd` in control; and P2-A's `ponytail:` uid check activates (H9 —
A's mode-bits check is uid-agnostic, `mode() & 0o077 == 0`, so once owner and reader can
differ, mode bits stop proving anything). **The two `chown`s are operator-applied and postinst
never re-asserts ownership after first install** (G4/G7's `[ -z "$2" ]`), which is what makes
the flip survive upgrades.

### 3. OS image position (G13)

Runbook-on-stock-Debian-12 **is** the P2 image story (approved in session — exact parity with
how Windows shipped: MSI + `lockdown.md`, no golden image). The runbook ends with a capture
step, and that capture is the "pinned image" parent §3.4:289 / §10:874 speak of, in
reproducible-enough form for a small fleet:

```sh
dpkg -l > snapshot-$(date +%F).txt                                     # human reading
dpkg-query -W -f='${binary:Package}\t${Version}\t${db:Status-Abbrev}\n' \
  > snapshot-$(date +%F).tsv                                           # machine-diffable
cage -v >> snapshot-$(date +%F).tsv                                    # the floor, recorded
```

Config diffs are archived alongside, per validated device class. Automated image building
(preseed/FAI/debos) is the recorded ponytail, promoted only when fleet size makes hand-run
runbooks the bottleneck. Checklist results append per device class — **the validation is the
image pin.**

## Hardware validation checklist (G14)

Parameterized on device class; every row cites its origin, and every row that a sibling spec
defers to exists here by ID.

| # | Item | Origin |
|---|---|---|
| H1 | Real cage boot chain: unit → cage → launcher → main; fullscreen on the physical display; `-m last`/`-m extend` behaviour recorded. **Promotes or rejects the G16 service-user fork.** | A `p2a:339-342` — a *plan-time* item A expected to settle under weston, escalated to hardware because tao's monitor behaviour is display-dependent |
| H2 | `RestartPreventExitStatus=86` **and `SuccessExitStatus=86`** end-to-end via systemctl: technician exit stays exited **and** `systemctl status` reads `inactive (dead)`, not `failed`. Plus **`is-active` → `active` after boot** — the assertion no container can make (G15) | C `p2c:155-156` (smoke 14's systemd half); G8's `StartLimitIntervalSec=0` residual is pinned here |
| H3 | Keep-awake positive: G11's no-idle-consumer gate re-verified on the device; display never blanks over 24 h; panel OSD sleep timer observed. `systemd-inhibit --list` is a **regression check that B's spawn path ran**, not a keep-awake proof | B `p2b:193-196` + G11 |
| **H4a** | **Touch (the restored touch row):** corner-tap opens the pin pad on the device's own touch panel. Record: taps counted per single-finger tap; taps counted per N-finger tap (Windows counts 1 — the over-count is D's declared C3 divergence); whether `GDK_TOUCH_CANCEL` is emitted at all on this panel | D5/D11 (`GDK_TOUCH_CANCEL`, N-finger deadband); smoke 17's cage-headless fallback |
| **H4b** | **Text entry:** verify the deployed site's text-entry surfaces on the device class; record whether any input has no usable keyboard | Ledger **I1** discoverability; parent §7 keyboard row |
| H5 | ≥72 h offline-video soak, RSS trend, loop count; visual black-frame check | E `p2e:99` / RT-05 |
| H6 | Escape-vector sweep under the locked session: parent §7.2's vectors (VT chords, zap, sleep) plus §7's shortcut/dialog/edge rows | parent §7.2 + §10:879-881. **Corrected:** §10 does not enumerate a hardening list, it points at `docs/testing.md`, which does not exist — the checklist file G creates is `docs/testing/linux-hardening-checklist.md`, matching the existing directory convention |
| H7 | Egress + nav guard against a real network: DNS failure modes, captive-portal interference | parent §3.3 (captive portals) + §7 SEC-10. *"A/B" was invented and is withdrawn*; the row stays because SEC-10's residual-gap documentation is a P2 obligation and B's smoke runs against a local httpd only |
| H8 | Runbook executed cold on the device class, timed; both snapshots captured; mp4 and credential provisioning steps verified. **And (INT-8): after first boot and before sign-off, read the local spool and assert it contains no `egress.filter_absent` and no `egress.csp_absent`** | G §2–3; B's C4-vs-C5 resolution rests on loudness this provisioning model must actually deliver |
| H9 | Under the promoted service user (G16): credential readable, `/var/lib/kiosk` writable, spool drains, `/run/kiosk` socket reachable | A's `ponytail:` `p2a:275-276` — *"add an owner check if a non-root service user lands"* — carried forward with an owner for the first time |
| **H10** | **Pinch-zoom does not scale the page on touch hardware; two-finger pan/scroll still works.** Second clause failing ⇒ D13's recorded `scale_delta()` deadband is the fix | D13 / PF-04, parent §7 zoom-lock row |
| **H11** | **Wedged-compositor recovery:** with cage `SIGSTOP`ped, confirm the device does **not** self-recover and that the documented recovery step (power cycle, G12) restores service. Record time-to-detect by an on-site observer | C12's wedged-cage residual (`p2c:494-504`); also carries C17's residual — a wedged cage is unreachable by the JS-ping because the compositor holds the DRM device |

**Why H4 split.** It began as the touch row and eroded into the text-entry row across two
revisions while a sibling kept routing touch deferrals into it. H4a restores the touch content
verbatim; H4b keeps the text-entry text. Both IDs are now cited by name from P2-D.

## Testing (G15)

Both gates have a named runner, and the split is drawn where the mechanism actually falls.

**What a systemd-less container can and cannot answer**, measured (`ps -p 1 -o comm=` →
`process_api`, `/run/systemd/system` absent): `systemctl --root=… is-enabled` answers — it is
a filesystem query, rc=0. `is-active` does not: *"System has not been booted with systemd as
init system (PID 1). Can't operate."* (and with `--root=`, *"Verb 'is-active' cannot be used
with --root="*). That line is exactly where the missing-`[Install]` regression lives, which is
what makes the split worth drawing rather than deferring everything.

**F's nightly `debian:12` container job (unchanged runner) asserts:**

- install / remove / purge / upgrade cycle completes, rc=0, non-interactive, stdin closed.
- `test -x /usr/lib/kiosk/kiosk-launcher` and `test -x /usr/lib/kiosk/kiosk-main`. **Required:**
  with `cage` as the `ExecStart` command the launcher is an *argument*, so
  `systemd-analyze verify` never checks its path — verified, rc=0 with the launcher absent
  versus rc=1 when the launcher is the command. This is the cheapest cover for the exact hole
  G1's path change opens.
- Modes: `/etc/kiosk` and `/var/lib/kiosk` `0750`; `/usr/share/kiosk/kiosk.ini.example` present.
- **All three operator files absent on a fresh install** — `/etc/kiosk/kiosk.ini`,
  `/etc/kiosk/kiosk-credential.json`, `/etc/kiosk/kiosk-offline.mp4`. That is the provisioning
  contract, asserted rather than implied.
- Upgrade preserves an operator-written `kiosk.ini`, an operator-placed credential and mp4;
  and `deb-systemd-helper` does **not** re-enable a unit the test disabled first.
- `systemctl --root=/ is-enabled kiosk.service` → **`enabled`** (the missing-`[Install]` guard).
- `systemd-analyze verify` on the shipped unit (static analysis, no PID 1 required) — this is
  how the `StartLimitIntervalSec` placement defect was found and how a misplaced key stays
  found.
- `grep -R` over the built `.deb` finds zero `BEGIN PRIVATE KEY` and no `kioskctl`.
- `grep -q '\${shlibs' <extracted control> && FAIL` — the substvar-not-substituted trap.
- **`pkill -9 kiosk-launcher; sleep 2; ! pgrep kiosk-main`** — C12's orphan-kill gate, which
  closes verification finding V4 for real. It belongs here, not in an H row: H rows are human
  hardware checks, this one is mechanical and runs where a `.deb` is installed and processes
  actually exist.
- **`cage -v` emitted and asserted equal to the recorded floor `0.1.4`** — C10/C15's cage-floor
  assertion. (`cage -v`, not `cage --version`, which exits 1 with `invalid option -- '-'` and
  would abort the script under `set -e` — ruling R3.)

**F's release job:** `dpkg-shlibdeps` → `dpkg-gencontrol` → `dpkg-deb -b`, then
`lintian --fail-on error`. Overrides live in `debian/source/lintian-overrides` with a comment
each. After G1 and G6, zero error-severity tags are expected — which is the point of both
changes.

**Not assertable before hardware, and owned:** the unit reaching `active`, cage obtaining DRM
master, seat/session availability, `RuntimeDirectory` under a non-root uid, library resolution
at exec time. All of those are H1/H2. Frame §4.5 disposition, declared not assumed.

**The runbook is testable prose:** every step ends with a verify command; H8 is the integration
test.

## Error handling / edge cases

Mirrors F2's section shape. Install on a system with the service already running: the upgrade
path is `deb-systemd-invoke restart` under debhelper's `[ -n "$2" ]` discriminator, itself
inside the `[ -d /run/systemd/system ]` guard, with `|| true`. Missing WebKitGTK runtime
version versus the hold: the dependency solver handles it; the runbook's hold step documents
the downgrade-refusal case. Credential present-but-wrong-mode after a manual operator edit:
postinst re-asserts the mode on upgrade (mode only, never ownership), and A/C's runtime gate is
the real enforcement — a wrong mode is caught at the next boot *and* on every fetch poll.
Disk-full on `/var/lib/kiosk`: the spool's existing degradation, nothing package-level to add.
An operator who relocates the credential via `credential =` in `kiosk.ini` loses the
upgrade-time re-assert — declared assumption, narrowed: postinst acts on the template default
path only (`config_dir.join(&bootstrap.credential)`, `main.rs:730`), the runbook states *"if
you change `credential =`, you own that file's mode"*, G15 asserts the default path only, and
the residual is bounded by the app's fail-closed gate.

## Residual risks — each with a named carrier

| Risk | Carrier |
|---|---|
| **Linux touch keyboard — NOT discharged.** Parent §7 Linux cell is an erratum (ruling R2); both named packages impossible under cage | **Ledger item I1** — HIGH, open, owner = whoever picks up RT-16 (fallback: a new `inject.rs`-scoped sub-project), phase P2. Discoverability: the runbook prerequisite + **H4b** |
| Parent §4 install-path erratum awaiting an owner-level amendment | **Ruling R1**; fallback stated and survivable (`/opt/kiosk/` + documented lintian override) |
| Root by default is looser than Windows' unprivileged kiosk account | C3 divergence declared in G16; **H1** promotes or rejects the `seatd` recipe |
| `Conflicts:` removal risk — `apt -y` pulling an idle daemon proposes removing `kiosk` | Loud, not silent; bounded by G12's update discipline |
| `StartLimitIntervalSec=0` — a permanently broken install loops at 30 s forever | `Storage=persistent` + a computed ≥7-day journal floor preserve the cause; **H2** |
| Unprovisioned device is invisible to the fleet (spool retention window) | On-screen `safe.html` + **H8**'s cold-install step, which also asserts no `egress.filter_absent` / `egress.csp_absent` |
| Panel-OSD sleep timer is outside every software layer | **H3** observes it |
| Compositor other than cage 0.1.4/0.1.5 on a future device class ⇒ G11's analysis does not transfer | **H1** |
| G1's dependency on C's Linux `spawn_main` carrying `--config` | Fail-closed (safe mode) if absent; declared in both registers — **C5 ↔ G1** |
| Wedged cage: unit stays `active`, orphan survives, launcher not restarted; C17 cannot reach it | **H11** + the runbook power-cycle line |
| Unpinned until hardware: cage on a real DRM seat, the non-root uid path | **H1 / H2** |

## Open decisions to resolve at plan time

Values and file layout only; no mechanism is left unpinned.

- The exact `SystemMaxUse` byte value satisfying the ≥7-day floor at `RestartSec=30` on the
  chosen device class's journal volume (the *arithmetic* is fixed; the input is per-class).
- Whether `packaging/linux/` assembles via `debhelper`/`dh` or a hand-rolled tree feeding the
  three named tools — the three tools and their order are fixed either way.

## Change register and cross-spec edges

| ID | Change | Discharges | Depends on |
|---|---|---|---|
| G1 | `/usr/lib/kiosk/` binaries+assets, `/etc/kiosk/` operator files, joined by `--config`; parent §4 erratum named and escalated; `/opt` + lintian override the stated fallback | parent §4 (both rows), Policy 9.1.1 / 10.7.2 | **Ruling R1**; C11's `ExecStart`; **C5's Linux `spawn_main` must carry `--config`** |
| G2 | Payload = 2 binaries + `bundled/` + unit + `kiosk.ini.example` + `kiosk-provision-credential`; `kioskctl` withdrawn | parent §9 P2 (".deb"); SEC-08/SEC-11 | G1 |
| G3 | `${shlibs:Depends}` via `dpkg-shlibdeps` → `dpkg-gencontrol` → `dpkg-deb -b`; `cage` + 4 GStreamer hand-written; `Conflicts:` on idle daemons; no keyboard `Depends:`, stated | parent §3.4 verbatim; PF-05; PF-07 | F release job (three tools named) |
| G4 | `/var/lib/kiosk` `0750`; create + `chown` **first-install-only** (`[ -z "$2" ]`), modes-only on upgrade | parent §4 row 3; A's `resolve_data_dir` | G16, P2-A |
| G5 | Ship nothing at the credential path; `/etc/kiosk` `0750` a **traversal barrier only**; `kiosk-provision-credential` sets the mode; upgrade-only `chmod` re-assert; C3 asymmetry vs F2 declared; visibility bounded by spool retention | SEC-09; parent §8 | G1, G16, P2-A |
| G6 | mp4 operator-provisioned; postinst warns on absence; no conffile; four grounds recorded | parent §3.4, §9 (silent-black-video class); Policy 10.7.3 | G1, E |
| G7 | debhelper autoscripts verbatim: `deb-systemd-helper enable` (not `systemctl enable`) + `deb-systemd-invoke start`/`restart`; `was-enabled` guard; `[ -d /run/systemd/system ]` on start only | parent §3.1 autostart, arch-05 | G8 |
| G8 | Unit values + `[Unit]` + **`[Install] WantedBy=multi-user.target`**; `StartLimitIntervalSec=0` in `[Unit]`; `SuccessExitStatus=86` with `RestartPreventExitStatus=86`; `RestartSec=30`; `RuntimeDirectory=kiosk` | arch-05; parent §3.1:170-173 verbatim | C11's `[Service]` shape |
| G9 | **Zero conffiles**; example at `/usr/share/kiosk/` (not `/usr/share/doc/`, `path-exclude`d) | F2 upgrade-idempotence precedent | G1, G5, G6 |
| G10 | cage invoked without `-s`; "no other TTYs" a gate step; X11 `DontVTSwitch`/`DontZap` in the demo-only appendix | parent §7.2 Linux verbatim | D (chord note lands here) |
| G11 | `Conflicts:` as continuous enforcement, grep as the verify line; parent §11 closed **negatively**; H3 loses `--list`; B's child relabelled | PF-07 / M8 / H5; parent §7 keep-awake row, §7.2 DPMS | B (finding stated against it) |
| G12 | Sleep-target masking, cosmetics, journald `Storage=persistent` + computed floor, `unattended-upgrades` off, WebKitGTK `apt-mark hold`, SSH, the recovery step | parent §7.2, §3.4:289, §10:874; F §4 | F §4; C17 residual |
| G13 | `dpkg -l` + `dpkg-query -W -f=…` + `cage -v` capture per device class | parent §3.4, §10 | G12 |
| G14 | **H1–H11**, with H4 split into **H4a** (touch) and **H4b** (text entry); H8 gains the SEC-10 spool assertion | parent §10; A–E deferrals | G8, G11, G16 |
| G15 | Container assertions (incl. `pkill -9` orphan-kill and `cage -v` floor) on F's nightly `debian:12`; `active` moved to H2; lintian + the three-tool pipeline on F's release job | C9; parent §10 | F nightly + F release |
| G16 | Default root (no `User=`); C3 Windows-parity divergence declared; full `seatd` recipe + flip-list; postinst never re-asserts ownership after first install | parent §7.2 (dedicated seat); SEC-09 uid interaction | A `p2a:275-276`; H1 |

**Edges, both directions.** C5 → G1 (`--config` forwarding; fail-closed if absent).
C11 → G8 (the `[Service]` shape; G supplies `[Unit]`, `[Install]` and every value).
C12 → G15 (`pkill -9` orphan-kill assertion, closing V4). C10/C15 → G15 (`cage -v` equals the
recorded floor `0.1.4`). C12/C17 residual → G H11 + G12's runbook power-cycle line.
D3 → G10 (the chord sentence's reserved slot). D13 → G H10 (PF-04 pinch intercept).
D5/D11 → G H4a (`GDK_TOUCH_CANCEL`, N-finger count), and smoke 17's cage-headless fallback.
E7 → G6 (the mp4 path). B9 → G11 (relabelled inert defence-in-depth; labelling only, no code).
B's C4-vs-C5 loudness → G H8 (the spool assertion that makes it real). G15 → F5/F8/F12
(the container assertions, the three-tool pipeline, `lintian --fail-on error`) — F's nightly
and release jobs each need a stated addition, declared as a dependency on F, not assumed.
G lands **after** C, B, D and E and **before** F in the committed merge order.

## Scope / defer

Automated image build (recorded ponytail, promoted by fleet size); apt repo / fleet update
mechanics (F's ponytail); Android packaging (P3); target-hardware selection (explicitly TBD —
the checklist is hardware-parameterized and ready for whatever the answer is). The Linux touch
keyboard and RT-16's `inject_css`/`inject_js` knobs are **one gap in one file** and are ledger
item **I1**, owned outside this spec at owner level, phase P2; G contributes the deployment
prerequisite stated unhedged and H4b's per-device-class enumeration, and claims neither as a
discharge.
