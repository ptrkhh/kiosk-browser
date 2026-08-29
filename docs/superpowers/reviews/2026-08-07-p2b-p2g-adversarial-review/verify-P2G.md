# VERIFIER report — P2-G (Linux packaging / OS image / hardware validation)

Role: verification only. No arguments, no proposals. Every entry is a mechanically
checkable claim from `docs/superpowers/specs/2026-08-06-p2g-linux-packaging-image-design.md`
with the check result and its evidence.

Environment note (affects UNVERIFIABLE calls): this host is **Ubuntu 24.04.4 (noble)**,
`systemd 255`, `dpkg 1.22.6`, no man pages installed, `lintian` not installed, `docker`
client present but **no daemon** (`/var/run/docker.sock` absent). Network egress works
(HTTPS 200 to `sources.debian.org` / `packages.debian.org` / raw.githubusercontent.com).

---

## §1 — P1-F2 precedent claims

Source: `docs/superpowers/specs/2026-08-02-p1f2-packaging-deployment-design.md` (read in full).

### 1.1 "installer owns app-scoped setup, a lockdown *runbook* owns OS hardening, secrets never ship in the package" — **VERIFIED**

F2 §1 (payload/ACL/data-dir/bootstrap/signing are all app-scoped); F2 §3 header: "**§7.2
Windows OS-lockdown runbook — `packaging/windows/lockdown.md`** … The app cannot enforce OS
boundaries; this doc is a provisioning checklist." F2 scope line 122-124: "the code-signing
**certificate** is operator/CI-supplied — F2 ships the signing steps and the
placeholder-only `kiosk-credential.json`, never a real secret."

### 1.2 "the MSI makes the DACL the default (F2 §1)" — **VERIFIED (verbatim)**

F2 §1, Credential ACL bullet: "The app already fails closed at boot/reload if the credential
lacks its restrictive mode (§8) — **the MSI is what makes that mode the default.**"

### 1.3 "`lockdown.md` precedent" — **VERIFIED**

F2 §3 names `packaging/windows/lockdown.md`; the file exists on disk:
`/home/user/kiosk-browser/packaging/windows/lockdown.md`.

### 1.4 "**Secrets discipline (F2 verbatim):** the package ships a `kiosk.ini` *template* and **no credential**" — **FALSE**

F2 §1, payload bullet, verbatim:

> Ship **placeholder** `kiosk.ini` and `kiosk-credential.json` (the obviously-fake
> placeholder from `dist-template/`) for the operator to replace per device — the MSI must
> NOT ship real secrets.

F2's discipline is **placeholder credential shipped**, not **no credential**. Both
placeholders exist on disk and both are shipped by the MSI per F2.
G's "no credential + postinst pre-creates an empty `0600` file" is **G's own invention**,
labelled "F2 verbatim". The behavioural consequence is in §4 below.

### 1.5 "Mirrors F2's section shape" (error-handling) — **VERIFIED structurally; one F2 item dropped**

F2's "Error handling / edge cases" covers: bootstrap-offline, upgrade idempotence +
never-overwrite operator files, uninstall-leaves-data-dir, and the inherited safe-mode/
config-fault item. G's section covers analogous ground (service-already-running upgrade,
dependency-solver vs hold, wrong-mode credential, disk-full).

**Dropped:** F2's fourth bullet is an explicit *runbook obligation*:

> F2 owns the fix … and the **runbook must state it**: on a freshly installed device stuck
> black, suspect `kiosk.ini` / the credential first — `safe.html` appearing is *not* a
> prerequisite for a config problem.

G's `lockdown.md` outline (spec §2) contains no equivalent bullet, and G's §4 SEC-09 change
(pre-created empty credential → a *generic* parse error, see §4) makes the same
freshly-installed-device diagnosis harder, not easier.

### 1.6 "Mirrors F2's MSI scope disciplines **exactly**" but adds `kioskctl` to the device payload — **DRIFT**

F2's payload list: `kiosk-main.exe`, `kiosk-launcher.exe`, the bundled web assets, the
offline video, and the two placeholders. **No signing tool.** G §1 adds "`kioskctl` for the
signing workflow" to the `.deb` payload. See §2.4 for what `kioskctl` actually is.

---

## §2 — What exists on disk

### 2.1 `packaging/` — **windows only**

```
packaging/windows/{verify-webview2.ps1, README.md, sign.ps1, install-task.ps1,
                   kiosk.wixproj, bundle.wxs, kiosk.wxs, bundle.wixproj,
                   lockdown.md, KioskLauncher.xml}
```
No `packaging/linux/`, no `packaging/android/`. Parent §4's tree (line 391-394) lists all
three; only `windows/` has been created. G's `packaging/linux/` is entirely new — **VERIFIED
as not-yet-existing** (this is consistent with G being a design spec, recorded for accuracy).

### 2.2 `dist-template/` — 3 files, all placeholders

| File | Present | Content |
|---|---|---|
| `kiosk.ini` | yes (368 B) | `; OBVIOUSLY FAKE PLACEHOLDER. Replace per device before production use.` — full `[kiosk]`/`[bootstrap]` template, `credential = kiosk-credential.json` |
| `kiosk-credential.json` | yes (266 B) | valid JSON, `"project_id": "OBVIOUSLY-FAKE-REPLACE-ME"`, non-empty `client_email`/`private_key`/`token_uri` |
| `kiosk-offline.mp4` | yes (**88 B**) | **not a video** — plain text: `OBVIOUSLY FAKE VIDEO PLACEHOLDER. Replace with a valid MP4 before production packaging.` |

**Note:** parent §4's tree also lists `kiosk-config.example.json` in `dist-template/`; it
does **not** exist. Pre-existing drift, not G's.

G's open decision "Whether the offline mp4 ships in the `.deb` or the runbook places it
(size vs completeness; lean: ship the default…)" — **there is no default mp4 to ship**; the
repo's is an 88-byte text placeholder. The "size vs completeness" framing assumes an asset
that does not exist. Undeclared assumption.

### 2.3 `bundled/` — **VERIFIED, 5 pages**

`crates/kiosk-main/bundled/` = `error.html`, `offline.html`, `pinpad.html`, `safe.html`,
`splash.html`. Matches F2's list exactly. (Parent §4's tree calls the directory `assets/`
and lists only four — pre-existing drift, not G's.)

### 2.4 `kioskctl` — **FALSE as a payload item**

It is **not** a binary crate and **not** a workspace member. It is a **cargo example**:
`/home/user/kiosk-browser/crates/kiosk-core/examples/kioskctl.rs`. Workspace members are
exactly `crates/kiosk-{core,main,launcher}` (`Cargo.toml`). It is not produced by a default
`cargo build --release`; it needs `cargo build -p kiosk-core --example kioskctl`.

Its own module doc:

> //! kioskctl — fleet config signing tool (smoke/ops use).
> …
> //!   KIOSK_SIGNING_KEY_B64=<seed> … sign config.json > signed.json

It consumes the **fleet private signing seed**. Shipping it in the on-device `.deb` is a
divergence from F2 (§1.6) that G does not state in either direction (C3).

### 2.5 Payload list vs reality — summary

| G's payload item | On disk | Verdict |
|---|---|---|
| `kiosk-main`, `kiosk-launcher` | crates exist | VERIFIED |
| `bundled/` pages | 5 files | VERIFIED |
| `kiosk-offline.mp4` | 88-byte text placeholder | DRIFT (no real default exists) |
| `kiosk.service` | does not exist yet | new (expected) |
| `kioskctl` | cargo *example*, signing-key tool | FALSE as stated |
| `kiosk.ini` template | `dist-template/kiosk.ini` | VERIFIED |
| credential | G ships none; F2 ships the placeholder | see 1.4 |

---

## §3 — Install-dir paths: **CONTRADICTION with the parent spec (undeclared)**

### What the parent says

`2026-07-05-kiosk-browser-design.md`, **§4 "File & directory conventions"**, lines 405-410,
verbatim:

```
| Item | Windows | Linux | Android |
|---|---|---|---|
| Install dir (read-only) | `C:\Program Files\kiosk\` | `/opt/kiosk/` | APK |
| `kiosk.ini`, credential, mp4 | next to binaries (override: `--config <path>`) | same | app files dir |
| Data dir (cache, spool, last-good) | `%ProgramData%\kiosk\` | `/var/lib/kiosk/` | app files dir |
```

### What G says

G §1, Payload bullet: "`kiosk-main` + `kiosk-launcher` → **`/usr/libexec/kiosk/`**; bundled
assets alongside".

### What C says

`2026-08-06-p2c-linux-launcher-supervision-design.md:85`, unit contract:

```ini
ExecStart=cage -- /usr/libexec/kiosk/kiosk-launcher
```

(Note for the record: C's line is `cage -- /usr/libexec/kiosk/kiosk-launcher`, i.e. cage is
the unit's main process, **not** a bare `ExecStart=/usr/libexec/kiosk/kiosk-launcher`.)

### Verdict: **FALSE / CONTRADICTION, undeclared**

`/usr/libexec/kiosk/` ≠ `/opt/kiosk/`. G and C both use `/usr/libexec/kiosk/`; neither spec
states the divergence, neither justifies it, and C's line 84 explicitly says "values and
installation in P2-G", i.e. G is the spec that owns this path.

**Is the parent's table normative?** It carries no illustrative/non-binding hedge; it sits
under a `### File & directory conventions` heading; and both A and G cite the **same table**
as authority. G's own §1 State-dirs bullet: "postinst creates `/var/lib/kiosk` (`0750
root:root`) — **parent §4's path**, A's `resolve_data_dir`." G therefore cites row 3 of the
table as binding in the same bullet list where it silently overrides row 1. It cannot be
illustrative for one row and normative for the next.

### Does any code hardcode `/opt/kiosk`? — **No**

- `crates/kiosk-main/src/main.rs:436` `resolve_data_dir()` → `std::env::var_os("ProgramData")
  … .join("kiosk")`, falling back to `PathBuf::from(".")`. Windows-shaped; on Linux **today**
  it resolves to `./kiosk` (cwd). Same function duplicated at
  `crates/kiosk-launcher/src/main.rs:48`.
- A's spec (`p2a`, line 113) plans the change: "`resolve_data_dir()` → `/var/lib/kiosk/` on
  Linux (parent spec §4, never operator-overridden)". So G's `/var/lib/kiosk` claim is
  VERIFIED against A's *design*, not against shipped code.
- `crates/kiosk-main/src/main.rs:423-426` `resolve_config_dir()` → dir of `current_exe()`
  unless `--config <dir>`.
- `crates/kiosk-launcher/src/main.rs:56` "The supervised binary: `kiosk-main` next to this exe."

So nothing in code pins `/opt/kiosk`; the install dir is code-agnostic.

### The consequence G does not state

Because `config_dir` = the binary's directory (`main.rs:423-426`), everything else follows it:

- `main.rs:655` `let ini_path = config_dir.join("kiosk.ini");`
- `main.rs:730` `let credential_path = config_dir.join(&bootstrap.credential);`
- `main.rs:999` `let mp4 = config_dir.join("kiosk-offline.mp4");`

With binaries at `/usr/libexec/kiosk/`, the **configuration file, the credential secret, and
the operator-replaceable video all land under `/usr`**. Debian Policy 10.7.2 (quoted in §7
below) forbids configuration files outside `/etc`, and `/usr` is expected to be mountable
read-only. G does not name this. Undeclared assumption / factual conflict.

---

## §4 — SEC-09 / the credential

### Parent wording

§4, lines 418-420:
> The credential must have a restrictive owner-only ACL/mode; the default
> `C:\Program Files\kiosk\` is world-readable, so the installer MUST tighten it (WiX
> `util:PermissionEx` on Windows; **`root:root 0600` or the keyring on Linux**) — see §8/SEC-09.

§8, lines 739-743:
> **Credential at rest (SEC-09).** Stored in the OS keystore, not a flat file (Windows
> DPAPI/Credential Manager, Linux kernel keyring or `root:root 0600`, Android Keystore).
> Permissions are enforced **fail-closed**: the installer sets an owner-only ACL/mode …
> at boot AND on every config reload, if the credential lacks its required restrictive mode
> the device refuses to load it and enters safe mode …

G's `0600 root:root` matches. **VERIFIED.**

### The Unix check A and C specify

`p2a` lines 269-273:
```rust
#[cfg(unix)]
pub fn credential_is_owner_only(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o077 == 0)
}
```
`p2c:135`: "Same `#[cfg(unix)]` mode-bits implementation … `metadata.permissions().mode() &
0o077 == 0`, `Err` stays a violation via the existing `is_violation`." **VERIFIED both.**

Current shipped code is still the fail-open stub — `credential_acl.rs:100-104`:
```rust
/// Non-Windows stub (dev hosts only; the kiosk target is Windows x64).
#[cfg(not(windows))]
pub fn credential_is_owner_only(_path: &Path) -> io::Result<bool> {
    Ok(true)
}
```

### The gates

`credential_acl.rs:24-26`:
```rust
pub fn is_violation(check: io::Result<bool>) -> bool {
    !matches!(check, Ok(true))
}
```

Boot gate — `crates/kiosk-main/src/boot.rs:161-179`:
```rust
    // … `Ok(false)`/`Err` (fail closed — a
    // missing file included, since `credential_is_owner_only` itself fails to
    // read a nonexistent path's security info) is a violation; only `Ok(true)`
    // proceeds to read and parse.
    if credential_acl::is_violation(credential_acl::credential_is_owner_only(&credential_path)) {
        return BootOutcome::RenderSafe {
            booted,
            error: CREDENTIAL_PERMISSIONS_MESSAGE.to_string(),
            reason: Some(CREDENTIAL_PERMISSIONS_REASON),
        };
    }

    let credential = std::fs::read_to_string(&credential_path)
        .map_err(|e| format!("cannot read {} ({e})", credential_path.display()))
        .and_then(|json| {
            ServiceAccount::from_json(&json)
                .map(|_| ())
                .map_err(|e| format!("cannot parse {} ({e})", credential_path.display()))
        });
```
…and on parse failure (`boot.rs:183-187`): `RenderSafe { error, reason: None }`.

Fetch gate — `crates/kiosk-main/src/fetch.rs:100-106`:
```rust
        if credential_acl::is_violation(credential_acl::credential_is_owner_only(&credential_path))
        {
            telem.config_error(crate::boot::CREDENTIAL_PERMISSIONS_REASON);
            let _ = credential_violation_tx
                .try_send(crate::boot::CREDENTIAL_PERMISSIONS_MESSAGE.to_string());
            break;
        }
```

`ServiceAccount::from_json` — `crates/kiosk-core/src/logging/auth.rs:164-175`: `serde_json::
from_str` → `AuthError::MalformedJson` on parse failure; then rejects empty
`client_email` / `private_key` / `token_uri`.

### Behaviour matrix (factual)

| Credential file state (Linux, with A's impl) | `credential_is_owner_only` | `is_violation` | Boot outcome | Telemetry reason | Fetch loop |
|---|---|---|---|---|---|
| **MISSING** | `Err` (`metadata()?`) | true | `RenderSafe` | `credential_permissions` | `config_error(credential_permissions)` then `break` |
| **G's pre-created EMPTY, `0600`** | `Ok(true)` | false | falls through; `from_json("")` → `MalformedJson` → `RenderSafe` | **`reason: None`** ("cannot parse …") | **keeps polling** (gate passes) |
| **F2-style placeholder JSON, `0600`** | `Ok(true)` | false | `from_json` succeeds (all three fields non-empty) → **`BootOutcome::Ready`** | none | keeps polling; auth fails later at token exchange |
| Real credential, `0644` | `Ok(false)` | true | `RenderSafe` | `credential_permissions` | `config_error` + `break` |

**Findings, stated factually:**

1. G's pre-created empty `0600` file **does** satisfy the mode gate — it is not fail-open on
   permissions. But it **changes the failure classification of an unprovisioned device**
   from `config.error{credential_permissions}` (today's missing-file path) to a generic
   `reason: None` parse error. The SEC-09-named signal disappears for exactly the state the
   pre-creation is meant to cover.
2. It also removes the fetch-loop `break`: with a missing file the reload gate stops the
   fetch task; with an empty-but-0600 file the task keeps polling.
3. Neither behaviour is a fail-open on the security gate (C5 intact); both are observability
   changes (Q3 class), unstated in G.

### Two further factual interactions G does not state

- **`0600 root:root` credential + a non-root service user.** G's own leading open decision
  leans "dedicated user + logind". A root-owned `0600` file is unreadable by a `kiosk` user.
  A's spec already flags exactly this, `p2a:275-276`:
  > `ponytail:` mode bits only, no uid check — a root-owned `0o600` file is the deployment
  > shape; **add an owner check if a non-root service user lands in P2-C.**

  G neither carries that deferral forward nor gives it an H-row. **Unowned deferral.**
- **`/var/lib/kiosk` at `0750 root:root` + a non-root service user** ⇒ spool / last-good /
  cache writes fail. Same class, also unstated.
- **postinst cannot know the credential path in general.** It is
  `config_dir.join(&bootstrap.credential)` (`main.rs:730`) — i.e. whatever `kiosk.ini`'s
  `credential =` key says (template default `kiosk-credential.json`). "postinst pre-creates
  the credential path" assumes the operator never changes that key. Undeclared assumption.

---

## §5 — Dependency list

All seven names checked against **Debian 12 (bookworm)** via `packages.debian.org` (tier 5
evidence; local apt is Ubuntu noble and cannot answer for bookworm).

| Package | In Debian 12? | Version | Archive area |
|---|---|---|---|
| `libwebkit2gtk-4.1-0` | yes | `2.50.6-1~deb12u2 and others` | main |
| `libgtk-3-0` | yes | `3.24.38-2~deb12u3` | main |
| `cage` | **yes** | **`0.1.4-4`** | **main** (`deb.debian.org/debian/pool/main/c/cage/cage_0.1.4-4.dsc`) |
| `gstreamer1.0-plugins-base` | yes | `1.22.0-3+deb12u6` | main |
| `gstreamer1.0-plugins-good` | yes | `1.22.0-5+deb12u3` | main |
| `gstreamer1.0-plugins-bad` | yes | `1.22.0-4+deb12u7` | main |
| `gstreamer1.0-libav` | yes | `1.22.0-2` | main (`pool/main/g/gst-libav1.0/`) |

**Verdict: VERIFIED** — all seven are real Debian 12 binary package names, and `cage` **is**
in Debian 12 **main** at **0.1.4-4**.

The four GStreamer names are verbatim parent §3.4, lines 286-289:
> The Linux `.deb` declares the exact GStreamer decode chain:
> `gstreamer1.0-plugins-base`, `gstreamer1.0-plugins-good` (qtdemux),
> `gstreamer1.0-plugins-bad` (h264parse), `gstreamer1.0-libav` (avdec_h264)

**VERIFIED verbatim.**

Local-environment caveat (recorded, not a defect against the Debian 12 floor): on this host
(Ubuntu 24.04) `apt-cache policy libgtk-3-0` returns **`Candidate: (none)`** — noble renamed
it `libgtk-3-0t64` in the `time_t` transition. Irrelevant to Debian 12 and to Ubuntu 22.04
(both still ship `libgtk-3-0`), but it means a `Depends: libgtk-3-0` `.deb` is uninstallable
on Ubuntu 24.04+.

**UNVERIFIABLE here:** that these seven actually *resolve together* and that the resulting
package installs. **Pinning mechanism:** G's own proposed `debian:12` container
install/remove/upgrade cycle. See §10 — no declared CI job currently runs it.

---

## §6 — systemd / logind directives

Verified against systemd 255 on this host using `systemd-analyze verify` (mechanical), the
shipped `/etc/systemd/logind.conf` defaults file, `systemctl list-unit-files`, and the
kernel v6.1 `kernel-parameters.txt` (Debian 12's kernel line).

### The headline: `StartLimitIntervalSec` is a `[Unit]` directive — **FALSE placement**

Mechanical proof:

```
$ systemd-analyze verify kiosk-a.service      # StartLimitIntervalSec in [Service]
kiosk-a.service:10: Unknown key name 'StartLimitIntervalSec' in section 'Service', ignoring.

$ systemd-analyze verify kiosk-b.service      # StartLimitIntervalSec in [Unit]
(no output — clean)
```

Per-key, in `[Service]`:

| Key in `[Service]` | systemd 255 result |
|---|---|
| `StartLimitIntervalSec` | **"Unknown key name … ignoring"** — silently dropped |
| `StartLimitBurst` | accepted (legacy compat) |
| `StartLimitInterval` (old name) | accepted (legacy compat) |

C's contract shape (`p2c:82-89`) places the entire block under `[Service]`, and G says it
supplies "`StartLimitIntervalSec`/`StartLimitBurst` chosen here" into that shape. If an
implementer follows C's block literally, **the interval value is silently discarded and
systemd falls back to its default 10 s window** while G's burst value *is* applied. G's own
open decision states the requirement this breaks: "systemd's limits must be strictly looser
than the FSM, so the FSM, not systemd, is always the authority that gives up first." A
dropped interval makes the effective window an order of magnitude tighter than intended,
with no error at load time beyond one journal line.

### `SuccessExitStatus=86` — **ABSENT from G (and from C)**

Parent §3.1, lines 170-173, verbatim:
> The autostart integration must **exempt code 86** from auto-restart so a technician exit
> reaches the desktop: systemd `Restart=always` with `RestartPreventExitStatus=86` **and
> `SuccessExitStatus=86`**;

G §1 enumerates the unit values it "chooses here": `StartLimitIntervalSec`/
`StartLimitBurst`, `RuntimeDirectory=kiosk`, `RestartPreventExitStatus=86`. **No
`SuccessExitStatus=86`.** C's shape (`p2c:82-89`) also omits it, and C explicitly hands
"values and installation" to G. So the parent-named directive has no owner.

Factual effect: without it, a technician exit records `Result=exit-code` / `Active: failed`
in `systemctl status` and the journal — a clean, intentional exit is indistinguishable from
a crash to the field technician and to any monitoring reading unit state.

(`systemd-analyze verify` accepts `RestartPreventExitStatus=86` and `SuccessExitStatus=86`
together with `Restart=always` — no conflict.)

### Every other directive, checked

| Directive | Real? | Correct config location? | Verdict |
|---|---|---|---|
| `StartLimitIntervalSec` | yes | **`[Unit]`**, not `[Service]` | **FALSE placement** (proof above) |
| `StartLimitBurst` | yes | `[Unit]`; `[Service]` accepted for compat | DRIFT (works, but split from its sibling) |
| `RuntimeDirectory=kiosk` | yes | `[Service]` / systemd.exec | **VERIFIED**; cross-checked against C:39-49 (`/run/kiosk/hb-<pid>.sock`, wiped on stop) |
| `RestartPreventExitStatus=86` | yes | `[Service]` | **VERIFIED** |
| `SuccessExitStatus=86` | yes | `[Service]` | **ABSENT** — see above |
| `NAutoVTs=0` | yes | `logind.conf` `[Login]` | **VERIFIED** — host file shows `#NAutoVTs=6` (default 6) |
| `ReserveVT=0` | yes | `logind.conf` `[Login]` | **VERIFIED** — host file shows `#ReserveVT=6` (default 6) |
| `IdleAction=ignore` | yes | `logind.conf` `[Login]` | **VERIFIED name/location, but it is already the default** — host file line 44: `#IdleAction=ignore`. Also governs *system* idle action (suspend/poweroff), **not display blanking**. |
| `consoleblank=0` | yes | **kernel cmdline** (correct file) | **VERIFIED name/location, already the default and wrong layer** — kernel v6.1 `kernel-parameters.txt`: "consoleblank= [KNL] The console blank (screen saver) timeout in seconds. A value of 0 disables the blank timer. **Defaults to 0.**" It affects the VT text console only, not a Wayland/cage DRM output. |
| mask `sleep.target suspend.target hibernate.target hybrid-sleep.target` | yes | all four exist | **VERIFIED** — `systemctl list-unit-files` shows all four, `static` |
| `apt-mark hold` | yes | apt | **VERIFIED** — `apt-mark --help`: "hold - Mark a package as held back" |
| `deb-systemd-invoke` | yes | maintscripts | **VERIFIED** — `/usr/bin/deb-systemd-invoke` (`init-system-helpers`); doc: "wrapper around systemctl, respecting policy-rc.d" |
| `policy-rc.d` | yes | `/usr/sbin/policy-rc.d` | **VERIFIED** — invoke asks it first; "0 or 104 means run / 101 means do not run" |
| conffile for the mp4 | — | — | see §7 |

### `systemctl enable kiosk.service` in postinst — **FALSE (wrong helper)**

G §1 Autostart: "`systemctl enable kiosk.service` in postinst (the Scheduled-Task analogue);
`deb-systemd-invoke` conventions so policy-rc.d environments behave."

`deb-systemd-invoke` is the **start/stop/restart** helper. The **enable** helper is
`deb-systemd-helper`, and its own documentation states the reason it exists
(`/usr/bin/deb-systemd-helper`, DESCRIPTION):

> The "enable" action will only be performed once (when first installing the package). On the
> first "enable", a state file is created which will be deleted upon "purge".
> …
> The "was-enabled" action is not present in systemctl, but is required in Debian so that we
> can figure out whether a service was enabled before we installed an updated service file.

A raw `systemctl enable` in postinst therefore **re-enables the unit on every upgrade**,
silently reverting an operator's deliberate `systemctl disable` — the same class of state
G's "upgrades preserve the three operator-owned files" bullet is written to protect. It is
also not `policy-rc.d`-aware and needs `--root` to work in a chroot/container build. G names
the wrong one of the two non-interchangeable helpers.

### `deb-systemd-invoke try-restart` — **DRIFT (works, undocumented verb)**

G's error-handling section: "upgrade path: `deb-systemd-invoke try-restart` after unpack".
The script's documented synopsis is `start|stop|restart` (+ `daemon-reload|daemon-reexec`).
`try-restart` falls into the script's terminal branch:
```perl
} else {
    exec('systemctl', @ARGV);
}
```
so it does execute — but it **bypasses the enabled/active guard** that the script applies
specifically to `start`/`restart` ("If the job is disabled and is not currently running, the
job is not started or restarted"). Real, functional, outside the documented contract.

---

## §7 — The conffile claim

G §1: the mp4 is "user-replaceable per §3.4, **marked conffile-adjacent** so upgrades don't
clobber a replaced video". G's open decisions: "lean: ship the default, conffile-protect the
replacement."

### Can a file under `/usr/libexec/` or `/opt/` be a conffile?

**Mechanically at the dpkg level: yes.** I built and installed a test package to prove it:

```
DEBIAN/conffiles:
  /usr/libexec/kiosk/kiosk-offline.mp4
  /etc/kiosk/kiosk.ini
```
`dpkg-deb --build` succeeded; `dpkg --root=… -i` installed it; I then overwrote both files
("OPERATOR-REPLACED"), built a v2 with different content, and upgraded with
`--force-confold`:
```
 ==> Using current old file as you requested.
=== after upgrade ===
mp4: OPERATOR-REPLACED
ini: OPERATOR-INI
```
dpkg preserved the replaced file under `/usr/libexec/`. The *mechanism* works.

### What Debian actually requires — **it is a policy violation and a lintian ERROR**

Debian Policy **10.7.2 Location**, verbatim:

> Any configuration files created or used by your package **must** reside in `/etc`. If there
> are several, consider creating a subdirectory of `/etc` named after your package.
> If your package creates or uses configuration files outside of `/etc`, and it is not
> feasible to modify the package to use `/etc` directly, put the files in `/etc` and create
> symbolic links to those files from the location that the package requires.

Lintian tag **`file-in-usr-marked-as-conffile`**, verbatim:

> All configuration files must reside in /etc. Files below /usr may not be marked as
> conffiles since /usr might be mounted read-only. The local system administrator would
> therefore not have a chance to modify this configuration file.
> **Severity: error**

So a conffile at `/usr/libexec/kiosk/kiosk-offline.mp4` **fails G's own "Lintian clean"
gate** with an *error*-severity tag. Under `/opt/kiosk/` (the parent's install dir) that
specific tag does not fire, but Policy 10.7.2 is still violated.

### Additional factual points

- "**conffile-adjacent**" is not a dpkg concept. A file is either listed in
  `DEBIAN/conffiles` or it is not; there is no adjacent state.
- An **mp4 is not a configuration file**. Policy's conffile machinery is defined for
  configuration files; marking a binary blob as one makes dpkg present the operator with a
  `==> Modified (by you or by a script) since installation. / ==> Package distributor has
  shipped an updated version.` prompt for a video on every upgrade that changes it
  (reproduced verbatim in the test above).
- The three "operator-owned files" G promises to preserve are not symmetric: the **credential
  is postinst-created and never shipped**, so dpkg never touches it — preserved for free, no
  conffile needed. Only `kiosk.ini` and the mp4 would need the mechanism.

### What actually preserves a replaced binary asset

Stating the factual alternatives, not recommending one:

1. Policy 10.7.3's **second method**: do not list it as a conffile and do not ship it in the
   package; postinst places a default only if the path is absent (or `ucf` mediates).
2. The app already resolves the mp4 as `config_dir.join("kiosk-offline.mp4")`
   (`main.rs:999`), where `config_dir` is the binary dir **or `--config <dir>`**
   (`main.rs:423-426`, and parent §4 row 2: "next to binaries (override: `--config <path>`)").
   An operator override path needs no dpkg machinery at all.
3. Policy 10.7.2's own escape hatch: file in `/etc`, symlink from the install dir.

---

## §8 — Cross-spec consistency: the H1–H8 checklist

Sources: the five `2026-08-06-p2{a,b,c,d,e}-*.md` specs.

| # | G's cited origin | Does the cited deferral exist? | Evidence |
|---|---|---|---|
| **H1** | A (Wayland monitor semantics open item) | **DRIFT** — item exists, but A defers it to *implementation under weston*, not to hardware | `p2a:339-342`: "**Wayland monitor placement:** Wayland reports dummy monitor positions; `display.monitor` selection may degrade to primary + `config.warn` … **Confirm tao's actual behavior under weston during implementation**; document observed behavior." Filed under "Open decisions to resolve at plan time". |
| **H2** | C smoke 14's systemd half | **VERIFIED (verbatim)** | `p2c:155-156`: "14. **technician exit:** drive the pinpad exit → assert the *launcher process* exits 86 (**the systemd half of the contract is asserted at P2-G's image validation**)." |
| **H3** | B smoke 12's deferred half | **VERIFIED (verbatim)** | `p2b:193-196`: "12. **keep-awake:** the container has no systemd, so the smoke asserts only the degrade path … The positive assertion (`systemd-inhibit --list` shows the hold) **goes on the deferred hardware checklist with cage**." |
| **H4** | D (smoke 17 if headless virtual input was unavailable; §7 keyboard row) | **VERIFIED**, both halves | `p2d:124-128`: "17. … (blocking under cage-headless **IF virtual input is available, else hardware-checklist**) … **if it does not, scenario 17 moves to the deferred hardware list with the cage items — recorded, not silently dropped.**" And parent §7 line 697: "Linux: **squeekboard/onboard** deployment docs"; `p2d:162` defers it: "on-screen keyboard (squeekboard/onboard per parent §7) … → P2-G". *(`GDK_TOUCH_CANCEL`, also in H4, is a D **plan-time** open decision (`p2d:157-158`), not a hardware deferral — G promotes it; recorded, harmless.)* |
| **H5** | E / RT-05 | **VERIFIED (verbatim)** | `p2e:99`: "**hardware** ≥72 h (**G checklist, RT-05**)"; `p2e:59`: "the visual check is a hardware-checklist item"; `p2e:132`: "`.deb` gst deps + hardware soak + visual checks → P2-G". |
| **H6** | B/D + §10 | **PARTIAL** — the §7.2 escape vectors exist; the "**§10 hardening list: chords, edges, dialogs, VT attempts**" does not | Parent §10, line 880-882: "per-platform manual smoke checklist in **`docs/testing.md`** (hardening set incl. **the escape vectors in §7.2**, video loop, reconnect, watchdog kill test)". §10 contains no enumerated hardening list; the enumeration is G's. Also **`docs/testing.md` does not exist** — `docs/testing/` is a directory containing only `p1d2-signed-config-smoke.md`. |
| **H7** | A/B | **FALSE citation** | No deferral about a real network, DNS failure modes, or captive portals exists in A or B. `grep -rn -i "dns\|captive"` across all six P2 specs returns exactly **one** hit: G's own H7 row. (The parent mentions captive portals at §3.3 line 246 and DNS at line 572 — neither is a deferred item, and neither is "A/B".) The item may be worth doing; the *origin* is invented. |
| **H8** | G §2-3 | **VERIFIED** (self-citation; G §3 does specify the `dpkg -l` snapshot + timed cold run) | |

### Deferred-to-hardware items in A–E with **no** H-row

Scanned all five specs for hardware deferrals. Result: **no A–E hardware deferral is
dropped.** H1–H5 cover A/B/C/D/E's four explicit hardware deferrals plus A's monitor item;
D's `seat permissions` deferral (`p2d:162`) lands in G's open fork; B's `IdleAction`
"asserted at hardware validation" (`p2b:172`) is behaviourally covered by H3's 24 h
no-blank assertion.

**The gap is in the other direction** — parent §7.2 items with no H-row and no runbook step.
See §9.

---

## §9 — Parent §7.2 Linux and §9 P2 row vs G's runbook

### Parent §7.2, Linux bullet, verbatim (lines 716-721)

> - **Linux.** cage (Wayland) locked session as the supported secure config (X11/openbox is
>   documented but NOT app-enforced — demo only). Disable VT switching and zap: logind
>   `NAutoVTs=0`/`ReserveVT=0` (or X11 `DontVTSwitch`/`DontZap`); **run on a dedicated seat
>   with no other TTYs**; **disable DPMS/screensaver in the cage session**; mask
>   sleep/suspend targets (H5/PF-07/M8).

### Parent §9, P2 row, verbatim (line 839)

> **P2** | Linux + robustness | WebKitGTK parity (incl. pinch-gesture intercept, keep-awake at
> compositor), **.deb + systemd + cage docs + §7.2 Linux hardening**, idle reset (native),
> **memory cap restart + health-sampled RSS**, cross-platform webview-hang detection (JS
> ping), config-driven `inject_css`/`inject_js` knobs (behind signed config), remote log
> level, restart_app

### Item-by-item

| §7.2 Linux item | In G's runbook? | Verdict |
|---|---|---|
| cage locked session as the supported secure config | yes, first bullet | **VERIFIED** |
| X11/openbox documented but NOT app-enforced, demo only | yes — "**X11/openbox stays demo-only, documented as NOT app-enforced** (parent §7.2 verbatim — one appendix paragraph, no more)" | **VERIFIED (verbatim)** |
| logind `NAutoVTs=0` / `ReserveVT=0` | yes | **VERIFIED** |
| X11 `DontVTSwitch` / `DontZap` (the parenthetical alternative) | **no** — neither name appears in G | **OMITTED** (mitigated: G caps X11 at one appendix paragraph) |
| **run on a dedicated seat with no other TTYs** | **partially** — G has "no getty on the kiosk seat", but the *dedicated seat* itself is demoted from a gate requirement to G's "**one open fork**", resolved by evidence at H1 | **DRIFT** — the parent states it as a deployment-gate requirement, G states it as an open decision |
| **disable DPMS/screensaver in the cage session** | **no** | **OMITTED — headline** (below) |
| mask sleep/suspend targets | yes, and expanded to all four (`sleep`/`suspend`/`hibernate`/`hybrid-sleep`) | **VERIFIED+** |

### The DPMS/blanking omission, in full

Parent §7 keep-awake row, line 695, verbatim:
> Linux/Wayland: `systemd-inhibit` blocks *suspend* only, display blanking is
> compositor-owned — **PRIMARY is configuring cage/wlroots not to blank** (idle-inhibit is
> secondary and only if wry exposes an inhibitor surface; validated in P2, PF-07)

Parent §11 risk row, line 895:
> keep_awake blocks suspend but not display blanking on Wayland/cage | **P2 disables blanking
> at the compositor (cage/wlroots) as primary**

G's runbook "Sleep/idle" bullet offers three mechanisms in place of it:
- mask the four sleep targets — suspend, not blanking;
- `IdleAction=ignore` — already the logind default (§6), and system idle action, not blanking;
- `consoleblank=0` — already the kernel default on 6.1 (§6), and the **VT text console**, not
  the Wayland/DRM output;
- "no screensaver/idle daemon installed" — an absence, not a compositor configuration.

**None of these configures cage/wlroots not to blank.** G is the spec that owns the §7.2
runbook, B explicitly hands the suspenders to G ("The *suspenders* are P2-G's image
contract: no idle daemon, `IdleAction=ignore` in `logind.conf`", `p2b:171-172`), and G's H3
asserts the *outcome* ("display never blanks over 24 h") with no step in the runbook that
would make it true. The parent's **PRIMARY** keep-awake mechanism has no owning step.

### One misattributed section reference

G §2, Updates bullet: "(**parent §9's** pinned-image intent, expressed as a package hold on
stock Debian)". `grep -n "pinned"` on the parent: the phrase "pinned Debian 12 image" occurs
at **line 289 (§3.4)** and **line 874 (§10)**. **§9 never mentions a pinned image** — its
only Linux platform text is the floor paragraph ("Ubuntu 22.04 / Debian 12 (webkit2gtk-4.1
…; cage required for secure lockdown)"). **DRIFT** — correct citations are §3.4 and §10.

---

## §10 — Lintian, `dpkg -l`, `debian:12`

| Claim | Verdict | Evidence |
|---|---|---|
| "Lintian clean (or documented overrides)" | **UNVERIFIABLE here + one known-error tag** | `lintian` is **not installed** (`apt-cache policy lintian` → `Installed: (none)`, candidate `2.117.0ubuntu1.4`). Cannot run it. But the specific tag the conffile plan triggers is documented and **severity: error** (§7). **Pinning mechanism:** a `lintian` invocation in F's release job — F §3 (`p2f:55-57`) currently specifies **`dpkg-deb` assembly only**, no lintian step. G asserts a gate no declared job runs (C9). |
| `dpkg -l` snapshot as the image pin | **VERIFIED (works)** | `dpkg 1.22.6` present; piped output is not width-truncated (max line 172 chars on this host) and hold state shows in the desired-action column (`hi`). Adequate as a snapshot. (`dpkg --get-selections` / `dpkg-query -W -f=…` are the more canonical machine-diffable forms; not a defect.) |
| `debian:12` container availability | **VERIFIED as an image; UNVERIFIABLE as a test here** | Docker Hub API confirms `library/debian:12`, amd64, `status: active`, last pushed 2026-08-05, 48.5 MB. But **no docker daemon** in this environment (`dial unix /var/run/docker.sock: no such file or directory`), so the install/remove/upgrade cycle cannot be executed. **Pinning mechanism:** a CI job with a container runtime. G says "the install cycle test lives with G" but declares no job, workflow, or trigger for it; F's `release` job is tag-triggered and does assembly only. The gate has no runner (C9). |
| "F's release job runs assembly" | **VERIFIED** | `p2f:55-57`: "Tag push `v*`: existing release builds, plus **`.deb` assembly from P2-G's `packaging/linux/`** (dpkg-deb; the package *content* is G's spec, F only executes it)". |
| "unattended-upgrades off (F §4 — update timing is operator-owned)" | **VERIFIED** | `p2f:68-70`: "an apt repository, `unattended-upgrades` policy, delta/A-B updates. **The G runbook pins `unattended-upgrades` off** so the fleet's [update timing is operator-owned]". |
| "version from the workspace" | **VERIFIED** | root `Cargo.toml`: `[workspace.package] version = "0.1.0"`. |

---

## Claims stated as fact that are actually undeclared assumptions

Listed separately per the task. Each is asserted in G's prose without a hedge, a stated
assumption, or a pinning mechanism.

1. **`/usr/libexec/kiosk/` is the install dir.** Asserted flatly; contradicts parent §4's
   `/opt/kiosk/` with no divergence statement (§3).
2. **`kiosk.ini` + the credential + the mp4 can live under `/usr`.** Follows mechanically
   from #1 via `resolve_config_dir` (`main.rs:423-426`, `:655`, `:730`, `:999`); never
   stated; conflicts with Policy 10.7.2 and with a read-only `/usr`.
3. **A conffile can protect a file outside `/etc`.** Presented as settled ("conffile-adjacent",
   "conffile-protect the replacement"). dpkg permits it; Debian Policy forbids it and lintian
   errors on it under `/usr` (§7) — which also silently contradicts G's own lintian gate.
4. **A default `kiosk-offline.mp4` exists to ship.** The repo's is an 88-byte text
   placeholder (§2.2). The "size vs completeness" open decision is framed around an asset
   that does not exist.
5. **`kioskctl` is a shippable binary.** It is a cargo example in `kiosk-core`, built only
   with `--example`, and it is the fleet **private-signing-key** tool (§2.4).
6. **postinst knows the credential path.** The path is `kiosk.ini`'s operator-editable
   `credential =` value (`main.rs:730`).
7. **`0600 root:root` credential and `0750 root:root` data dir are compatible with the
   service user.** G's own lean is a non-root `kiosk` user, under which both are unreadable/
   unwritable. A already flagged the trigger (`p2a:275-276`) and G does not carry it forward.
8. **`StartLimitIntervalSec` takes effect in C's `[Service]` block.** systemd 255 discards it
   (§6, mechanically proven).
9. **`consoleblank=0` and `IdleAction=ignore` are keep-awake controls.** Both are already the
   stock defaults, and neither touches Wayland/DRM display blanking (§6, §9).
10. **`systemctl enable` in postinst is the Debian convention.** `deb-systemd-helper enable`
    is; the difference is upgrade-time re-enable behaviour (§6).
11. **The `debian:12` install-cycle test and the lintian gate have a runner.** No workflow,
    job, or trigger is declared in G or in F (§10).
12. **H7's origin is "A/B".** No such deferral exists in either (§8).
13. **The parent's `pinned image` intent is in §9.** It is in §3.4 and §10 (§9 of this report).

---

## Counts

| Verdict | Count |
|---|---|
| **VERIFIED** | 31 |
| **FALSE** | 7 |
| **DRIFT** | 9 |
| **UNVERIFIABLE** | 3 |
| **Undeclared assumptions** (listed separately) | 13 |

### FALSE (the seven)

1. **Install dir `/usr/libexec/kiosk/` contradicts parent §4's `/opt/kiosk/`**, undeclared,
   and G cites the same §4 table as authority for `/var/lib/kiosk` two bullets later.
2. **"Secrets discipline (F2 verbatim): … no credential"** — F2 ships a *placeholder*
   credential and says so in the same sentence G paraphrases.
3. **`StartLimitIntervalSec` in C's `[Service]` block is silently ignored** by systemd 255
   ("Unknown key name … ignoring"), while `StartLimitBurst` is applied — proven by
   `systemd-analyze verify`. The rate limit G's open decision depends on is not the one that
   would take effect.
4. **`SuccessExitStatus=86` is absent** from G's unit values and from C's shape, though
   parent §3.1 names it explicitly alongside `RestartPreventExitStatus=86`, and C hands unit
   *values* to G.
5. **The mp4 conffile plan is a lintian `error`** (`file-in-usr-marked-as-conffile`) and a
   Policy 10.7.2 violation — it fails G's own "Lintian clean" gate. (dpkg *will* preserve the
   file; I proved that too.)
6. **`systemctl enable` in postinst is the wrong helper** — `deb-systemd-helper enable` is
   the once-only, state-tracked Debian convention; a raw `systemctl enable` re-enables on
   every upgrade, reverting an operator's `systemctl disable`.
7. **H7's cited origin "A/B" does not exist** — no DNS / captive-portal / real-network
   deferral appears anywhere in A or B (the only hit for "dns|captive" across the six P2
   specs is G's own row).

### Highest-value coverage finding (OMISSION rather than FALSE)

**Parent §7.2's "disable DPMS/screensaver in the cage session" — and §7's PRIMARY keep-awake
mechanism, "configuring cage/wlroots not to blank" — has no step in the runbook that owns
it.** G's three substitutes (`consoleblank=0`, `IdleAction=ignore`, sleep-target masking) are
respectively the VT console, the stock default and a no-op on the default, and suspend-only.
G's own H3 asserts the outcome ("display never blanks over 24 h") with nothing in the runbook
that would produce it. Runner-up omission: §7.2's "run on a dedicated seat with no other
TTYs" is demoted from a deployment-gate requirement to G's "one open fork".
