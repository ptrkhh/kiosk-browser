# P2-G — CRITIC, Round 1

No frame dispute.

Everything below marked **[C]** I checked myself this turn (command + result inline). Where
the Writer's claim survived my check I say so and do not object.

## Objection index

| ID | Change | Objection (one line) | Sev | Evidence tier |
|---|---|---|---|---|
| OB-1 | G8 + G7 | The unit G8 prints in full has **no `[Install]` section**, so `deb-systemd-helper enable` creates no symlink and `deb-systemd-invoke start` refuses to start it — autostart (arch-05) is a no-op as specified | **HIGH** | 3/4 (reproduced on systemd 255) |
| OB-2 | G15 | The named runner cannot run the gate: F's nightly `debian:12` container has no systemd (P2-B says so verbatim), so "unit enabled and `active`" is unassertable — and G8's declared residual risk is pinned to it | **HIGH** | 2 (p2b:193-196, p2f:45-49) |
| OB-3 | G3 + G12 + G14 | Parent §7's Linux keyboard deliverable is **deployment docs** (squeekboard/onboard); D hands it to G twice; after the revision G still has only H4 "exercised and chosen", no runbook section, no `Depends:` line | **HIGH** | 1/2 |
| OB-4 | G5 | "A directory an operator cannot `cp` a world-readable secret through" is false — `cp` into `0750` yields `0644`. F2's "installer makes the mode the default" property is therefore *not* reproduced; and the matrix's telemetry column overclaims (no credential ⇒ no telemetry transport) | MED | 3 (reproduced) |
| OB-5 | G9 | The one surviving conffile is the one file modified on 100% of devices; a template change makes `dpkg -i` **abort** on an unattended device — reproduced: `end of file on stdin at conffile prompt` | MED | 3 (reproduced) |
| OB-6 | G11 | A provisioning-time absence check is not durable and is not "the only mechanism the source admits" — dpkg `Conflicts:` on the idle daemons enforces the same absence continuously, in one control line | MED | 4/5 |
| OB-7 | G11 + B | Under G's own runbook, B's `systemd-inhibit --what=idle:sleep` child is provably inert on both axes; H3's `--list` assertion proves only that a hold exists. Parent §11's "confirm cage honours idle-inhibit **before** relying on it" is now answered *negatively* and not recorded | MED | 1/5 |
| OB-8 | G4 + G16 | Nothing says the postinst ownership/mode assertion on `/etc/kiosk` and `/var/lib/kiosk` is first-install-only; if it runs on every upgrade it reverts G16's non-root recipe — the exact failure class G7 conceded for `systemctl enable` | MED | 2 |
| OB-9 | G16 | C3 requires the divergence stated in both directions: parent §7.2 requires an **unprivileged** account on Windows; G ships root on Linux. The residual risk is declared, the parity divergence is not | MED | 1 |
| OB-10 | G8 + G12 | `StartLimitIntervalSec=0` + `RestartSec=5` is unbounded by construction, and G12 caps journald — the conjunction can evict first-failure evidence; the cited `startup-degraded.txt` breadcrumb truncates and is only written if the launcher starts at all | MED | 3 |
| OB-11 | G1 | "Nothing to implement" cites `spawn.rs:121`, which sits inside `#[cfg(windows)]`; the Linux `spawn_main` (`spawn.rs:199-210`) is `Err(Unsupported)`. The dependency on C's Linux spawn preserving `--config` is real and under-declared | LOW | 3 |
| OB-12 | G3 | Hand-written `Depends:` recreates the `libgtk-3-0`/`t64` problem the Writer then declares as residual risk; `dpkg-shlibdeps` (already on any build host) derives it correctly per floor — Q2 | LOW | 3 |

---

## OB-1 — G8's unit cannot be enabled (vs G8/G7, **HIGH**)

**What breaks.** G8 prints the unit "in full" as `[Unit]` + `[Service]`. There is no
`[Install]` section. G7's autostart chain is `deb-systemd-helper enable` then
`deb-systemd-invoke start`.

**When.** Every fresh install, at postinst, on every device.

**Why it matters.** `enable` on a unit with no `[Install]` creates no symlink; and
`deb-systemd-invoke` refuses to start what `systemctl is-enabled` does not report as
`enabled` — the same guard the Writer relied on in G7. So the kiosk does not start at boot
and does not start at install. This is parent §3.1 / arch-05, the headline reason the `.deb`
exists. C's shape (`p2c:82-89`) has no `[Install]` either, and C explicitly hands unit
*values and installation* to G, so it has no other owner — exactly the argument the Writer
used to take ownership of `SuccessExitStatus=86`.

**Evidence [C].** systemd 255 on this host:

```
$ systemctl --root=/tmp/en3 enable kiosk.service          # G8's unit verbatim
The unit files have no installation config (WantedBy=, RequiredBy=, UpheldBy=,
Also=, or Alias= settings in the [Install] section …) This means they are not
meant to be enabled or disabled using systemctl.
$ find /tmp/en3/etc -type l        →  (nothing)

$ printf '\n[Install]\nWantedBy=multi-user.target\n' >> …/kiosk.service
$ systemctl --root=/tmp/en3 enable kiosk.service
Created symlink …/etc/systemd/system/multi-user.target.wants/kiosk.service → …
```

And `/usr/bin/deb-systemd-invoke:131-141` (read on this host): `is-enabled` must match
`/enabled/` or the script prints *"$unit is a disabled or a static unit, not starting it."*
and skips it.

**What is missing, minimally.** `[Install] WantedBy=multi-user.target` (a cage kiosk on a
box with no display manager), plus the ordering G's own runbook implies but never states —
at minimum `After=systemd-user-sessions.service`, and `After=seatd.service` /
`Wants=seatd.service` if G16's fork flips. The flip-list in G16 does not name `[Install]`
or ordering either.

---

## OB-2 — G15's gate has a named runner that cannot run it (vs G15, **HIGH**)

**What breaks.** G15 attaches the install/remove/upgrade cycle to *"F's nightly job (a),
which already runs in a `debian:12` container"*, and lists among its assertions
**"unit enabled and `active`"**. G8's residual risk (`StartLimitIntervalSec=0` → possible
permanent 5 s spin) is declared *"pinned by the G15 install-cycle test (asserts the unit
reaches `active`)"*.

**When.** The first time the job is written.

**Why it matters.** Frame §6 lists "a gate that cannot run" as HIGH, and C9 makes the gate
part of the change. A GitHub-Actions `container: debian:12` job has no systemd as PID 1;
`systemctl is-active` cannot answer. This is not my inference — **P2-B states it about the
same container**: *"the container has no systemd, so the smoke asserts only the degrade
path … The positive assertion … goes on the deferred hardware checklist"* (`p2b:193-196`).
F §2(a) is that container (`p2f:45-49`). The Writer's own G8 argument therefore rests on a
pin that does not exist, and the `active` assertion is the only pre-hardware check that
anything in the systemd+cage chain works at all — F §1's per-PR smoke is ubuntu-22.04 with
`weston`, and C/F drive cage only under `WLR_BACKENDS=headless`. Net: nothing before H1
exercises the unit.

**Secondary, same root.** `deb-systemd-invoke start` in postinst calls
`system('systemctl', …, 'start', …) == 0 or die(…)` (`/usr/bin/deb-systemd-invoke:145`,
read on this host). In a container with no systemd and no `policy-rc.d` returning 101, that
`die` propagates a nonzero postinst and **`dpkg -i` fails**, so even the install half of the
cycle needs a declared `policy-rc.d` shim. G declares neither.

**What survives in that container** (so this is a scoping fix, not a deletion): dpkg
install/remove/purge, file modes, the zero-`BEGIN PRIVATE KEY` grep, the absent-credential/
absent-mp4 contract, conffile preservation, and `deb-systemd-helper`'s state-file
behaviour (symlink bookkeeping, no running systemd required). "Unit reaches `active`" has to
move to H1/H2 or to a systemd-capable runner, and G8's residual risk needs a different pin.

---

## OB-3 — The Linux on-screen-keyboard deliverable is still unowned (vs G3/G12/G14, **HIGH**)

**What breaks.** Parent §7 keyboard row, Linux cell, verbatim (**[C]**, line 697 of the
parent): *"Linux: squeekboard/onboard deployment docs"*. §7's preamble puts the Linux column
at P2. D hands it to G twice (`p2d` "Out: … on-screen keyboard deployment (parent §7 table
— P2-G)"; and again in Scope/defer). The coverage matrix rows H-q / §2.7 record it as
PARTIAL.

**When.** At first touch-hardware deployment; and immediately, as a coverage gap.

**Why it matters.** Frame §2: *"A P2 row item that no spec in A–G owns is a HIGH defect
against whichever spec is its natural owner."* G is the named owner. After the revision, G
still contains exactly one mention: G14's H4, *"on-screen keyboard (squeekboard/onboard)
exercised and chosen"*. That is the **decision**, not the deliverable. **[C]** the runbook
component list (G12, "unchanged in substance") has no keyboard section, and G3 is explicitly
**"Dependencies unchanged"** — `grep -n "squeekboard\|onboard\|keyboard"` over the G spec
returns one line, the H4 row. A device with a text input and no IME is a dead kiosk, and the
gap survives H4: choosing squeekboard on hardware does not produce the recipe or the
`Depends:` entry that makes it install.

**Falsifier.** A runbook subsection with the chosen package's install/enable/verify steps,
plus either a `Depends:`/`Recommends:` line or an explicit statement that it is
operator-provisioned like the mp4. One paragraph; it just has to exist.

---

## OB-4 — G5's directory limb does not do what it claims (vs G5, MED)

**What breaks.** G5 part 1: *"Package creates `/etc/kiosk/` `0750 root:root`. A directory an
operator cannot `cp` a world-readable secret **through**."* This is the limb that is supposed
to carry F2 §1's actual property ("the installer is what makes that mode the default").

**When.** The realistic field action — an operator with a credential in `~` running `cp`.

**Why it matters.** Directory mode does not constrain the mode of files created inside it;
`umask` does. **[C]**:

```
$ chmod 0750 etckiosk ; chmod 0644 cred.json ; cp cred.json etckiosk/
750 etckiosk
644 etckiosk/cred.json          ← world-readable file, inside the 0750 dir
$ install -m 0600 cred.json etckiosk/c2.json
600 etckiosk/c2.json
```

So the only thing that makes `0600` the default is part 2 — a runbook sentence. That is
weaker than F2's Windows property (the MSI *sets* the DACL) and the asymmetry is undeclared
(C3). It is not fail-open — I checked the gate myself (`boot.rs:165` → `is_violation` →
`RenderSafe{reason: Some(CREDENTIAL_PERMISSIONS_REASON)}`; `fetch.rs:100-106` →
`config_error` + `break`), so C5 is intact and a `0644` credential is caught. The defect is
that G5 claims a mechanism it does not have, and the honest version ("the app's fail-closed
gate is what enforces the mode; the package only supplies the traversal barrier") is
materially different — it means a mis-provisioned device sits in safe mode instead of never
having been mis-provisioned.

**Second limb — the telemetry column overclaims.** G5's matrix reports the absent-file row
as *Telemetry: `config.error{credential_permissions}`*. **[C]** with no credential there is
no telemetry transport: `crates/kiosk-launcher/src/sink.rs:88-93` returns `Err` at the same
gate and `LauncherSink` runs with `telemetry: None`; `crates/kiosk-main/src/telemetry.rs:203`
needs `ServiceAccount::from_json` of that same file. The event is spooled and never
uploaded. So "best observability (Q3)" is true only *on the device*; to the fleet, all three
rows of G5's matrix are equally invisible. The ranking still favours G5's choice — the
on-screen `safe.html` plus the breadcrumb is the loudest of the three — but the argument as
written asserts fleet visibility it does not have, and the runbook bullet imported from F2
is doing the real work.

---

## OB-5 — The last conffile aborts unattended upgrades (vs G9, MED)

**What breaks.** G9 collapses upgrade preservation to *"exactly one conffile
(`/etc/kiosk/kiosk.ini`)"*. F §4 defines the update path as *"install the new `.deb`
(`dpkg -i` / operator tooling)"*.

**When.** Any release whose shipped `kiosk.ini` template differs from the previous one — on
every device, because `kiosk.ini` is the one file every device *must* edit (`device_id`,
`project_id`, `config_url`, `credential`).

**Why it matters.** **[C]** built two versions of a package with `/etc/kiosk/kiosk.ini` as a
conffile, modified it, upgraded with `DEBIAN_FRONTEND=noninteractive` and `</dev/null`:

```
 ==> Modified (by you or by a script) since installation.
 ==> Package distributor has shipped an updated version.
*** kiosk.ini (Y/I/N/O/D/Z) [default=N] ? dpkg: error processing package cftest (--install):
 end of file on stdin at conffile prompt
Errors were encountered while processing: cftest
```

The upgrade **fails**, not silently but hard, and leaves dpkg with a half-configured
package. The Writer moved the mp4 out of the package partly to escape dpkg's conffile prompt
on a blob (G6, correctly) and then left the prompt on the file that is guaranteed modified.
The failure is loud, so this is MED not HIGH, and the fix is one declared line — the runbook/
update step must pin `--force-confold` (or `Dpkg::Options::=--force-confdef`), or G must not
ship a `kiosk.ini` at all and let the runbook write it, which is G5's and G6's own logic
applied consistently.

**Note:** G15's asserted "upgrade preserves an operator-edited `kiosk.ini`" is the test that
would have caught this — and per OB-2 it currently has no runner.

---

## OB-6 — The blanking gate is an absence check with no durable enforcement (vs G11, MED)

**Verified first, so this is not a factual dispute.** I fetched Debian 12's cage 0.1.4-4
sources independently. **[C]** `cage.c:196` `" -s\t Allow VT switching\n"`, `:238`
`server->allow_vt_switch = true;`; `:372` `wlr_idle_create`, `:379`
`wlr_idle_inhibit_v1_create`. **[C]** `seat.c:236-241` is the only `wlr_session_change_vt`
call and it is behind `allow_vt_switch`; `wlr_idle_notify_activity` appears at 8 sites.
**[C]** `output.c:255-265` `output_disable()` → `wlr_output_enable(…, false)`, called only
from `:481` (layout handling). **[C]** `grep -i 'timeout\|timer'` over `cage.c`/`seat.c`/
`output.c` returns nothing. The Writer's finding is correct: cage 0.1.4 has no idle timeout
and no idle blanking. I concede that entirely and it defeats any objection of the form "name
the cage setting".

**What I still object to.** G11 says *"it is the only mechanism the source admits"* and
discharges parent §7's **PRIMARY** with a provisioning-time `dpkg -l | grep` + `systemctl
list-units | grep`. Two problems:

1. **Not durable.** The hazard is a *package appearing later*. `apt-mark hold` covers only
   `libwebkit2gtk-4.1-0`; `unattended-upgrades` off stops automatic drift but not an
   operator's `apt install` of anything that pulls `xdg-desktop-portal`, a power manager, or
   `swayidle` as a dependency. Nothing re-checks. H3 is a 24 h window at validation time,
   before the hazard exists. Q3: the resulting failure is a screen that blanks in the field
   with no event — the parent's named silent class.
2. **Not the only mechanism.** dpkg dependency metadata enforces the same absence
   continuously: `Conflicts: swayidle, xautolock, xscreensaver, light-locker,
   gnome-screensaver` in the package's control file makes apt refuse the install rather than
   discovering it at the next H3. One control line, no new dependency, no code — Q2. It also
   turns G11's grep list into a machine-checked artifact instead of runbook prose that can
   drift from the gate.

**Severity MED, not HIGH,** because the negative gate does discharge the requirement *at
provisioning time* and the analysis behind it is sound and verified.

---

## OB-7 — B's `systemd-inhibit` child is inert under G's own runbook (vs G11, MED, cross-spec)

**What breaks.** G11 keeps B's `systemd-inhibit --what=idle:sleep … --mode=block cat` as
"the belt" and keeps H3's *"`systemd-inhibit --list` shows the hold"* as the positive
keep-awake assertion.

**When.** Always, on every conforming device.

**Why it matters.** Take G's runbook as written and both halves of that inhibitor lock have
nothing to inhibit:

- `sleep` — G12/§7.2 masks `sleep.target suspend.target hibernate.target hybrid-sleep.target`
  (**[C]** all four exist and are `static`). logind cannot suspend a machine whose sleep
  targets are masked, inhibitor or not.
- `idle` — a `what=idle` lock blocks logind's `IdleAction`. G11 demotes `IdleAction=ignore`
  to *"assert the default is intact"*, i.e. the action being blocked is already `ignore`.
  And per G11's own finding, nothing consumes `wlr_idle`, so logind's idle hint is not even
  reached from the compositor side.

So H3's `--list` assertion proves that a lock *is held*, not that it does anything — it is
not a keep-awake proof, and the only real keep-awake evidence in H3 is the 24 h observation.
Two consequences G should state and does not: (a) B spawns a permanent `cat` child for a
lock with no effect (Q2 — a mechanism with no effect should be recorded as such, and this is
G's spec because B explicitly *"hands the suspenders to G"*); (b) parent §11's precondition
*"confirm cage honours idle-inhibit **before** relying on it"* is now **answered — negatively**
(cage's `wlr_idle_inhibit_v1` is a Wayland-protocol surface for clients, unrelated to
logind's inhibitor, and there is no idle timeout for it to inhibit). G11 supplies the
evidence that closes that parent risk row and does not close it.

---

## OB-8 — postinst ownership assertion has no first-install guard (vs G4/G16, MED)

**What breaks.** G4: *"`/var/lib/kiosk` `0750`, **ownership follows G16**"*; G5 step 1:
*"Package creates `/etc/kiosk/` `0750 root:root` (owner per G16)"*. G16's flip-list names
*"the two `chown`s"* as things that change when H1 promotes the non-root recipe.

**When.** The first upgrade after an operator has followed G16's `seatd` recipe.

**Why it matters.** G16's non-root recipe is a **runbook** procedure (`adduser --system`,
`chown -R kiosk:kiosk /etc/kiosk /var/lib/kiosk`), executed by the operator on a package
whose shipped default is root. If the postinst's `install -d -o root -g root` / `chown` runs
on every configure — which is what "postinst creates … `root:root`" says, and what the
default `dh_installdirs`-shaped postinst does — the next upgrade reverts the operator's
ownership and the service loses read access to its own credential and write access to
`/var/lib/kiosk`. That is precisely the class of defect the Writer conceded for
`systemctl enable` in G7 ("silently reverting an operator's deliberate `systemctl disable`").
G5 step 3 got this right for the credential (`chmod`, mode-only, uid-agnostic, upgrade-only);
G4/G16 do not state the equivalent rule for the two directories.

**Falsifier.** One sentence: the directory create/chown is `$1 = configure` **first install
only** (`[ -z "$2" ]`), and the upgrade path asserts modes only. Cheap; it is just missing.

---

## OB-9 — Root-by-default is not stated as a Windows-parity divergence (vs G16, MED)

**What breaks.** C3: *"Divergence from Windows is permitted but must be stated in both
directions (stricter and looser) with justification. Silent divergence is a defect."*

**Why it matters.** **[C]** parent §7.2, Windows bullet, verbatim: *"**Autologon** to a
locked, **unprivileged** kiosk local account"*. That is the shipped Windows posture. G16
ships **root** on Linux and states a residual risk with mitigations, but never names the
divergence against the Windows requirement — a reader of G16 alone would conclude root is
the project's normal posture. It is the looser direction, which C3 specifically calls out.

**Where I do *not* object, so the record is clear.** (a) `root:root 0600` is the parent's own
sanctioned Linux form (§4 line 418-420, §8 line 740, both **[C]**), so G16 does not violate
SEC-09 as written. (b) The non-root variant does not close the renderer→credential path
either — `WebKitWebProcess` runs as the same uid, so a compromised renderer reads a
`0600 kiosk:kiosk` credential exactly as it reads a `0600 root:root` one. The real delta is
everything *else* root can do (rewrite `/usr`, load modules, `/dev/mem`), and G16's
mitigation list (cage input ownership, SEC-10 egress, signed config, SEC-08 physical) is
about escape, not privilege. (c) "H1 promotes one" **is** a legitimate deferral: frame §4.5
names "a hardware-checklist row" as an acceptable owner, and G16 pins the mechanism
(seatd 0.7.0-6 in bookworm, wlroots ≥0.14 libseat) and enumerates the flip-list, which
satisfies Q5. So this is a one-paragraph fix, not a redesign.

---

## OB-10 — Unbounded restart plus capped journald can erase the cause (vs G8/G12, MED)

**Semantics conceded first. [C]** `systemd.unit(5)`, current upstream: *"Defaults to
`DefaultStartLimitIntervalSec=` … and **may be set to 0 to disable any kind of rate
limiting**."* And **[C]** on this host, `systemd-analyze verify` accepts G8's unit exactly
as printed, exit 0, with `SuccessExitStatus=86` and `RestartPreventExitStatus=86` together;
moving `StartLimitIntervalSec` into `[Service]` reproduces *"Unknown key name … ignoring"*.
The directive semantics, the placement and the two exit-86 directives are all correct and I
do not contest them.

**What I object to** is the loudness argument attached to the residual risk. G8 says the
permanently-broken-install case is loud via *"the journal plus the launcher's
`startup-degraded.txt` breadcrumb"*. Both are conditional:

- **[C]** `crates/kiosk-launcher/src/sink.rs:172-178`: `breadcrumb` is `File::create` +
  one `writeln!` — it **truncates**, single line, last-writer-wins. It is only written if
  the launcher process starts and gets as far as `load_bootstrap`. The failure mode the
  start limit exists for — cage fails to obtain DRM master, a missing shared library, the
  `ExecStart` path wrong — never reaches it.
- G12 caps journald size. 5 s restarts is ~17k unit starts/day; with a `SystemMaxUse` cap
  the first failure's messages are rotated out well before anyone looks.

Result: the one case `StartLimitIntervalSec=0` deliberately keeps alive is also the case
with no surviving evidence — the "invisible infinite loop" arch-14 names, which G8 cites in
its own support. `RestartSec=5` bounds the rate, not the retention. A `RestartSec` that
escalates (or a `StartLimitIntervalSec`/`Burst` pair that stops the unit only after the
journal has captured the cause) would preserve the Writer's requirement — FSM decides first —
while keeping the evidence. The choice is defensible; the loudness claim as stated is not.

---

## OB-11 — "Nothing to implement" cites a `cfg(windows)` line (vs G1, LOW)

`spawn.rs:121` `cmd.arg("--config").arg(config_dir);` is real, but **[C]** it is inside
`#[cfg(windows)] pub fn spawn_main` (`spawn.rs:66-121`); the non-Windows `spawn_main`
(`spawn.rs:199-210`) is `Err(io::ErrorKind::Unsupported)`. So on Linux today nothing
propagates `--config`, and G1's *"binaries-in-one-place, config-in-another is the shape the
code already supports. Nothing to implement"* is true of Windows, not of the target. The
practical risk is small — **[C]** C's `spawn.rs` section (`p2c:110-118`) says the Linux body
keeps *"the unchanged `--safe` argument chain"*, which reasonably covers `--config` — but G1
declares its dependency on C as only *"C:85's `ExecStart`"*. The dependency list should say
"and C's Linux `spawn_main` must carry `--config` through, or `kiosk-main` resolves its
config dir to `/usr/lib/kiosk` and boots into safe mode". Fail-closed, so LOW, not MED.

---

## OB-12 — Hand-written `Depends:` recreates a problem `dpkg-shlibdeps` solves (vs G3, LOW)

G3 keeps the hand-written seven and then declares the `libgtk-3-0` / `libgtk-3-0t64` residual
with a manual alternation as the future fix. `dpkg-shlibdeps` — shipped with `dpkg-dev`,
present on any build host, **[C]** `dpkg 1.22.6` here — derives library `Depends:` from the
built ELF binaries against the build host's own library packages, which is exactly right per
floor and makes the t64 alternation moot. Q2: existing tool over a hand-maintained list plus
a documented caveat. Keep the four GStreamer names hand-written — they are runtime plugin
packages `shlibdeps` cannot see, and they are parent §3.4 verbatim. `cage` likewise.

---

## Clean passes

**G1 — the layout split and the "erratum" disposition. Clean pass.** I checked the two
things that could have sunk it and both hold.

- **The lintian claim is true. [C]** fetched `lintian.debian.org/tags/dir-or-file-in-opt`:
  *"Debian packages should not install into `/opt`, because it is reserved for add-on
  software."* — `<code class="error">`, **Severity: error**. So the parent's literal cell is
  not costlessly satisfiable, and "just conform" is not the free option it looks like.
- **The disposition is legitimate, not fiat.** The Writer names the exact cells to amend,
  states the divergence in both directions per C3, and offers a survivable fallback
  (`/opt/kiosk/` + documented lintian override) explicitly conditioned on the Moderator
  ruling the cell binding. That is escalation, not override. Frame tier 1 is not breached by
  a spec that says "here is why I think this cell is wrong, here is what I do if you
  disagree." I would object if he had simply written `/usr/lib/kiosk` and moved on — that
  was the R0 defect and it is fixed.
- **No config-dir consumer is left under `/usr`. [C]** I traced every one:
  `main.rs:645` `resolve_config_dir(args.config)` feeds `:655` `kiosk.ini`, `:730`
  `config_dir.join(&bootstrap.credential)`, `:999` `kiosk-offline.mp4` — all three follow
  `--config`. `resolve_data_dir` (`main.rs:436`, launcher `:47-52`) is independent, never
  operator-overridden, and A moves it to `/var/lib/kiosk`, so spool / last-good / cache are
  unaffected. The launcher parses its own `--config` (`main.rs:27-42`) and resolves
  `kiosk-main` from `current_exe()` (`:56-62`), not from `config_dir`, so binaries-here/
  config-there is genuinely the shape the code wants. **No credential path resolves under
  `/usr`** once the unit passes `--config /etc/kiosk`. (The one caveat is OB-11.)
- **[C]** `grep -n "opt/kiosk\|libexec"` over P2-A returns nothing, so the amendment
  contradicts no reviewed spec — only the parent cell and C:85, both named.

**G2 — `kioskctl` withdrawn. Clean pass, and the security reason is right.** **[C]** it is
`crates/kiosk-core/examples/kioskctl.rs`, a cargo example, not a workspace member. Nothing
depends on a *shipped* copy: **[C]** `p2a:290` cites it as *"the P1 `kioskctl` signing
harness"* for smoke fixtures, and `docs/testing/p1d2-signed-config-smoke.md:17,181,206`
invokes it as `cargo run -p kiosk-core --example kioskctl` — repo/CI-side, never from the
device. Withdrawing it from the payload breaks no A/F workflow and keeps the fleet private
signing seed's tool off a physically-removable device (§8/SEC-08).

**G6 — mp4 not shipped. Clean pass, and this is NOT an over-correction.** Four reasons, all
checked: (i) **[C]** `dist-template/kiosk-offline.mp4` is 88 bytes of ASCII — there is no
asset to ship; (ii) under G1 the app reads `/etc/kiosk/kiosk-offline.mp4`, so any shipped
default would land in `/etc` and force either `file-in-etc-not-marked-as-conffile` or a
binary blob as a conffile — the same error-severity trap in a new place; (iii) **[C]** E's
soak (`p2e:96-103`) and F's per-PR subset (`p2f:29-38`) supply their own fixtures and never
consume a package-provided mp4, so there is no cross-spec break; (iv) absence is caught —
**[C]** `main.rs:998-1012` 404s and `offline.html`'s handlers degrade, which E's `media.error`
bridge turns into a spooled event, plus H5 and H8 gate it on hardware. The postinst warning
is nearly worthless on an unattended install (nobody reads dpkg output), but it costs one
line and does no harm.

**G7 — `deb-systemd-helper` + `deb-systemd-invoke`. Clean pass on the helper choice.**
**[C]** re-read `/usr/bin/deb-systemd-invoke` on this host: lines 115-146 apply the
disabled-and-not-running guard to `start`/`restart` only; line 186 `exec('systemctl',
@ARGV)` is the fall-through `try-restart` would have taken. The Writer's strengthened
reason is correct. (The chain is still inert until OB-1 is fixed — that is OB-1's finding,
not a second objection here.)

**G8 — directive semantics. Clean pass** (see OB-10 for what I do contest). `[Unit]`
placement, `StartLimitIntervalSec=0` meaning, and `SuccessExitStatus=86` +
`RestartPreventExitStatus=86` coexisting were all reproduced by me on systemd 255.

**G10 — cage without `-s`, TTY half promoted to a gate step. Clean pass.** Cage source
citations verified above; VT switching is off by default and `-s` is the only path to
`wlr_session_change_vt`. Promoting "no other TTYs" from open fork to gate step is the right
reading of parent §7.2 lines 716-721, which I read directly. The X11 `DontVTSwitch`/`DontZap`
one-liner closes the verifier's OMITTED row.

**G12 — cosmetics / updates / SSH, and the §3.4+§10 citation fix. Clean pass** (the journald
cap interaction is OB-10, filed against G8).

**G13 — image position + `dpkg-query -W -f=…`. Clean pass.** One line, strictly better
diffs, no new dependency.

**G14 — H-row re-attribution and H9. Clean pass except OB-3.** H9 is coherent and carries
`p2a:275-276` forward with a named owner for the first time — that closes undeclared
assumption 7 properly. H2's upgrade to `inactive (dead)` and H7's re-attribution to parent
§3.3 + §7 SEC-10 are correct. H6's `docs/testing/linux-hardening-checklist.md` matches the
existing directory convention. No A–E deferral is left without a row.

**Abandoning `/opt/kiosk/`: not an over-correction.** Verified error-severity tag, blessed
Policy §9.1.1 alternative, escalated rather than decided, and a stated fallback. If anything
the Writer under-sells it: `/etc/kiosk` is also what makes a read-only `/usr` possible,
which is a hardening win the spec mentions once and never claims.

**Shipping the mp4: not an over-correction either** — see G6 above. The concession that
*did* go too far is none of the ones the Moderator flagged; it is G9's, where the same
"operator owns it, the package does not touch it" logic was applied to the credential and
the mp4 and then **not** applied to `kiosk.ini`, leaving the one guaranteed-modified file
under dpkg's interactive conffile machinery (OB-5).

---

## Counts

| Severity | Count | IDs |
|---|---|---|
| **HIGH** | 3 | OB-1, OB-2, OB-3 |
| **MED** | 7 | OB-4, OB-5, OB-6, OB-7, OB-8, OB-9, OB-10 |
| **LOW** | 2 | OB-11, OB-12 |

**Clean passes (8 changes + 1 sub-claim):** G1, G2, G6, G7, G8 (directive semantics), G10,
G12, G13, G14 (all rows except the keyboard deliverable).

No fast-track veto. OB-11 and OB-12 may be bundled.
