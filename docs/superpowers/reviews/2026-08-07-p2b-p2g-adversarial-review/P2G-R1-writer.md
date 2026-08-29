# P2-G — WRITER, Round 1 (opening)

No frame dispute.

Everything below marked **[V]** was re-verified by me in this turn (commands/citations
inline). Where I reach the verifier's conclusion by my own check I say so; where I found
evidence the verifier did not have, I say that too.

---

## Change register

| ID | Change | Requirement discharged | Depends on |
|---|---|---|---|
| G1 | Install layout: binaries+assets `/usr/lib/kiosk/`, operator files `/etc/kiosk/`, joined by the already-shipped `--config` flag | parent §4 (both rows), Policy 9.1.1/10.7.2 | C's `ExecStart` (must change); parent §4 table row 1 Linux cell (must change) |
| G2 | `.deb` payload = 2 binaries + `bundled/` + `kiosk.service`. `kioskctl` withdrawn | parent §9 P2 (".deb"), SEC-11 | G1 |
| G3 | Dependency list unchanged (all 7 verified in bookworm main) | parent §3.4 verbatim, PF-05 | — |
| G4 | `/var/lib/kiosk` `0750`, owner follows G16 | parent §4 row 3; A `resolve_data_dir` | G16 |
| G5 | Secrets: ship nothing at the credential path; package sets the *directory* default; runbook `install -m 0600`; postinst re-asserts mode on upgrade only | SEC-09, parent §8; F2 §1's "installer makes the mode the default" | G1, G16, boot/fetch gates |
| G6 | Offline mp4 not shipped; operator-provisioned to `/etc/kiosk/`; postinst warns if absent. No conffile | parent §3.4, §9 (silent-black-video class), Policy 10.7.2 | G1; E (soak needs a real asset) |
| G7 | Autostart: `deb-systemd-helper enable` (once) + `deb-systemd-invoke start` / `restart` | parent §3.1 autostart, arch-05 | G8 |
| G8 | Unit values: `[Unit] StartLimitIntervalSec=0`; `[Service] RestartSec=5`, `Restart=always`, `RestartPreventExitStatus=86`, **`SuccessExitStatus=86`**, `RuntimeDirectory=kiosk` | parent §3.1 verbatim (both exit-86 directives) | C's unit *shape* (G supplies values + the `[Unit]` section C's shape lacks) |
| G9 | Versioning + upgrade preservation (now: exactly one conffile) | F2 upgrade-idempotence precedent | G1, G5, G6 |
| G10 | Runbook — VT/console/seat: `cage` invoked **without `-s`**; `NAutoVTs=0`/`ReserveVT=0`; mask `getty@.service`; "no other TTYs" promoted from open fork to gate step | parent §7.2 Linux verbatim | D (chord note lands here) |
| G11 | Runbook — blanking: the real mechanism is "no `wlr_idle` client on the box", because cage 0.1.4 has no idle timeout of its own. Verify step + H3 | parent §7 keep-awake row (PRIMARY), §7.2 DPMS, PF-07/M8 | B (hands the suspenders to G) |
| G12 | Runbook — boot cosmetics, `unattended-upgrades` off, `apt-mark hold libwebkit2gtk-4.1-0`, SSH | parent §7.2, §9; F §4 | F §4 |
| G13 | Image position: `dpkg -l` + config-diff capture per device class; citation corrected to §3.4/§10 | parent §3.4, §10 | G12 |
| G14 | H1–H8 revised: origins re-attributed, H3 given a producing step, H9 added (credential/data-dir ownership under the promoted service user) | parent §10; A–E deferrals | G11, G16 |
| G15 | Install/remove/upgrade + lintian gate given a named runner: F's **nightly `debian:12`** job (cycle) and F's **release** job (lintian) | C9, parent §10 | F §2(a), F §3 — both need a stated addition |
| G16 | Service user: shipped default **root** (no `User=`); non-root `seatd` recipe fully specified in the runbook; H1 promotes | parent §7.2 (dedicated seat), SEC-09 uid interaction | A's `ponytail:` uid deferral (`p2a:275-276`) |

---

## G1 — Install layout, and the parent-spec divergence

**Proposal.** Binaries + `bundled/` + the unit → `/usr/lib/kiosk/`. `kiosk.ini`,
credential, offline mp4 → `/etc/kiosk/`. The unit's `ExecStart` carries
`--config /etc/kiosk`. Data dir `/var/lib/kiosk` unchanged.

```ini
ExecStart=cage -- /usr/lib/kiosk/kiosk-launcher --config /etc/kiosk
```

**Requirement.** Parent §4 row 1 (install dir), row 2 (`kiosk.ini`/credential/mp4 —
"next to binaries (**override: `--config <path>`**)"), §8/SEC-09 (credential mode).

**Evidence.**

- **[V] The parent's `/opt/kiosk/` cannot be shipped in a Policy-clean `.deb`.** Lintian
  tag `dir-or-file-in-opt` — I fetched the tag page: *"Debian packages should not install
  into /opt, because it is reserved for add-on software. **Severity: error**"*
  (lintian 2.139.0). That is the **same severity** as the conffile tag the verifier used
  to defeat my mp4 plan (§7 of the report). Conforming to the parent's literal cell
  trades one lintian *error* for another. Tier 5, but uncontested by tiers 1–4.
- **[V] `/usr/lib/<pkg>/` is explicitly blessed.** Debian Policy 4.7.4.1 §9.1.1,
  exception 1, verbatim (fetched `policy.txt`, lines 6787-6793): *"a subdirectory of
  `/usr/lib` may be used by a package (or a collection of packages) to hold a mixture of
  architecture-independent and architecture-dependent files."* Our payload is exactly
  that mixture (two ELF binaries + five HTML files). **[V]** `grep -n libexec policy.txt`
  returns **nothing** — `/usr/libexec` has no Policy standing at all, so C's current
  choice is the weaker of the two.
- **[V] Config under `/usr` is a real conflict, and `/etc` is free.** `resolve_config_dir`
  is the binary dir unless overridden — `crates/kiosk-main/src/main.rs:423-431`; consumers
  `main.rs:655` (`kiosk.ini`), `:730` (`config_dir.join(&bootstrap.credential)`),
  `:999` (`kiosk-offline.mp4`). I read all four. Policy 10.7.2 requires config in `/etc`.
- **[V] The split costs zero code.** `crates/kiosk-launcher/src/main.rs:27-42`
  `resolve_config_dir` already parses `--config <dir>`; `crates/kiosk-launcher/src/spawn.rs:121`
  is literally `cmd.arg("--config").arg(config_dir);` — the launcher propagates it to
  `kiosk-main`. And `resolve_main_exe` (`main.rs:56-62`) resolves the child from
  **`current_exe()`'s dir**, *not* from `config_dir`. So binaries-in-one-place,
  config-in-another is the shape the code already supports. Nothing to implement.

**Divergence statement (C3), both directions.** Stricter than the parent: the operator
files move out of the install dir into `/etc/kiosk`, so `/usr` can be mounted read-only
and the config is where Policy says it is. Looser than the parent: the install directory
is not `/opt/kiosk/`. **What else must change, named:** (a) parent §4 table, Linux cell
of row 1, `/opt/kiosk/` → `/usr/lib/kiosk/`, and row 2's Linux cell "same" →
`/etc/kiosk/ (--config)`; (b) P2-C:85 `ExecStart`. I am asserting a parent-spec erratum,
not silently overriding it — if the Moderator rules the parent's cell binding as written,
the fallback is `/opt/kiosk/` binaries + `/etc/kiosk/` config with a **documented lintian
override** for `dir-or-file-in-opt`, which G's testing section already permits
("documented overrides"). I prefer the first; the second is survivable. Either way the
config/credential/mp4 move to `/etc/kiosk`, and that part is not optional.

**Dependencies.** C's `ExecStart` line; parent §4 erratum; G5, G6, G9 all assume `/etc/kiosk`.

---

## G2 — `.deb` payload

**Proposal.** `/usr/lib/kiosk/{kiosk-main,kiosk-launcher}`,
`/usr/lib/kiosk/bundled/{error,offline,pinpad,safe,splash}.html`,
`/lib/systemd/system/kiosk.service`, `/etc/kiosk/kiosk.ini` (conffile, from
`dist-template/`). **`kioskctl` is withdrawn from the payload.**

**Requirement.** parent §9 P2 row (".deb"); SEC-11 (fleet signing key).

**Evidence. [V]** `ls crates/kiosk-main/bundled/` → exactly the five pages.
`Cargo.toml` members = `kiosk-{core,main,launcher}`; `ls crates/kiosk-core/examples/` →
`kioskctl.rs`. It is a cargo example, not a workspace binary, and its module doc names
`KIOSK_SIGNING_KEY_B64`. Withdrawing it is both the correct fact and the correct posture:
the fleet **private** signing seed's tool has no business on a device that an attacker can
physically remove (parent §8/SEC-08 threat). It stays a CI/ops-host tool. This is a
straight concession to verifier §2.4 with a security reason attached.

---

## G3 — Dependencies

**Proposal.** Unchanged:
`libwebkit2gtk-4.1-0, libgtk-3-0, cage, gstreamer1.0-plugins-{base,good,bad}, gstreamer1.0-libav`.

**Requirement.** parent §3.4 verbatim (the four GStreamer names, lines 285-289 — **[V]** I
read them); C7 platform floor.

**Evidence.** Verifier §5 checked all seven against bookworm; I did not re-run the
package lookups (tier 5, uncontested, and it *supports* my spec). I did independently
**[V]** fetch `cage` 0.1.4-4's `debian/control` from `sources.debian.org` — it exists,
`Architecture: any`, `Build-Depends: libwlroots-dev (>= 0.14.0)`.

**Declared limitation (was undeclared).** `Depends: libgtk-3-0` makes the package
uninstallable on Ubuntu 24.04+ (`libgtk-3-0t64` after the `time_t` transition). C7 pins
the floor at Debian 12 / Ubuntu 22.04, both of which ship `libgtk-3-0`, so this is in
scope but it should be a sentence in the runbook's prerequisites, not a field discovery.
**Residual risk:** a future floor bump needs a `libgtk-3-0 | libgtk-3-0t64` alternation.

---

## G4 — State dir

`/var/lib/kiosk`, `0750`. **[V]** parent §4 row 3; **[V]** `p2a:113` plans
`resolve_data_dir() → /var/lib/kiosk/` (shipped code today is
`crates/kiosk-main/src/main.rs:436` `ProgramData`-shaped, duplicated at
`crates/kiosk-launcher/src/main.rs:47-52` — so this depends on A landing, and I now say
so). **Ownership follows G16**, not asserted independently. That is the fix for
undeclared assumption 7's second half.

---

## G5 — Secrets discipline (the real design)

**Concession first.** "Secrets discipline (F2 verbatim)" was false as a label and the
mechanism under it was mine, not F2's. **[V]** I read F2 §1: *"Ship **placeholder**
`kiosk.ini` and `kiosk-credential.json` (the obviously-fake placeholder from
`dist-template/`)"*. That is not what G said.

**But F2's mechanism is also wrong to port, and I have evidence the verifier's report
implies but does not state as a defect.** **[V]** `cat dist-template/kiosk-credential.json`:
`client_email`, `private_key`, `token_uri` are all **non-empty**. **[V]**
`crates/kiosk-core/src/logging/auth.rs` `ServiceAccount::from_json` only rejects *empty*
fields. So the F2 placeholder at `0600` yields `BootOutcome::Ready`
(`crates/kiosk-main/src/boot.rs:186`) — **an unprovisioned device boots reporting
healthy** and fails invisibly at token exchange. That is the worst of the three options
on Q3, and it is the one F2 ships. Porting it verbatim would be a defect.

**The design.** Ship **nothing** at the credential path. Leave it absent.

| Path state | `credential_is_owner_only` | Boot | Telemetry | Fetch loop |
|---|---|---|---|---|
| absent (this design) | `Err` | `RenderSafe` | `config.error{credential_permissions}` | `break` |
| G's old empty-0600 | `Ok(true)` | `RenderSafe` | **`reason: None`** | keeps polling |
| F2 placeholder 0600 | `Ok(true)` | **`Ready`** | none | keeps polling |

**[V]** I read all three gates: `boot.rs:161-190` (violation → `RenderSafe` with
`reason: Some(CREDENTIAL_PERMISSIONS_REASON)`; parse failure → `reason: None`),
`fetch.rs:100-106` (`config_error` + `break`), and `credential_acl.rs:24-26`
(`is_violation` = `!matches!(check, Ok(true))`). The absent-file path is the loudest and
most specific of the three, it is the shipped behaviour, and it costs **zero postinst
code**. Least mechanism (Q2) and best observability (Q3) coincide.

**What then makes the mode "the default" (F2 §1's actual property)?** Three cheap parts:

1. Package creates `/etc/kiosk/` **`0750 root:root`** (owner per G16). A directory an
   operator cannot `cp` a world-readable secret *through*.
2. Runbook provisioning step is one atomic command with a verify line:
   `install -m 0600 -o root -g root cred.json /etc/kiosk/kiosk-credential.json` then
   `stat -c '%a %U:%G' /etc/kiosk/kiosk-credential.json` → `600 root:root`.
3. postinst, **upgrade path only**: `[ -e /etc/kiosk/kiosk-credential.json ] && chmod 0600`
   — re-assert, never create.

**Undeclared assumption 6, conceded and narrowed.** postinst cannot know the credential
path in general — it is `config_dir.join(&bootstrap.credential)` (**[V]** `main.rs:730`;
template default `credential = kiosk-credential.json`, **[V]** `cat dist-template/kiosk.ini`).
Step 3 therefore acts on the template default only. **Declared assumption + pinning:** the
runbook states "if you change `credential =`, you own that file's mode"; the install-cycle
test (G15) asserts step 3 on the default path only. **Residual risk:** an operator who
relocates the credential loses the upgrade-time re-assert — bounded, because the app's
fail-closed gate still catches a bad mode at the next boot *and* on every fetch poll.

**Also carried forward (F2 §1 obligation the verifier caught me dropping):** the runbook
gets F2's fourth error-handling bullet — *"on a freshly installed device stuck black,
suspect `kiosk.ini` / the credential first — `safe.html` appearing is not a prerequisite
for a config problem."* Conceded, added.

---

## G6 — The offline mp4

**Concessions.** (a) "conffile-adjacent" is not a dpkg concept — withdrawn. (b) A
conffile under `/usr` is `file-in-usr-marked-as-conffile`, **[V] severity: error** (I
fetched the tag), which fails my own lintian gate. (c) **[V]** `ls -la dist-template/` +
`head -c 120 dist-template/kiosk-offline.mp4` → 88 bytes of text,
`OBVIOUSLY FAKE VIDEO PLACEHOLDER`. There is no default asset. The "size vs completeness"
open decision was framed around a file that does not exist — withdrawn.

**Proposal.** The mp4 is **operator-provisioned like the credential**: not in the package,
placed at `/etc/kiosk/kiosk-offline.mp4` by the runbook. This is Policy 10.7.3's second
method and it is what lintian's own `file-in-etc-not-marked-as-conffile` text prescribes
(**[V]** *"Otherwise they should be created by maintainer scripts"* — severity: error for
the shipped-but-unmarked case, so not shipping is the clean branch). Upgrade preservation
is then free: dpkg never touches a path it does not own.

**Observability, because absence is silent today.** **[V]** `main.rs:998-1012`: a missing
mp4 returns **404**, and `offline.html` degrades to the black splash. That is precisely
the parent's named "silent black video" defect class. So:
- postinst prints a warning if `/etc/kiosk/kiosk-offline.mp4` is absent;
- the runbook step ends with `ffprobe`/`file` verify;
- H5's soak cannot start without it, and H8's cold run checks it.

**Cost:** the operator must supply an H.264 baseline mp4. That is already true — the repo
has no shippable one.

---

## G7 — Autostart

**Concession, in full.** **[V]** `/usr/bin/deb-systemd-helper` DESCRIPTION, read on this
host: *"re-implements the enable, disable, is-enabled and reenable commands from
systemctl. The **"enable" action will only be performed once** (when first installing the
package)."* A raw `systemctl enable` in postinst re-enables on every upgrade and reverts
an operator `systemctl disable`. G named the wrong helper.

**Proposal.**
- postinst: `deb-systemd-helper unmask kiosk.service; deb-systemd-helper enable kiosk.service`
- postinst, then: `deb-systemd-invoke start kiosk.service` (fresh) /
  `deb-systemd-invoke restart kiosk.service` (upgrade)
- prerm/postrm: `deb-systemd-invoke stop`; `deb-systemd-helper purge` on purge.

**`try-restart` withdrawn — and `restart` is strictly better, not merely in-contract.**
**[V]** I read `/usr/bin/deb-systemd-invoke`: line 38 documents `start|stop|restart`;
lines 115-146 apply the guard *"If the job is disabled and is not currently running, the
job is not started or restarted"* to **`start` and `restart` only**; line 186's
`exec('systemctl', @ARGV)` is the fall-through that would have run my `try-restart`
**bypassing that guard**. The guard is exactly the semantic I wanted from `try-restart`,
so the documented verb is the correct one.

---

## G8 — Unit values

**Concession 1 — `StartLimitIntervalSec`.** **[V]** Reproduced independently on this host
(systemd 255.4-1ubuntu8.14):

```
$ systemd-analyze verify ut/k-a.service     # StartLimitIntervalSec in [Service]
ut/k-a.service:10: Unknown key name 'StartLimitIntervalSec' in section 'Service', ignoring.
$ systemd-analyze verify ut/k-b.service     # StartLimitIntervalSec in [Unit]
(clean)
```

C's contract shape has no `[Unit]` section at all (**[V]** `p2c:82-89`). G supplies one.

**Concession 2 — `SuccessExitStatus=86`.** **[V]** parent §3.1, lines 170-173, read
directly: *"systemd `Restart=always` with `RestartPreventExitStatus=86` **and
`SuccessExitStatus=86`**"*. Absent from G and from C, and C hands values to G, so it had
no owner. **Position: it is required, and G owns it.** Without it a technician exit shows
`Active: failed` and a deliberate exit is indistinguishable from a crash to the field tech
and to anything reading unit state — a Q3 defect on the one flow a technician uses.
Both directives are needed and they do not conflict: `SuccessExitStatus` reclassifies the
exit, `RestartPreventExitStatus` suppresses the restart that `Restart=always` would
otherwise perform *even on success*.

**Concession 3 — the start-limit open decision is resolved here, not deferred.**

```ini
[Unit]
StartLimitIntervalSec=0

[Service]
Type=simple
ExecStart=cage -- /usr/lib/kiosk/kiosk-launcher --config /etc/kiosk
Restart=always
RestartSec=5
RestartPreventExitStatus=86
SuccessExitStatus=86
RuntimeDirectory=kiosk
```

**[V]** `systemd-analyze verify` accepts every line above (only complaint was the
not-yet-existing `ExecStart` path).

**Why `0` rather than tuned numbers.** G's own stated requirement is "systemd's limits
must be strictly looser so the FSM, not systemd, is always the authority that gives up
first." `0` is that requirement's limit case in one token. And **[V]** the FSM never
hands systemd a decision in the normal path: `crates/kiosk-core/src/watchdog.rs:80`
`WINDOW_S = 600`, `:81` `SAFE_FAIL_LIMIT = 3`, `:159-162` backoff doubles to a **60 s
ceiling and holds there** once escalated — the launcher **does not exit** when it gives
up, it emits `watchdog.safe_mode_failed` and keeps looping at 60 s. systemd's start limit
therefore only ever governs the *launcher process itself* crash-looping, and there the
right answer for a kiosk is "never stop trying": a systemd-enforced permanent stop is a
black screen forever, strictly worse than the parent's own bounded-loud-loop doctrine
(§3.1 arch-14: *"so the blank-screen outage is bounded and visible in GCL rather than an
invisible infinite loop"*). `RestartSec=5` bounds the spin rate. Loudness is the journal
plus the launcher's `startup-degraded.txt` breadcrumb
(**[V]** `crates/kiosk-launcher/src/main.rs:66-73`).

**Residual risk, declared:** a permanently broken install restarts every 5 s indefinitely
instead of stopping. Pinned by the G15 install-cycle test (asserts the unit reaches
`active`) and by H8.

---

## G9 — Versioning / upgrades

Version from the workspace (**[V]** root `Cargo.toml` `version = "0.1.0"`);
`Conflicts`/`Replaces` unused. After G5 + G6, the upgrade-preservation surface collapses
to **one conffile** (`/etc/kiosk/kiosk.ini`) plus two paths the package never owns
(credential, mp4) — dpkg preserves those by not knowing about them. This is a simpler and
more provable claim than G's original "preserve the three operator-owned files", and it
is the verifier's §7 asymmetry observation turned into the design.

---

## G10 — Runbook: VT / console / seat

**New load-bearing evidence the verification record does not contain.** I read the actual
Debian 12 `cage` source (0.1.4-4, `sources.debian.org`):

- **[V] `cage.1.scd`:** `*-s*  Allow VT switching`.
- **[V] `cage.c:196`** help text `" -s\t Allow VT switching\n"`, `cage.c:238`
  `server->allow_vt_switch = true;`.
- **[V] `seat.c:236-246`:** `if (server->allow_vt_switch && sym >= XKB_KEY_XF86Switch_VT_1
  …) { wlr_session_change_vt(...) } else { return false; }` — the **only**
  `wlr_session_change_vt` call in the tree.

So **VT switching is off by default in cage; the runbook's step is "invoke cage without
`-s`"**, and it is directly verifiable. That is a stronger, in-session discharge of §7.2's
"disable VT switching and zap" than the logind settings alone, and it belongs in the unit
(G8's `ExecStart` has no `-s`) as well as the runbook.

**"Dedicated seat with no other TTYs" — promoted from open fork to gate step.** Conceded:
the parent states it as a deployment-gate requirement (**[V]** §7.2 lines 716-721, read
directly) and G demoted it. The demotion conflated two questions. Separating them:

- *No other TTYs* — a gate step, decidable now, independent of the service user:
  `NAutoVTs=0` + `ReserveVT=0` in `logind.conf`, `systemctl mask getty@.service`,
  `systemctl disable getty.target`. Verify: `ls /dev/tty[1-9]* ; systemctl list-units 'getty@*' ; loginctl seat-status seat0`.
- *Which seat / which user* — G16, still a fork, but with its mechanism pinned.

X11's `DontVTSwitch`/`DontZap` (the parent's parenthetical alternative) goes in the
X11-is-demo-only appendix paragraph, one line, for completeness. Conceded from the
verifier's §9 OMITTED row.

---

## G11 — Runbook: display blanking (the headline omission)

**Position: I take it, it is mine, and here is the step.** Conceded that
`consoleblank=0`, `IdleAction=ignore` and sleep-target masking do not blank-proof a
Wayland session — **[V]** on this host `/etc/systemd/logind.conf` ships `#IdleAction=ignore`
(already the default) and kernel v6.1 documents `consoleblank=` as *"Defaults to 0"* and
VT-console-scoped. Three substitutes, none of which produces H3's outcome.

**What actually blanks a cage session — verified in cage's source, not asserted.**

- **[V]** `grep -n -i 'wlr_output_enable|dpms|idle' cage.c seat.c output.c idle_inhibit_v1.c`:
  the only `wlr_output_enable(..., false)` is `output.c:255-268` `output_disable()`, which
  is the **`-m last` multi-monitor** path (it is called from output layout handling, not
  from any timer).
- **[V]** cage creates `wlr_idle` (`cage.c:372`) and `wlr_idle_inhibit_v1`
  (`cage.c:379-386`) and calls `wlr_idle_notify_activity` on input (`seat.c`, 11 sites).
  `wlr_idle` is a **notification** protocol (`org_kde_kwin_idle`) — it tells registered
  clients that N ms of idle elapsed. **cage 0.1.4 contains no idle timeout and never
  disables an output on idle.**

**Therefore the runbook step is:** *no `org_kde_kwin_idle`/`ext-idle-notify` consumer may
be installed or running on the device* — i.e. no `swayidle`, no `xdg-desktop-portal` idle
service, no `gnome-session`/`xfce4-power-manager`. Concretely, as a gate step with a
verify command:

```
# gate: nothing can ask cage to blank
dpkg -l | grep -E 'swayidle|xautolock|xscreensaver|light-locker|gnome-screensaver' && FAIL
systemctl list-units --type=service --state=running | grep -iE 'idle|screensaver|power-manager' && FAIL
```

plus `cage.c`'s inhibitor path documented as the *belt* (B's `systemd-inhibit` child
covers suspend; `zwp_idle_inhibit_v1` would cover the compositor **if** wry exposed an
inhibitor surface — parent §7 calls that "secondary and only if", and it stays secondary).

**Honest statement of what this is.** The parent says "PRIMARY is configuring
cage/wlroots not to blank." cage 0.1.4 has **no blanking to configure** — so the primary
mechanism is discharged by *ensuring nothing else supplies one*, which is a negative
configuration and must be a verified gate step rather than an assumption. Evidence tier 5
(upstream source of the exact Debian 12 package version), mechanically checked, and it is
the only mechanism the source admits.

**Pinning + residual risk.** H3 (24 h no-blank) is the pin, and it now has a producing
step. **Residual:** panel-side power management (the monitor's own OSD sleep timer) is
outside every software layer — that is hardware, explicitly H3's job to observe, and it is
now stated instead of implied. Second residual: if the device class forces a compositor
other than cage 0.1.4, this analysis does not transfer and the step must be re-derived —
recorded, owner = H1.

`consoleblank=0` is **demoted** from a keep-awake mechanism to a one-line belt in the boot
cosmetics bullet ("already the default on 6.1; set it explicitly so an inherited cmdline
cannot un-default it"). `IdleAction=ignore` likewise demoted to "assert the default is
intact", which is what B actually asked G for (**[V]** `p2b:171-172`).

---

## G12 — Runbook: cosmetics, updates, SSH

Unchanged in substance: quiet boot + cursor blink off, journald size caps;
`unattended-upgrades` off (**[V]** `p2f:68-70` states F's expectation of exactly this);
`apt-mark hold libwebkit2gtk-4.1-0` released only through the revalidation loop; SSH
keyed-only if present, absent by default.

**Citation corrected (verifier §9, conceded).** The pinned-image intent is **parent §3.4
(line 289) and §10 (line 874)**, not §9 — **[V]** `grep -n pinned` on the parent returns
289 and 874 for the image phrase; §9's only Linux text is the platform floor. G's "§9"
was wrong.

---

## G13 — Image position

Unchanged: runbook-on-stock-Debian-12 is the P2 image story; the pin is the `dpkg -l`
snapshot + config diffs archived per validated device class; automated image build stays a
recorded ponytail. **[V]** `dpkg 1.22.6` present, snapshot form adequate; I adopt the
verifier's note and specify `dpkg-query -W -f='${binary:Package}\t${Version}\t${db:Status-Abbrev}\n'`
as the machine-diffable form alongside `dpkg -l` for human reading — one line, strictly
better diffs, no new dependency.

---

## G14 — Hardware checklist, revised

| # | Item | Origin (corrected) |
|---|---|---|
| H1 | Real cage boot chain: unit → cage → launcher → main; fullscreen on the physical display; `-m last`/`-m extend` behaviour recorded; **promotes the G16 service-user fork** | A `p2a:339-342` — recorded honestly as a *plan-time* item A expected to settle under weston, escalated to hardware because tao's monitor behaviour is display-dependent |
| H2 | `RestartPreventExitStatus=86` **+ `SuccessExitStatus=86`** end-to-end via systemctl: technician exit stays exited **and** `systemctl status` reads `inactive (dead)`, not `failed` | C `p2c:155-156` (verbatim: "the systemd half of the contract is asserted at P2-G's image validation") |
| H3 | Keep-awake positive: `systemd-inhibit --list` shows the hold; **G11's no-idle-client gate re-verified on the device**; display never blanks over 24 h; panel OSD sleep observed | B `p2b:193-196` (verbatim) + G11 |
| H4 | Touch: corner-tap on real hardware; `GDK_TOUCH_CANCEL`; on-screen keyboard (squeekboard/onboard) exercised and chosen | D `p2d:124-128` (hardware half) + `p2d:162` (keyboard → P2-G). `GDK_TOUCH_CANCEL` is a D *plan-time* item G promotes — stated as a promotion |
| H5 | ≥72 h offline-video soak, RSS trend, loop count; visual black-frame check | E `p2e:99` / RT-05 |
| H6 | Escape-vector sweep under the locked session: the **parent §7.2** vectors (VT chords, zap, sleep) + the parent §7 shortcut/dialog/edge rows, per §10's manual checklist | parent §7.2 + §10 line 879-881. **Corrected:** §10 does not enumerate a hardening list; it points at `docs/testing.md`, which **[V]** does not exist (`docs/testing/` holds only `p1d2-signed-config-smoke.md`). G's enumeration was G's. H6 now cites the two parent sections and the checklist file is created by G |
| H7 | Egress + nav guard against a real network: DNS failure modes, captive-portal interference | **parent §3.3 (captive portals) + §7 SEC-10 row.** *"A/B" was invented — conceded and withdrawn.* The item stays because SEC-10's residual-gap documentation is a P2 obligation and B's smoke runs against a local httpd only |
| H8 | Runbook executed cold on the device class, timed; `dpkg -l` + `dpkg-query` snapshot captured; mp4 and credential provisioning steps verified | G §2-3 |
| **H9** | **Under the promoted service user (G16): credential readable, `/var/lib/kiosk` writable, spool drains, `/run/kiosk` socket reachable** | **A's `ponytail:` `p2a:275-276`** — "add an owner check if a non-root service user lands" — carried forward with an owner for the first time |

H9 is the fix for undeclared assumption 7. **[V]** `p2a:275-276` read directly.

---

## G15 — Testing, with a runner

**Concession.** G asserted two gates with no job (C9). Named now:

- **Install/remove/upgrade cycle** → **F's nightly job (a)**, which **[V]** already runs
  *"in a `debian:12` container"* (`p2f:45-49`). This is a stated addition to F, declared as
  a dependency, not assumed. Asserts: unit enabled and `active`; `/etc/kiosk` `0750`;
  `kiosk.ini` present; **credential and mp4 absent** (the provisioning contract);
  `grep -R` over the built `.deb` finds zero `BEGIN PRIVATE KEY` / no `kioskctl`;
  upgrade preserves an operator-edited `kiosk.ini`, an operator-placed credential and mp4;
  `deb-systemd-helper` does not re-enable a unit the test disabled first.
- **`lintian`** → **F's release job**, which **[V]** currently specifies `dpkg-deb`
  assembly only (`p2f:55-57`). Also a stated addition to F. Gate is
  `lintian --fail-on error`; overrides must be in `debian/source/lintian-overrides` with a
  comment each. After G1/G6 I expect zero error-severity tags, which is the point of both
  changes.
- The runbook stays testable prose; every step ends with a verify command; H8 is the
  integration test.

**[V]** No docker daemon in this environment, so I cannot execute either gate here; both
are declared as pinned-by-CI, not as verified.

---

## G16 — Service user + seat access (fork kept, mechanism pinned)

**Shipped default: root** (the unit carries no `User=`). `/etc/kiosk` and
`/var/lib/kiosk` `root:root`; credential `0600 root:root`. This is exactly parent §4's and
§8/SEC-09's literal Linux wording ("`root:root 0600` or the keyring"), it is the least
mechanism, and it is the only variant that needs no additional package.

**The hardened recipe is fully specified in the runbook, not hand-waved** — this is the
Q5 requirement that "whether the mechanism works at all" be pinned:

- `seatd` **[V]** exists in Debian 12 (`packages.debian.org/bookworm/seatd` → `0.7.0-6`,
  HTTP 200), and **[V]** cage builds against `libwlroots-dev (>= 0.14.0)` (its
  `debian/control`), i.e. the libseat-based wlroots — so a non-logind seat backend is
  available without a login session.
- Recipe: `adduser --system --group kiosk`; `usermod -aG _seatd,video,input,render kiosk`;
  `systemctl enable seatd`; unit gains `User=kiosk`,
  `SupplementaryGroups=video,input,render,_seatd`, `RuntimeDirectoryMode=0750`;
  `chown -R kiosk:kiosk /etc/kiosk /var/lib/kiosk`; credential stays `0600` (A's mode-bits
  check is uid-agnostic — **[V]** `p2a:269-273`, `mode() & 0o077 == 0`).
- **If the fork flips, exactly these change**, named now so H1 is a decision and not a
  redesign: the two `chown`s, `User=`, `SupplementaryGroups=`, `Depends: seatd`, and A's
  `ponytail:` uid check activates (H9).

**Residual risk, declared:** the default runs a WebKitGTK browser as root. Mitigations in
force are the covering ones — cage owns all input with no VT escape (G10), SEC-10 egress
containment (B), signed config with a pinned key (P1), and §8/SEC-08 physical prereqs.
The non-root recipe is the posture upgrade and H1 is where it is promoted or rejected on
evidence. I am not claiming root is better; I am claiming it is the correct *default to
ship* while the mechanism that replaces it is written down and testable.

---

# Response to the verification record

## The seven FALSE — all conceded, four with a substituted mechanism

| # | FALSE | Disposition |
|---|---|---|
| 1 | Install dir contradicts parent §4 | **Concede the undeclared contradiction; revise.** G1: `/usr/lib/kiosk` + `/etc/kiosk` via `--config`, divergence declared in both directions, parent §4 erratum + C:85 named as the two things that must change. I add evidence the record lacked: conforming to `/opt/kiosk/` is itself a lintian **error** (`dir-or-file-in-opt`), so "just conform" is not a clean option, and Policy §9.1.1 exception 1 blesses `/usr/lib/<pkg>/` in words |
| 2 | "Secrets discipline (F2 verbatim)" is not F2 | **Concede the label and the mechanism.** G5: ship nothing at the credential path. I add: F2's actual placeholder would boot `Ready` — worse than either alternative — so F2's mechanism should not be ported either |
| 3 | `StartLimitIntervalSec` in `[Service]` discarded | **Concede; reproduced independently** on systemd 255. G8 adds a `[Unit]` section (C's shape has none) and resolves the open decision to `StartLimitIntervalSec=0` + `RestartSec=5`, with the FSM constants cited (`watchdog.rs:80,159-162`) |
| 4 | `SuccessExitStatus=86` absent | **Concede; adopt.** G8. Parent §3.1 read verbatim; G owns it because C hands values to G. H2 now asserts `inactive (dead)`, not merely "stays exited" |
| 5 | mp4 conffile fails G's own lintian gate | **Concede.** G6: not shipped at all (Policy 10.7.3 method 2), operator-provisioned, postinst warns on absence because **[V]** a missing mp4 is a silent 404 → black splash (`main.rs:1005-1011`) |
| 6 | `systemctl enable` is the wrong helper | **Concede.** G7: `deb-systemd-helper enable` + `deb-systemd-invoke start`/`restart`. `try-restart` withdrawn — and I add that `restart` is *better*, because **[V]** the disabled-and-not-running guard applies only to `start`/`restart` and `try-restart` would have hit the `exec systemctl` fall-through and bypassed it |
| 7 | H7's origin "A/B" does not exist | **Concede the citation, keep the item.** G14: re-attributed to parent §3.3 + §7 SEC-10 |

## The headline omission — DPMS / cage blanking

**Position taken, step given: G11.** Not deferred, not reassigned. PF-07 is a P2 obligation
per frame §2 and G owns the §7.2 runbook. The substantive finding is that cage 0.1.4
**has no idle blanking to disable** — verified in its source, three ways — so the discharging
step is a negative gate ("no `wlr_idle` consumer installed or running") with a runnable
check, plus the panel-OSD residual stated as hardware. `consoleblank=0` and
`IdleAction=ignore` are demoted to belts. H3 now has a producing step.

## Dedicated seat

**Concede the demotion; split the question (G10).** "No other TTYs" becomes a gate step
decidable now, with verify commands. Only the service-user/seat-ownership question stays a
fork, and G16 pins its mechanism so H1 is a promotion, not a redesign.

## The thirteen undeclared assumptions

| # | Assumption | Disposition |
|---|---|---|
| 1 | `/usr/libexec/kiosk/` is the install dir | Concede → G1 (`/usr/lib/kiosk/`, divergence declared) |
| 2 | config/credential/mp4 can live under `/usr` | Concede → G1 (`/etc/kiosk` via `--config`; **[V]** zero code change, `spawn.rs:121`) |
| 3 | a conffile can protect a file outside `/etc` | Concede → G6 (no conffile; one conffile total, in `/etc`) |
| 4 | a default mp4 exists to ship | Concede → G6 (**[V]** 88-byte text placeholder; open decision withdrawn) |
| 5 | `kioskctl` is shippable | Concede → G2 (withdrawn; **[V]** cargo example, fleet signing seed) |
| 6 | postinst knows the credential path | **Declared as assumption + narrowed** → G5 step 3 acts on the template default only; pinned by the G15 install test; residual = relocated credential loses the upgrade re-assert, bounded by the app's fail-closed gate |
| 7 | root-owned `0600`/`0750` are compatible with the service user | Concede → G16 (fork's flip-list named) + **new H9** carrying A's `p2a:275-276` `ponytail:` forward with an owner |
| 8 | `StartLimitIntervalSec` takes effect in `[Service]` | Concede → G8 (reproduced) |
| 9 | `consoleblank=0` / `IdleAction=ignore` are keep-awake controls | Concede → G11 (demoted to belts; real mechanism supplied) |
| 10 | `systemctl enable` is the Debian convention | Concede → G7 |
| 11 | the container test and lintian gate have a runner | Concede → G15 (F nightly job (a) + F release job; both declared as additions to F, dependencies stated) |
| 12 | H7's origin is "A/B" | Concede → G14 |
| 13 | pinned-image intent is in §9 | Concede → G12 (**[V]** §3.4 line 289, §10 line 874) |

## DRIFT items not already covered

- **`kioskctl` in the payload** (§1.6) → withdrawn, G2.
- **`try-restart`** (§6) → withdrawn, G7, with the reason strengthened.
- **`StartLimitBurst` split from its sibling** (§6) → both now in `[Unit]`; burst is moot
  since the interval is `0`, so `StartLimitBurst` is dropped entirely. One fewer knob.
- **H1's origin is a plan-time item, not a hardware deferral** (§8) → conceded, stated as
  an escalation with the reason (monitor behaviour is display-dependent), G14.
- **H6's "§10 hardening list" is G's enumeration; `docs/testing.md` does not exist** (§8)
  → conceded; H6 re-cited to §7.2 + §7 + §10, and G creates
  `docs/testing/linux-hardening-checklist.md` (matching the existing `docs/testing/`
  directory convention, not the parent's stale filename).
- **F2's fourth error-handling bullet dropped** (§1.5) → conceded, added to the runbook
  (G5).
- **`libgtk-3-0` uninstallable on Ubuntu 24.04+** (§5) → adopted as a declared limitation
  with the alternation named as the future fix (G3).
- **`dpkg -l` vs machine-diffable forms** (§10) → adopted alongside (G13).

## UNVERIFIABLE (3)

All three are environment limits, not disputes, and all three now have a named runner:
dependency co-resolution and the install cycle → F nightly `debian:12` (G15); lintian →
F release (G15). I could not execute them here either (**[V]** no docker daemon), and I
declare them as CI-pinned rather than verified.

---

# Withdrawals / restructuring

**Withdrawn outright**

1. `/usr/libexec/kiosk/` as the install dir — Policy has no standing for it (**[V]** zero
   hits for `libexec` in `policy.txt`), and the divergence was undeclared.
2. `kioskctl` from the device payload — factually not a workspace binary, and it carries
   the fleet private signing seed onto a physically-exposed device.
3. The offline mp4 from the payload — no shippable asset exists, and shipping it forced
   the conffile problem.
4. "conffile-adjacent" — not a dpkg concept; there is no such state.
5. The pre-created empty `0600` credential — my invention wearing F2's label, and it
   degrades the unprovisioned-device signal (`credential_permissions` → `reason: None`)
   and removes the fetch-loop `break`.
6. The label "**(F2 verbatim)**" on the secrets bullet — F2 says something else.
7. `systemctl enable` and `deb-systemd-invoke try-restart` — wrong helper, out-of-contract
   verb.
8. `StartLimitBurst` as a value to choose — moot once the interval is `0`.
9. `consoleblank=0` and `IdleAction=ignore` as *keep-awake mechanisms* — retained only as
   belts, with their already-default status stated.
10. Open decision 3 ("whether the mp4 ships in the `.deb`") — resolved by G6, and its
    premise was false.
11. Open decision 2 (`StartLimitBurst` numbers) — resolved by G8.

**Restructured**

- §7.2's "dedicated seat with no other TTYs" **split**: the TTY half is promoted from
  open fork to gate step (decidable now); only the seat-ownership half stays a fork, with
  its mechanism pinned (G16) and its flip-list enumerated.
- Keep-awake **inverted**: from "three settings that don't blank" to "cage has no blanking;
  the step is ensuring nothing else supplies one", with a runnable gate and the panel-OSD
  residual stated (G11).
- Secrets **inverted**: from "pre-create the file so the mode exists" to "ship nothing so
  the loudest existing failure path fires", with the mode-default property moved onto the
  directory + a single atomic provisioning command (G5). Less code, better signal.
- Upgrade preservation **collapsed** from three files to one conffile plus two
  package-unknown paths (G9).
- Testing gates **attached to F's existing jobs** rather than asserted (G15), with both
  additions declared as dependencies on F rather than assumed.

**Dependencies I am declaring, for the record**

On **C**: the `[Service]` shape; C:85's `ExecStart` must take G1's path and `--config`, and
C's shape must gain the `[Unit]` section G8 supplies.
On **F**: nightly job (a) gains the install/remove/upgrade cycle; the release job gains
`lintian --fail-on error`. Neither exists in F today.
On **A**: `resolve_data_dir()` → `/var/lib/kiosk` and the `#[cfg(unix)]`
`credential_is_owner_only` must land, or G5's whole SEC-09 story is the fail-open stub
(**[V]** `crates/kiosk-main/src/credential_acl.rs` non-Windows stub still returns
`Ok(true)`).
On **B/D/E**: the hardware deferrals H3/H4/H5 as cited.
On the **parent**: §4 table erratum (row 1 and row 2, Linux cells).
