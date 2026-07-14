# P1-C — `kiosk-core`: Connectivity Prober + Navigation Allowlist

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the two remaining pure-`kiosk-core` subsystems — the navigation allowlist matcher (which decides what the kiosk is allowed to navigate to) and the connectivity prober state machine (which decides whether the kiosk believes it is online).

**Architecture:** Both live in `crates/kiosk-core/`, both are **pure**: no HTTP, no timers, no platform APIs. The allowlist is a function from (patterns, URL) to a decision. The prober is a state machine fed probe *outcomes* and asked for the next interval — the actual HTTP GET and the timer live in `kiosk-main` (P1-D). That is what makes both fully host-testable and lets the allowlist be attacked adversarially in unit tests, which is the only way to trust it.

**Tech Stack:** Rust stable; `urlpattern` (URLPattern semantics), `url` (already present), `serde`. No new runtime deps beyond `urlpattern`.

## Global Constraints

- Spec of record: `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` — **§3.3** (prober), **§3.6** (navigation guard), **§10** (the RT-03 adversarial test list), **§4** (layering). **On conflict, the spec wins over this plan.**
- **⚠️ This plan's sample code is a STARTING POINT, NOT GOSPEL.** Across P1-A and P1-B, *seven* genuine defects were bugs in the plan's own sample code and fixtures — including a `debug_assert` that vanished in release, a clock parser that silently accepted `EST` as a −5h offset, and two test fixtures that were concealing live secret leaks. **If the code here looks wrong, it may well be wrong. Say so and stop rather than implementing something you believe is broken.** Reviewers: a defect is a defect even when the plan mandated it.
- **Layering rule (spec §4): `kiosk-core` must NEVER depend on Tauri or any per-OS API.** No HTTP client in these modules — the prober takes outcomes, it does not fetch.
- **The allowlist is NOT an exfiltration boundary** (spec SEC-10). It governs *navigation*. Egress containment (subresource host-allowlist + CSP) is a separate boundary implemented in P1-D. Say this in the module docs so nobody mistakes one for the other.
- Rust stable, edition 2021. Lint gates before every commit: `cargo fmt --check`, `cargo clippy -p kiosk-core --all-targets -- -D warnings`, `cargo test -p kiosk-core`.
- **NEVER run `cargo test --workspace`** — it rebuilds Tauri and takes >8 minutes; an agent already hung on it and lost all its work. **Run cargo commands SERIALLY** — concurrent runs deadlock on the build-directory lock, and an agent once misread the resulting empty output files as success.
- **Never report a test or lint result you have not actually seen in real output.** Verify committed files from **`git ls-tree`**, not the working tree — the repo's own `*.pem` gitignore rule once silently excluded a fixture the code `include_str!`d, so a green local run proved nothing.
- Commit: conventional prefix + trailer `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`. Repo-local git identity is configured — just `git commit`; do NOT pass `-c user.email=...`. Stage explicit paths; never `git add -A`. Nothing under `.superpowers/` or `FROM_GH/`.

## A note on the failure mode this project keeps producing

Six times now, across P1-A and P1-B, the same shape: **a guarantee that holds right up until the crash it exists to survive** (a counter in RAM, a state file that resets to zero, an un-fsynced directory entry). Every fix was "persist it next to the fsynced state."

**That shape mostly does not apply here, and I want to say so explicitly so nobody chases it.** Both subsystems are deliberately ephemeral: the prober's damping counter *should* reset on restart (a fresh process re-probes from scratch; there is no prior state to flap from), and the allowlist is a pure function of config. **Nothing in P1-C needs to survive a crash.** If you find something that does, that is a real finding — but do not invent persistence where none is needed.

## What exists already (consume, do not reimplement)

- `kiosk_core::config::schema::{Content, Network}` — `content.url`, `content.allowlist: Vec<String>`, `content.scheme_allowlist: Vec<String>`, `network.connectivity_check_url`, `network.probe_online_s`, `network.probe_offline_s`.
- `kiosk_core::config::bootstrap::BootstrapConfig` — `bootstrap_url`.
- `kiosk_core::identity::expand_device_id_template` — the home URL is `content.url` **after** `{device_id}` expansion.
- `kiosk_core::logging::time::TrustedClock` — `observe_http_date(&str)`. **The prober is the subsystem that feeds it** (spec TEL-01): it makes an HTTP request every 10–30 s, and every response carries a mandatory `Date` header. A dead-CMOS kiosk on a 443-only LAN bootstraps its entire clock from this. **`observe_http_date` is a deliberately strict fail-closed IMF-fixdate gate — never parse a `Date` header yourself.**
- `kiosk_core::logging::event::Event` — `NetOnline`, `NetOffline`, `NavBlocked`, `NavError`, `ClockSkew` already exist in the taxonomy. **Emitting them is P1-D's job; this plan only produces the decisions.**

## File structure

| File | Responsibility |
|---|---|
| `crates/kiosk-core/src/nav/mod.rs` | module root; `Decision` |
| `crates/kiosk-core/src/nav/allowlist.rs` | URL allowlist matcher (URLPattern), home implicit-allow, origin-lock |
| `crates/kiosk-core/src/nav/scheme.rs` | external-URI-scheme guard (H2) + the `kiosk://` rule |
| `crates/kiosk-core/src/net/mod.rs` | module root |
| `crates/kiosk-core/src/net/reach.rs` | private-host heuristic + probe-URL resolution (arch-13) |
| `crates/kiosk-core/src/net/prober.rs` | the damped Online/Offline state machine + `Date` harvest |

---

### Task 1: Navigation allowlist matcher

**This is the security-critical task in this plan.** It is the thing standing between a locked kiosk and an attacker-chosen page. Its whole reason for living in `kiosk-core` (spec §3.6) is that a *pure* matcher can be attacked in unit tests; the platform layer only wires the navigation-intercept to it.

**Files:**
- Create: `crates/kiosk-core/src/nav/mod.rs`
- Create: `crates/kiosk-core/src/nav/allowlist.rs`
- Modify: `crates/kiosk-core/src/lib.rs` (add `pub mod nav;`)
- Modify: `crates/kiosk-core/Cargo.toml` (add `urlpattern`)

**Interfaces produced:**
- `nav::Decision` — `Allow` | `Block(BlockReason)`; `BlockReason` is an enum (`NotAllowlisted`, `Unparseable`, `SchemeNotAllowed`, `KioskSchemeFromRemote`) so P1-D can put a structured reason in the `nav.blocked` telemetry rather than a free-form string.
- `nav::allowlist::Allowlist::new(patterns: &[String], home_url: &str) -> Allowlist`
- `Allowlist::allows(&self, url: &str) -> Decision`

**The rules, from spec §3.6 — all of them are load-bearing:**

1. **URLPattern semantics** (the `urlpattern` crate), NOT glob matching. The spec is explicit that a raw glob is not URL-aware and is the source of `?`/`/`/`#` ambiguities. A pattern like `https://app.example.com/*` must match on parsed URL components, not on the raw string.
2. **Default-deny on parse failure.** A URL that does not parse is `Block(Unparseable)`. Never fall back to string matching.
3. **The home URL is always implicitly allowed** (cfg-02) — `content.url` after `{device_id}` expansion. The initial navigation must never be self-blocked by a mis-typed allowlist. The caller passes the already-expanded home URL.
4. **An empty/absent allowlist origin-locks to the home URL's origin** (arch-08) — scheme+host, all paths. It does **not** mean "allow everything". A signed `{}` config must stay origin-locked, not open the device.

**The adversarial battery (spec §10, RT-03) — every one of these must be a test:**

| Attack | Example | Must be |
|---|---|---|
| host-suffix | pattern `https://app.example.com/*`, URL `https://app.example.com.evil.com/` | **Block** |
| userinfo | URL `https://app.example.com@evil.com/` | **Block** (host is `evil.com`) |
| scheme downgrade | pattern `https://app.example.com/*`, URL `http://app.example.com/` | **Block** |
| path traversal | URL `https://app.example.com/../../etc/passwd` | must not escape the pattern's path constraint |
| embedded URL in query | URL `https://evil.com/?next=https://app.example.com/` | **Block** |
| IDN / punycode | pattern in unicode vs URL in punycode (and vice versa) for the **same** host must agree; a *different* Cyrillic-homoglyph host (e.g. `аpp.example.com` with a Cyrillic `а`) must **Block** | see below |
| parse failure | `":::"`, `""` | **Block(Unparseable)** |
| pattern crossing `?`/`#`/`/` | patterns and URLs with query and fragment | per URLPattern semantics |

**On IDN specifically — think, do not guess.** The `url` crate normalizes IDN hosts to punycode on parse. So a homoglyph host is a *different* punycode host and will not match — good. But confirm the direction that matters: a pattern written in unicode and a URL in punycode for the **same** real host must **match** (a false *block* here would brick a legitimate deployment), while a homoglyph must **not** (a false *allow* is a security hole). Test both. If `urlpattern` does not normalize the pattern the way `url` normalizes the URL, **normalize both yourself before matching and say so** — this is exactly the kind of seam where a plausible-looking implementation is silently wrong in one direction.

- [ ] **Step 1: Add the dependency**

```toml
urlpattern = "0.3"
```
(If the API differs from what this plan assumes, follow the crate's actual API and record the deviation.)

- [ ] **Step 2: Write the tests** — the full adversarial battery above, plus: the home URL is allowed even when the allowlist would block it; an empty allowlist allows the home origin's other paths but blocks a different origin; a populated allowlist does **not** implicitly allow the home *origin* (only the exact home URL plus whatever the patterns say).

- [ ] **Step 3: Run them — confirm RED.**

Run: `cargo test -p kiosk-core nav::allowlist`

- [ ] **Step 4: Implement.** Parse with `url::Url`, match with `urlpattern`, default-deny everywhere.

- [ ] **Step 5: Run — confirm GREEN. Then attack your own matcher.**

**This is not optional.** Take your passing matcher and try to defeat it with inputs *not* in the table above. Ideas: a trailing dot on the host (`app.example.com.`), uppercase host, a port (`https://app.example.com:8443/`), `https://app.example.com\@evil.com`, a URL with a backslash, an over-long host, `data:`/`blob:` URLs, a pattern of `*` alone. **Report what you tried and what happened.** Anything that gets through and should not is a Critical finding — report it, do not quietly patch the test.

- [ ] **Step 6: fmt, clippy (serially), commit.**

```bash
git commit -m "feat(core): URL allowlist matcher with URLPattern semantics

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: External-URI-scheme guard + the `kiosk://` rule

**Files:**
- Create: `crates/kiosk-core/src/nav/scheme.rs`
- Modify: `crates/kiosk-core/src/nav/mod.rs`

**Why this exists (spec §3.6, H2):** `mailto:`, `tel:`, `ms-settings:`, `ms-store:`, and any OS-registered custom scheme are **not ordinary navigations**. Cancelling a navigation does not stop them — the OS launches an external app. On a kiosk that is an escape hatch: `ms-settings:` opens Windows Settings *on top of the locked kiosk*. This is the pure decision function; P1-D wires it to WebView2's `LaunchingExternalUriScheme`, WebKitGTK's `decide-policy`, and Android's `shouldOverrideUrlLoading`.

**Interfaces produced:**
- `nav::scheme::scheme_decision(url: &str, scheme_allowlist: &[String], is_remote_origin: bool) -> Decision`

**Rules:**
1. `http` and `https` are ordinary navigations — this function returns `Allow` for them (the *allowlist* decides those; that is Task 1's job, not this one).
2. **Every other scheme is blocked by default.** `content.scheme_allowlist` defaults to **empty** — so `mailto:`, `tel:`, `ms-settings:`, `ms-store:`, `intent:`, `file:` are all `Block(SchemeNotAllowed)` unless explicitly listed.
3. **`kiosk://` from a remote origin is always blocked** (`Block(KioskSchemeFromRemote)`) and never allowable via `scheme_allowlist` — P1-A removed the `kiosk://` navigation-sentinel bridge precisely because any script in the page's world could fire it. Do not resurrect it: an operator must not be able to re-open that hole by adding `kiosk` to the allowlist. **Test that adding `"kiosk"` to `scheme_allowlist` does NOT allow it from a remote origin.**
4. Unparseable → `Block(Unparseable)`.

- [ ] TDD: tests → RED → implement → GREEN → fmt/clippy → commit.

Tests must include: each of `mailto:`/`tel:`/`ms-settings:`/`ms-store:`/`intent:`/`file:` blocked by default; a scheme explicitly allowlisted is allowed; `http`/`https` pass through; `kiosk://` from remote is blocked **even when allowlisted**; case-insensitivity of the scheme (`MAILTO:` must also block).

---

### Task 3: Reachability scope — private-host heuristic + probe-URL resolution

**Files:**
- Create: `crates/kiosk-core/src/net/mod.rs`
- Create: `crates/kiosk-core/src/net/reach.rs`
- Modify: `crates/kiosk-core/src/lib.rs` (add `pub mod net;`)

**Why (spec §3.3, arch-13):** the prober measures reachability of *the content URL's network path*, not "the internet". On an intranet or air-gapped deployment, the default public probe (`https://www.gstatic.com/generate_204`) reads **permanently offline** while the content host is perfectly reachable — so the kiosk would show the offline video forever, on a site that is up. That is a total-outage bug for exactly the deployments most likely to buy a kiosk.

**Interfaces produced:**
- `net::reach::is_private_host(host: &str) -> bool`
- `net::reach::resolve_probe_url(configured: &str, home_url: &str) -> String`

**Rules:**
- `is_private_host` is true for: RFC1918 IPv4 (`10/8`, `172.16/12`, `192.168/16`), loopback (`127/8`, `::1`, `localhost`), link-local (`169.254/16`, `fe80::/10`), IPv6 ULA (`fc00::/7`), a `.local` suffix, and a **single-label hostname** (no dot — e.g. `kiosk-server`, which can only resolve via an internal resolver).
- `resolve_probe_url`: if `configured` is still the **default public URL** (`https://www.gstatic.com/generate_204`) **and** `home_url`'s host `is_private_host`, then probe the **home URL's origin** instead. Otherwise use `configured` verbatim — an operator who set an explicit probe URL always wins.

**Note the subtlety, and get it right:** `connectivity_check_url` has a schema *default*, so it is never literally "unset". "Unset" therefore means "still equal to the default". Compare against the default constant. If you think that is the wrong reading of the spec, say so.

- [ ] TDD: tests → RED → implement → GREEN → fmt/clippy → commit.

Tests: each private range and form (including `172.15.x` and `172.32.x` being **public** — the `172.16/12` boundary is the classic off-by-one); a public host is not private; the default probe URL + a private home → probes the home origin; the default probe URL + a public home → keeps gstatic; an **explicit** probe URL + a private home → keeps the explicit one (the operator wins).

---

### Task 4: The damped connectivity prober

**Files:**
- Create: `crates/kiosk-core/src/net/prober.rs`
- Modify: `crates/kiosk-core/src/net/mod.rs`

**Interfaces produced:**
- `net::prober::ProbeOutcome { reachable: bool, date_header: Option<String> }`
- `net::prober::Link` — `Online` | `Offline`
- `net::prober::Prober::new(clock: TrustedClock) -> Prober`
- `Prober::record(&mut self, outcome: ProbeOutcome) -> Option<Link>` — returns `Some(new_state)` **only on a flip**, `None` otherwise. P1-D emits `net.online` / `net.offline` on a flip.
- `Prober::link(&self) -> Link`
- `Prober::interval(&self, net: &Network) -> Duration` — `probe_offline_s` (default 10 s) while offline, `probe_online_s` (default 30 s) while online.

**Rules:**

1. **Damping (spec §3.3):** two consecutive successes flip Online; two consecutive failures flip Offline. This exists so the kiosk does not flap between the site and the offline video on a marginal link — a flap is far more visible and objectionable to a viewer than a few extra seconds of either state.
2. **`record()` MUST feed the `Date` header to `TrustedClock::observe_http_date`** on every outcome that carries one — success **and** failure (spec TEL-01). This is the subsystem that bootstraps the clock on a dead-CMOS device. Call `observe_http_date`; **never parse the header yourself** (it is a deliberately strict fail-closed gate — a previous bug silently accepted `Date: … EST` as a real −5h offset, which would have poisoned every log timestamp and every JWT). A malformed `Date` must be non-fatal.

3. **The initial state — a deliberate deviation from a literal reading of the spec, which I want the reviewer to challenge.**

   The spec says "two consecutive successes flip online". Read literally from a cold start, a perfectly healthy kiosk shows the **offline video for two probe intervals (~20 s) on every boot** before it will admit it is online. That is a visible regression on every healthy device, and damping exists to prevent *flapping* — but on first boot there is no prior state to flap *from*.

   **So: start in `Link::Offline` but with an `Unknown` damping state, where the FIRST outcome decides immediately** (one success → Online; one failure → stays Offline). Damping (two-consecutive) applies to every transition **after** the first.

   Implement it that way, and **argue in your report whether you agree.** If you think the literal spec reading is right and the 20 s boot delay is intended, say so and implement that instead — but make it a decision, not an accident.

4. **The damping counter is in RAM and that is correct.** A fresh process re-probes from scratch; there is no prior state to flap from. Do **not** persist it. (This is called out because six bugs in this project have been "state that should have survived a crash but didn't" — this is the opposite case, and inventing persistence here would be wrong.)

- [ ] **Step 1: Write the tests.**

Cover: one success from cold → Online immediately (the deviation above); one failure from cold → stays Offline; from Online, a single failure does **not** flip (still Online — this is the damping property that matters most, and a naive implementation gets it wrong); two consecutive failures flip Offline; a success **resets** the failure run (fail, succeed, fail → still Online, because the run was broken); the same in the other direction; `interval()` returns the offline value while offline and the online value while online; a `Date` header on a **failed** probe still reaches the clock (assert `clock.offset_seconds().is_some()`); a malformed `Date` does not panic and does not flip anything.

**Verify the damping tests actually fail against a broken implementation** (e.g. one that flips on a single outcome). A test that passes against a no-damping limiter proves nothing — this plan has already produced several tests whose names claimed properties they never checked, and in two cases such a test was concealing a live secret leak.

- [ ] **Steps 2–5:** RED → implement → GREEN → fmt/clippy (serially) → commit.

---

## Plan self-review (run at write time)

**Spec coverage.** §3.6 allowlist (URLPattern, default-deny, home implicit-allow, empty→origin-lock) → T1. §3.6 external URI schemes (H2) + `kiosk://` rule → T2. §3.3 reachability scope (arch-13) → T3. §3.3 prober damping + intervals, TEL-01 `Date` harvest → T4. §10 RT-03 adversarial battery → T1.

**Deliberately out of scope, with the owner named:**
- **The app state machine** (`Boot → ConfigLoad → Online ⇄ Offline → ErrorPage`, `error_max_retries`) — spec §4 puts `state.rs` in **`kiosk-main`**, because it drives the webview. P1-D owns it; this plan gives it the two decisions it needs.
- **The actual HTTP GET and the timer** — P1-D. The prober is fed outcomes.
- **`nav.blocked` / `net.online` / `net.offline` emission** — P1-D calls `Logger::log`. `BlockReason` exists so that payload is structured.
- **`frame_allowlist` (M6)** and **egress containment / CSP (SEC-10)** — P1-D. The navigation allowlist is *not* an exfiltration boundary and must not be mistaken for one.
- **Script dialogs (M3), downloads, PDF policy** — webview hardening, P1-D.

**Known risks in this plan (flagged, not hidden):**
- **The `urlpattern` crate's API and its IDN/normalization behavior are assumed, not verified.** If it does not normalize hosts the way `url` does, the matcher can be wrong in the *permissive* direction — which is a security hole, not a bug. T1 Step 5 exists precisely to catch that, and the implementer is told to attack their own matcher.
- **The prober's initial-state deviation (T4 rule 3) is a genuine judgment call**, not a certainty. It is flagged for the reviewer rather than buried.

## Follow-on plans

1. **P1-D** — `kiosk-main` integration: the app state machine, offline video, webview hardening (folds in the P0 gate verdict: KEYBOARD=PARTIAL means Assigned Access / Shell Launcher is a hard deployment requirement; POINTER=PASS means the native corner-tap exit gesture is in), the `display.monitor` topology check `kiosk-core` cannot do, the panic hook, `health.sample` content, and the dedicated logger thread.
2. **P1-E** — `kiosk-launcher` watchdog: heartbeat, READY arming, liveness disambiguation, backoff, safe mode, orphaned-spool drain.
3. **P1-F** — packaging: WiX MSI (Authenticode-signed, credential ACL), §7.2 Windows OS-lockdown docs.
