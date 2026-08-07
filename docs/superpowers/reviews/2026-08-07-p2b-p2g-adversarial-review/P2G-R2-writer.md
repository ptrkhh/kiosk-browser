# P2-G — WRITER, Round 2

No frame dispute. Banked clean passes (G1, G2, G6, G7, G10, G12, G13, G8 directive
semantics, G14 minus the keyboard row) are not re-argued.

**[V]** = re-verified by me this turn, command + result inline. Two objections carry a
**partial rebuttal of a sub-claim** inside a REVISE; both are marked and both are verified.

---

## OB-1 — REVISE (HIGH)

**Conceded, and my reproduction is worse than the objection.** **[V]** systemd 255,
G8's unit exactly as printed, installed at `en4/usr/lib/systemd/system/kiosk.service`:

```
$ systemctl --root=$PWD/en4 enable kiosk.service
The unit files have no installation config (WantedBy=, RequiredBy=, …)
$ find en4/etc -type l          → (nothing)
$ systemctl --root=$PWD/en4 is-enabled kiosk.service
static
```

`static` is the operative fact: **[V]** `/usr/bin/deb-systemd-invoke:131-134` matches
`is-enabled` output against `/enabled/`; `static` does not match, so `:138` prints
*"$unit is a disabled or a static unit, not starting it"* and skips it. So `enable` creates
nothing **and** `start` refuses. Autostart is a total no-op and arch-05 is undischarged.

**[V]** Adding the section fixes it:
```
$ printf '\n[Install]\nWantedBy=multi-user.target\n' >> …/kiosk.service
$ systemctl --root=$PWD/en4 enable kiosk.service
Created symlink …/etc/systemd/system/multi-user.target.wants/kiosk.service → …
$ systemctl --root=$PWD/en4 is-enabled kiosk.service    → enabled
```

**Ownership, stated explicitly: the `[Install]` section is G's.** C's contract line
(`p2c:80`) hands "values **and installation**" to G; `[Install]` is literally installation.
This is the same argument I used to take `SuccessExitStatus=86`, applied consistently. C's
shape stays the `[Service]` block; G supplies `[Unit]` and `[Install]`.

**Replacement — the unit in full:**

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

[Install]
WantedBy=multi-user.target
```

`multi-user.target`, not `graphical.target`: the runbook installs no display manager.
`RestartSec` is 30, not 5 — see OB-10. G16's flip-list gains `[Install]`-adjacent ordering
(`Wants=`/`After=seatd.service`), which it did not name.

---

## OB-2 — REVISE (HIGH)

**Conceded on both limbs, and I have a mechanically proven scoping split rather than a
guess.**

**[V] What a systemd-less container can and cannot answer.** This host is not
systemd-booted (`ps -p 1 -o comm=` → `process_api`; `[ -d /run/systemd/system ]` → ABSENT):

```
$ systemctl is-enabled ssh.service      → not-found          (answers; filesystem query)
$ systemctl is-active  ssh.service      → System has not been booted with systemd
                                          as init system (PID 1). Can't operate.
```

So **`is-enabled` is assertable with no PID-1 systemd; `is-active` is not.** That is the
line, and it is exactly where the OB-1 regression lives. Split accordingly:

- **F's nightly `debian:12` (unchanged runner) asserts:** install/remove/purge; file modes;
  the zero-`BEGIN PRIVATE KEY` grep; absent-credential and absent-mp4 contracts; upgrade
  preservation; **`systemctl --root=/ is-enabled kiosk.service` → `enabled`** (the OB-1
  guard, proven runnable above); `deb-systemd-helper` state-file/symlink bookkeeping;
  `systemd-analyze verify` on the shipped unit (static, no PID 1 — I have run it in this
  environment all round).
- **"Unit reaches `active`" moves to H2**, which already owns the systemd half of the
  exit-86 contract (`p2c:155-156`). H2's assertion list gains `is-active` → `active` after
  boot. G8's residual risk (`StartLimitIntervalSec=0`) **re-pins to H2 + `systemd-analyze
  verify`**, not to the container. That was a false pin and it is withdrawn.

**Secondary limb — the postinst `die`. [V]** `/usr/bin/deb-systemd-invoke:148`:
`system('systemctl', …, $action, @start_units) == 0 or die("Could not execute systemctl: $!")`.
Confirmed.

**Fix: stop hand-writing maintscripts and use debhelper's canonical autoscripts verbatim.**
**[V]** fetched from `Debian/debhelper` `autoscripts/postinst-systemd-start`:

```sh
if [ "$1" = "configure" ] || …; then
	if [ -z "${DPKG_ROOT:-}" ] && [ -d /run/systemd/system ]; then
		systemctl --system daemon-reload >/dev/null || true
		deb-systemd-invoke start #UNITFILES# >/dev/null || true
	fi
fi
```

The `[ -d /run/systemd/system ]` guard skips the whole block in a container (so no
`policy-rc.d` shim is needed — the shim I would otherwise have had to declare is now
unnecessary), and `|| true` means a failed start never fails the install. **[V]**
`postinst-systemd-restart` supplies the `[ -n "$2" ]` upgrade discriminator
(fresh → `start`, upgrade → `restart`), and `postinst-systemd-enable` supplies
`deb-systemd-helper --quiet was-enabled … else update-state`, which is the once-only
semantic G7 argued for, already written. Adopting the shipped snippets instead of my prose
is the Q2 answer and it closes OB-2's secondary, half of OB-8, and confirms G7 in one move.

---

## OB-3 — REVISE (HIGH)

**I take the deliverable. It is mine — parent §7 Linux cell, D hands it to G twice.** But
the objection's implied remedy ("write the recipe for the chosen package") is not
available, and the reason is verifiable.

**[V] `squeekboard` cannot run under cage 0.1.4.** cage's protocol surface is exhaustively
enumerated by its `*_create` calls in `cage.c` (I listed all of them, lines 338-455):
compositor, data-device-manager, seat, `wlr_idle`, `wlr_idle_inhibit_v1`, xdg-shell,
xdg-decoration, server-decoration, export-dmabuf, screencopy, xdg-output, gamma-control,
xwayland, xcursor. **There is no `wlr_layer_shell_v1_create`, no
`wlr_input_method_manager_v2_create`, no `wlr_virtual_keyboard_manager_v1_create`, no
`wlr_text_input_manager_v3_create`.** **[V]** `grep -n -i
'layer_shell|input_method|virtual_keyboard|text_input'` over `cage.c seat.c output.c view.c
xdg_shell.c` returns **nothing**; **[V]** the source file list has no `layer_shell.c` /
`input_method*.c`. squeekboard needs layer-shell to place itself and input-method-v2 to
type. Neither exists. It would not start, let alone type.

**`onboard` is X11/XTEST.** **[V]** cage does build Xwayland (`cage.c:446`
`wlr_xwayland_create`), so onboard can *run*; but under cage our GTK3/WebKitGTK window is a
native Wayland client, and XTEST events injected into Xwayland do not reach it. It works
only if the app is forced to `GDK_BACKEND=x11` inside cage's Xwayland — which forfeits the
Wayland input path P2-D is built on.

So both of the parent's two named packages are non-viable in the supported session. The
runbook must say that, not pick one.

**Runbook section — `On-screen keyboard` (new, in `lockdown.md`):**

1. **Default for the supported cage session: in-page.** The keyboard is rendered by the
   page, in our own process. Parent §7's keyboard row already sanctions this mechanism by
   name for Windows ("the bundled JS on-screen keyboard"), and the in-repo precedent exists:
   **[V]** `crates/kiosk-main/bundled/pinpad.html` (2858 B) is a grid-of-buttons in-page
   input surface. Deployment is either the site's own web app rendering its input UI, or the
   operator's `inject_js` — which is a **named P2-row deliverable** ("config-driven
   `inject_css`/`inject_js` knobs"), so the mechanism has an owner and G is not inventing a
   code deliverable inside a packaging spec.
2. **`Depends:` — none, stated explicitly** (same treatment as the mp4): no installable OS
   package solves this under cage, and adding one would ship a broken dependency.
3. **Documented fallback, with its cost:** `GDK_BACKEND=x11` inside cage's Xwayland +
   `onboard`. Costs the Wayland input path; recorded in the X11 appendix, not the supported
   recipe.
4. **`squeekboard` is documented as ruled out, with the evidence above**, so a future
   integrator does not re-litigate it.

**Cross-spec bonus, stated because it resolves a hazard rather than creating one.** The
verification record against P2-D notes that a *separate-process* OSK produces no GDK events
in our process, so D's per-process idle `ActivityClock` would keep counting while a
technician types. **The in-page answer is same-process, so it generates real GDK/DOM
activity and does not break D's premise.** Choosing the separate-process route would; that
is now written down where H4 will read it.

**H4 revised:** "on-screen keyboard *decision*" → "verify the in-page keyboard receives
touch input and resets the idle clock on the device class; if the site app has text inputs
and no in-page keyboard, record the `inject_js` payload as a deployment artifact."

---

## OB-4 — REVISE (MED), with a verified partial rebuttal of the second limb

**Limb 1 — conceded, reproduced. [V]**

```
$ chmod 0750 etckiosk; chmod 0644 cred.json; cp cred.json etckiosk/
750 etckiosk
644 etckiosk/cred.json          ← world-readable inside the 0750 dir
$ install -m 0600 cred.json etckiosk/c2.json  → 600
```

"A directory an operator cannot `cp` a world-readable secret through" is **false** and is
withdrawn. Directory mode governs traversal, not the mode of files created inside; `umask`
does.

**Replacement mechanism — the package sets the mode, not a sentence.** postinst installs a
provisioning helper `/usr/lib/kiosk/kiosk-provision-credential` (three lines: `install -m
0600 -o root -g root "$1" /etc/kiosk/kiosk-credential.json`), and the runbook's step is to
run it rather than to remember flags. Plus the upgrade-time `chmod 0600` re-assert already
in G5 step 3. Plus a `dpkg-statoverride`-free, code-free belt: `/etc/kiosk` stays `0750`
for traversal.

**Honest restatement, adopted verbatim into G5:** *the app's fail-closed gate is what
enforces the mode; the package supplies the traversal barrier and a provisioning command
that sets it correctly.* That is weaker than F2's Windows property (`util:PermissionEx`
sets the DACL on a file the MSI ships) and **the asymmetry is now declared under C3**: on
Windows the installer ships the file and sets its ACL; on Linux the installer ships no file
and cannot set a mode on a path that does not exist yet. Consequence, stated: a
mis-provisioned Linux device sits in safe mode rather than never having been
mis-provisioned.

**Limb 2 — partial rebuttal, verified.** The overclaim is conceded: **[V]**
`crates/kiosk-launcher/src/sink.rs:88-90` returns `Err` at the credential gate →
`telemetry: None`; **[V]** `crates/kiosk-main/src/telemetry.rs:202-203` reads that same file
to build the transport. With no credential there is no upload. G5's matrix column is
relabelled **"spooled locally; uploaded retroactively on the first provisioned boot"**.

But *"to the fleet, all three rows of G5's matrix are equally invisible"* is **not correct**,
and the difference is the reason to prefer row 1. **[V]** `crates/kiosk-main/src/main.rs:808-825`:

> // SEC-09 boot gate durability (Critical 1 fix): `telemetry::build` is never called
> // below when `boot_fault_reason` is `credential_permissions` … Write the SAME event
> // directly to the local `Spool` here … this needs no GCL client and no credential

`telemetry::spool_boot_config_error` (**[V]** `telemetry.rs:291-320`) appends the
byte-identical `config.error{credential_permissions}` entry to `<data>/spool/main` — the
same path `build` opens at `:224`, so the next provisioned boot's `Logger` drains it to
GCL. There is even a durability test, **[V]** `telemetry.rs:697`
`spool_boot_config_error_writes_a_durable_config_error_entry`. Rows 2 and 3 of the matrix
(empty-0600, F2-placeholder) trip **no** boot fault, so they spool nothing about the
credential at all. Row 1 is deferred-visible with a named draining mechanism; rows 2 and 3
are permanently invisible. The ranking argument survives on stronger ground than I gave it.

---

## OB-5 — REVISE (MED)

**Conceded; reproduced independently. [V]** two versions with `/etc/kiosk/kiosk.ini` as a
conffile, operator-edited, upgraded with `DEBIAN_FRONTEND=noninteractive … </dev/null`:

```
*** kiosk.ini (Y/I/N/O/D/Z) [default=N] ? dpkg: error processing package cftest (--install):
 end of file on stdin at conffile prompt
Errors were encountered while processing: cftest        rc=1
```

**Fix: ship no conffile at all — apply G5/G6's rule to `kiosk.ini` too.** The Critic is
right that G9 stopped short; `kiosk.ini` is the one file 100% of devices must edit
(**[V]** `dist-template/kiosk.ini` has an empty `device_id =` and `replace-me` in
`site`/`project_id`/`config_url`). The package ships an **example**, the runbook writes the
real file:

- `/usr/share/kiosk/kiosk.ini.example` — not a conffile, never touched by the operator.
- Runbook: `install -m 0640 -o root -g root /usr/share/kiosk/kiosk.ini.example /etc/kiosk/kiosk.ini`, then edit.
- **Zero conffiles in the package.** No prompt, no `--force-confold` to remember, upgrade
  preservation is total by construction rather than by flag.

**[V]** Verified end to end: zero-conffile upgrade, noninteractive, stdin closed → `rc=0`,
`Setting up cftest2 (2) …`, operator file `OPERATOR-WRITTEN-INI` intact at mode `640`.

**And a trap I caught while verifying, which changes the path.** **[V]** this host (and the
`debian:12` container image) ships `/etc/dpkg/dpkg.cfg.d/excludes` containing
`path-exclude=/usr/share/doc/*`. An example placed under `/usr/share/doc/kiosk/` is
**silently dropped on install** while still appearing in `dpkg -L` — I reproduced exactly
that. `/usr/share/kiosk/` is not excluded; **[V]** the same test there installs and the file
is present. Hence `/usr/share/kiosk/kiosk.ini.example`, not the reflexive doc directory.

---

## OB-6 — REVISE (MED)

**Adopted.** `Conflicts:` is durable where my provisioning-time grep is not, it is one
control line, no new dependency, no code — Q2, and it makes the grep list a machine-checked
artifact instead of prose that drifts. **[V]** Debian Policy 4.7.4.1 §7.4, verbatim:
*"When one binary package declares a conflict with another using a `Conflicts` field, `dpkg`
will refuse to allow them to be unpacked on the system at the same time … Otherwise, it will
halt the installation of the new package with an error."*

```
Conflicts: swayidle, xautolock, xscreensaver, light-locker, gnome-screensaver, xfce4-power-manager
```

Scoped to packages that actually consume an idle protocol or drive a screensaver — I am not
listing `xdg-desktop-portal`, which is a dependency of too much to conflict with and which
does not itself blank.

I withdraw *"it is the only mechanism the source admits"*. The correct sentence is: cage
supplies no blanking, so the requirement is discharged by preventing anything else from
supplying one — **continuously, via `Conflicts:`**, with the provisioning grep retained as
the runbook's verify line for the running-process case (`Conflicts:` binds packages, not a
hand-started binary).

**Residual, declared:** `Conflicts:` means `apt install <something that pulls swayidle>` will
propose **removing `kiosk`**, and `apt -y` would proceed. That is loud and is the refusal
signal; it is bounded by the same runbook that already pins `unattended-upgrades` off and
leaves SSH absent by default. Carried by G12's update discipline.

---

## OB-7 — REVISE (MED, cross-spec)

**Stated plainly, as asked.** Under G's own runbook, B's `systemd-inhibit --what=idle:sleep
… --mode=block` child is **inert on both axes**:

- `sleep` — G12 masks `sleep.target suspend.target hibernate.target hybrid-sleep.target`.
  logind cannot suspend a machine whose sleep targets are masked, lock or no lock.
- `idle` — a `what=idle` lock blocks logind's `IdleAction`, and G11 demotes
  `IdleAction=ignore` to "assert the default is intact", i.e. the blocked action is already
  a no-op. And per G11's own finding nothing consumes `wlr_idle`, so the compositor never
  raises the idle hint that would reach logind in the first place.

**Parent §11's precondition is answered, negatively, and G records it.** §11 says "confirm
cage honours idle-inhibit **before** relying on it". The answer: cage's
`wlr_idle_inhibit_v1` (**[V]** `cage.c:379-386`, `idle_inhibit_v1.c`) is a *Wayland client*
protocol whose only effect is `wlr_idle_set_enabled(server->idle, NULL, !inhibited)` — it
gates cage's own idle *notifier*, which nothing consumes, and it is unrelated to logind's
inhibitor locks. **cage does not honour logind idle-inhibit, and there is no idle timeout
for it to inhibit.** G's risk section now carries that sentence and closes the parent §11
row; it was G that produced the evidence and G that failed to close it.

**H3 revised.** The `systemd-inhibit --list` assertion is **removed as a keep-awake proof**
— it proves a lock is held, not that it does anything. H3's keep-awake evidence is the 24 h
observation plus the OB-6 `Conflicts:` gate. The `--list` check is retained only as a
regression check on B's spawn path (it proves B's child ran), relabelled as such.

**B's child: keep, do not remove.** Not because it works today — it does not — but because
it is the only thing that still functions if an operator unmasks a sleep target, and it
costs one `cat`. It is recorded as **defence-in-depth with no current effect**, which is the
Q2-honest label, rather than being presented as the keep-awake mechanism. This is a finding
against P2-B's framing and I am stating it against my own dependency rather than leaving it
for B to discover.

---

## OB-8 — REVISE (MED)

**Conceded — it is just missing, and the fix is the discriminator debhelper already uses.**
Directory creation and `chown` run on **first install only**:

```sh
if [ "$1" = configure ] && [ -z "$2" ]; then
    install -d -m 0750 -o root -g root /etc/kiosk /var/lib/kiosk
fi
# every configure, upgrade included: modes only, never ownership
[ -e /etc/kiosk/kiosk-credential.json ] && chmod 0600 /etc/kiosk/kiosk-credential.json
```

`[ -z "$2" ]` is the same first-install test **[V]** debhelper's `postinst-systemd-restart`
uses inverted (`[ -n "$2" ]` → upgrade). Mode-only on upgrade is uid-agnostic, so G16's
`chown -R kiosk:kiosk` survives every upgrade — which is exactly the property G5 step 3
already had for the credential and G4/G16 failed to state for the two directories. Same
defect class I conceded for `systemctl enable`; same fix shape.

G16's flip-list gains an explicit line: *"the two `chown`s are operator-applied and
postinst never re-asserts ownership after first install."*

---

## OB-9 — REVISE (MED)

**Conceded — the divergence paragraph is missing and C3 requires it.** **[V]** parent §7.2
Windows bullet: *"**Autologon** to a locked, **unprivileged** kiosk local account"*.

Added to G16 verbatim:

> **Windows-parity divergence (C3), looser direction.** Windows ships an *unprivileged*
> kiosk account (§7.2). Linux ships **root** by default. Justification: root is the only
> variant needing no additional package for DRM/seat access, and it is the form parent §4
> and §8 name for the credential (`root:root 0600`). The non-root `seatd` recipe is written
> and H1 promotes it. **This is a weaker posture than Windows and is not the project's
> normal posture** — it is a shipped default pending hardware evidence, not a decision that
> root is acceptable.
>
> Scope note: the non-root variant does **not** close the renderer→credential path —
> `WebKitWebProcess` runs as the same uid either way, so a compromised renderer reads a
> `0600 kiosk:kiosk` credential exactly as it reads `0600 root:root`. The delta is
> everything else root can do (write `/usr`, load modules, `/dev/mem`), which G16's
> mitigation list (cage input ownership, SEC-10 egress, signed config, SEC-08 physical) does
> not address — those are escape mitigations, not privilege mitigations.

I adopt the Critic's scoping note rather than paraphrasing around it; it makes the residual
narrower and more accurate than my R1 text, which implied the mitigations covered privilege.

---

## OB-10 — REVISE (MED), with a verified partial rebuttal

**The loudness claim is conceded and withdrawn.** **[V]**
`crates/kiosk-launcher/src/sink.rs:172-179`: `breadcrumb` is `File::create` + one `writeln!`
+ `sync_all` — truncating, single line, last-writer-wins; and **[V]** its own doc at
`:168-171` says *"Presence therefore means 'the LAST boot was degraded', not 'some boot,
once, was degraded'"*. It is written only if the launcher process starts and reaches
`load_bootstrap`. The failure class `StartLimitIntervalSec=0` exists for — cage cannot get
DRM master, missing `.so`, wrong `ExecStart` — never reaches it. G8 cited it for a case it
cannot cover.

**Partial rebuttal, on the evidence-loss half.** For a *permanently* broken install the Nth
failure is byte-identical to the first, so journald rotation destroys duplicates, not
evidence — `systemctl status kiosk.service` shows the current, i.e. the same, failure. The
loss is real only for a **transient first failure followed by a different persistent one**.
That is the residual, and it is narrower than "the cause is erased".

**Changes made anyway, because the residual is real and the fixes are cheap:**

- `RestartSec=5` → **`RestartSec=30`**. 2,880 starts/day instead of ~17k; still far looser
  than the FSM (**[V]** `watchdog.rs:80` `WINDOW_S = 600`, `:159-162` 60 s backoff ceiling),
  so the FSM remains the authority that gives up first.
- G12's journald cap gains an explicit floor and the arithmetic: `SystemMaxUse` sized to
  retain ≥7 days at the 30 s restart rate, stated as a computed minimum rather than a round
  number, and `Storage=persistent` so the first boot's failure survives a reboot.
- G8's loudness sentence is replaced with: *"the operator-facing signal is
  `systemctl status kiosk.service`; the breadcrumb covers only failures that reach
  `load_bootstrap`, and pre-launcher failures (cage/DRM/library/path) are journal-only."*

`StartLimitIntervalSec=0` itself stands — the Critic explicitly does not contest the
directive semantics, and a systemd-enforced permanent stop is a black screen forever, which
is worse than a bounded loud loop under arch-14's own doctrine.

---

## OB-11 — CONCEDE (LOW)

The claim was false and is withdrawn. **[V]** `crates/kiosk-launcher/src/spawn.rs`: the
`cmd.arg("--config").arg(config_dir)` at `:121` sits inside the `#[cfg(windows)]`
`spawn_main`; the non-Windows `spawn_main` at `:198-210` is
`Err(io::Error::new(io::ErrorKind::Unsupported, "kiosk-launcher spawn_main is Windows-only"))`.
G1's *"binaries-in-one-place, config-in-another is the shape the code already supports.
Nothing to implement"* is true of Windows only.

**Cost, stated:** G1 now carries a second, previously undeclared dependency on C.
G1's dependency line becomes: *"C:85's `ExecStart`, **and C's Linux `spawn_main` must carry
`--config <dir>` through to `kiosk-main`; if it does not, `kiosk-main` resolves its config
dir to `/usr/lib/kiosk` and boots into safe mode."* Fail-closed, so the failure is loud
rather than a silently mis-provisioned device — but it is a real dependency and it was
under-declared. The launcher's *own* `--config` parse (`main.rs:27-42`) and
`resolve_main_exe` from `current_exe()` (`:56-62`) are genuinely platform-free; only the
spawn hand-off is Windows-gated.

---

## OB-12 — REVISE (LOW)

**Adopted.** **[V]** `dpkg-shlibdeps` is present here (`/usr/bin/dpkg-shlibdeps`, version
1.22.6, from `dpkg-dev` per `dpkg -S`), i.e. on any build host that already runs
`dpkg-deb`. Deriving library `Depends:` from the built ELF binaries is per-floor-correct by
construction and makes the `libgtk-3-0`/`libgtk-3-0t64` alternation moot — Q2, existing tool
over a hand-maintained list plus a documented caveat.

Split as the Critic scopes it: `${shlibs:Depends}` via `dpkg-shlibdeps` for the library
side (`libwebkit2gtk-4.1-0`, `libgtk-3-0`, and whatever else the binaries actually link);
**hand-written and unchanged** for `cage` and the four GStreamer plugin packages, which are
runtime packages no ELF scan can see and which are parent §3.4 verbatim. G3's declared
`libgtk-3-0` residual risk is **withdrawn** — it no longer exists.

Requires F's release job to run `dpkg-shlibdeps` before `dpkg-deb`; declared as a
dependency on F alongside the lintian step.

---

## G9 / `kiosk.ini` — the concession that stopped short

Taken, in OB-5: the "operator owns it, the package does not touch it" rule now applies to
all three operator files. The package ships **zero conffiles**. This is the consistent
version of the rule I applied to the credential and the mp4 and failed to apply to the one
file guaranteed to be modified.

---

## Updated register — post-round state

**⚑ = dependencies moved this round.**

| ID | Post-round state | Depends on |
|---|---|---|
| G1 | Unchanged layout; *"nothing to implement"* withdrawn ⚑ | parent §4 erratum; C:85 `ExecStart`; **C's Linux `spawn_main` must carry `--config`** ⚑ |
| G2 | Unchanged (clean pass) | G1 |
| G3 | ⚑ `${shlibs:Depends}` via `dpkg-shlibdeps` for libraries; `cage` + 4 GStreamer hand-written; **t64 residual withdrawn**; **no keyboard `Depends:`, stated** | **F release job runs `dpkg-shlibdeps`** ⚑ |
| G4 | ⚑ Directory create/chown is **first-install-only** (`[ -z "$2" ]`); upgrades assert modes only | G16 |
| G5 | ⚑ Traversal-barrier claim withdrawn; `kiosk-provision-credential` helper sets the mode; C3 asymmetry vs F2 declared; matrix column relabelled "spooled locally, uploaded retroactively" | G1, G16 |
| G6 | Unchanged (clean pass) | G1, E |
| G7 | ⚑ **debhelper autoscripts adopted verbatim** (`postinst-systemd-enable` / `-start` / `-restart`, `postrm-systemd-reload-only`); no `policy-rc.d` shim needed | G8 |
| G8 | ⚑ **`[Install] WantedBy=multi-user.target` added — G owns it**; `After=systemd-user-sessions.service`; `RestartSec` 5→30; loudness sentence replaced; residual **re-pinned from G15 to H2** ⚑ | C's `[Service]` shape only |
| G9 | ⚑ **Zero conffiles.** `kiosk.ini` → `/usr/share/kiosk/kiosk.ini.example` (**not** `/usr/share/doc`, path-excluded); runbook installs it | G1, G5, G6 |
| G10 | Unchanged (clean pass) | D |
| G11 | ⚑ `Conflicts:` on idle daemons replaces the one-shot grep as the enforcement; grep retained as a verify line; *"only mechanism"* withdrawn; **parent §11 idle-inhibit row closed negatively**; B's child relabelled defence-in-depth-with-no-current-effect ⚑ | B (finding stated against it) |
| G12 | ⚑ journald cap gains a computed floor + `Storage=persistent`; `Conflicts:` removal-risk residual recorded | F §4 |
| G13 | Unchanged (clean pass) | G12 |
| G14 | ⚑ **H2 gains `is-active` → `active`** (moved from G15); **H3 loses the `--list` keep-awake claim**; **H4 rewritten** to verify the in-page keyboard + idle-clock interaction | G11, G16, G8 |
| G15 | ⚑ Container scope reduced to what a systemd-less runner can answer, **plus `is-enabled` → `enabled`** (proven runnable); `active` moved to H2 | F nightly (unchanged runner), F release (**+ lintian, + `dpkg-shlibdeps`**) ⚑ |
| G16 | ⚑ C3 Windows-parity divergence paragraph added; flip-list gains `[Install]`/`Wants=seatd.service` ordering **and** "postinst never re-asserts ownership after first install" | A `p2a:275-276`; H1 |
| **NEW** | Runbook section **On-screen keyboard** (OB-3): squeekboard ruled out on evidence, onboard as X11-appendix fallback, in-page default, no `Depends:` | parent §7 keyboard row; RT-16 `inject_js`; D (premise preserved) |
