# P2-B — Linux Webview Hardening + Subresource Egress + Keep-Awake (Design)

> Second sub-project of P2. Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §7 (hardening
> matrix, SEC-10 at `:700`), §3.6 H2. **Builds on P2-A** (nav guard, load lifecycle, origin
> constants — `2026-08-06-p2a-linux-bringup-design.md` rev 3) and mirrors the reviewed
> D2b `windows_impl` semantics in `crates/kiosk-main/src/{hardening,egress,scheme_guard}.rs`.
> Same doctrine as A: shipped Tauri/wry APIs first, raw webkit2gtk only where no shipped
> route exists, behavior parity with what Windows *actually enforces* — not with what P1
> descoped.

**Status:** rev 3, 2026-08-08 — owner amendment on top of rev 2 (adversarial design review;
see docs/superpowers/reviews/2026-08-07-p2b-p2g-adversarial-review/). Rev 3 adds **B13** (the
bundled on-screen keyboard) and **B14** (the native `print` signal), and replaces §M4's
"unowned and escalated" text with a recorded disposition that adds no code and no gate.
Authority for B13 and B14 is parent §7's errata block (rev 2.1, 2026-08-07, immediately above
§7.2); authority for the M4 disposition is the owner's ruling that the content origin is
operator-controlled. Everything else is rev 2 unchanged.

Closes the P2-A residual (`p2a:42`, "do not field a Linux device before P2-B") at
**(scheme, host, port)** granularity for subresources, with one declared looser divergence on
path/query. Pure additions (filter compiler, CSP derivation, permission classifier) are
host-tested; GTK/WebKit wiring extends the A smoke harness.

## Goal

The three Windows-only control groups get Linux bodies with honest parity: `hardening.rs`
(settings flags, script dialogs, permissions), `egress.rs` (SEC-10 subresource containment),
`scheme_guard.rs` (downloads), plus `display.keep_awake`. Rev 3 adds the two controls parent
§7's errata assign here, both on the P1 document-start engine's side of the line: the
**bundled on-screen keyboard** (B13, a new component section) and the native half of H1's
printing block (B14, one row in the hardening mapping). "Honest parity" still cuts both ways —
`hardening.rs`'s autofill row is a documented no-op and clipboard-read is unsatisfiable — and
for PDF it now cuts a third way: M4/OD-8 is **not a live control for this deployment** and B
adds no code for it (§M4).

## Scope

**In:** Linux bodies for `hardening.rs` and `egress.rs`; downloads deny in the builder;
keep-awake; the pure helpers each needs (allowlist→content-filter compiler in `kiosk-core`,
allowlist→CSP origin derivation, WebKit permission classifier); the bundled on-screen keyboard
(B13 — one more block in `inject::build_injection` plus its `keyboard.js` asset, Linux wiring
only); the `connect_print` suppression that completes H1 on Linux (B14); smoke scenarios 8–12.

**Out:** launcher/heartbeat/systemd → P2-C; idle/gesture → P2-D; video soak → P2-E;
CI automation of the harness → P2-F; OS image + logind config + WebKitGTK pin → P2-G.

**Documented divergences after B** (C3, both directions; each justified in its section):

- *Looser than Windows* — Layer 1 enforces the allowlist at **host+scheme+port** for
  subresources. An off-pattern *path* on an already-allowlisted host is permitted on Linux
  and blocked on Windows (`resource_allowed` → full URLPattern, `nav_policy.rs:131-137`).
  Navigations are unaffected: P2-A's nav guard runs the full URLPattern on every frame.
- *Stricter* — clipboard-read is unsatisfiable on Linux and always denied.
- *Stricter, over-block only* — userinfo forms, uppercase hosts and raw-string spelling
  variants under a wildcard-host pattern are blocked by Layer 1 where the allowlist allows
  them (enumerated in §egress; safe direction, availability cost only).
- *A capability Linux gains and Windows does not, declared rather than left implicit* — the
  bundled on-screen keyboard (B13). Windows carries the identical PF-02 gap
  (`grep -rniE 'tabtip|InputPane' crates/` → zero hits) and P2-B ships **Linux wiring only**;
  the Windows string is byte-identical to today (§B13, C8).
- *Deliberate hardening, tighter than the allowlist* — `object-src 'none'`, `base-uri 'none'`,
  `frame-ancestors 'none'` in the CSP belt. Not derivation output; a decision, declared here.
- *Residual, not a divergence* — the declared WebKitGTK feature minimum is a review
  convention, not a build-enforced floor (see §Feature/floor accounting).

## Architecture — routes

| Need | Route | Not this |
|---|---|---|
| Downloads deny | `WebviewWindowBuilder::on_download` (`tauri-2.11.5/src/webview/webview_window.rs:384`; `DownloadEvent::Requested` variant at `webview/mod.rs:77`) | a **hand-written** `download-started` handler — wry already installs one (`wry-0.55.1/src/webkitgtk/web_context.rs:317`) and `on_download` is its front door |
| Content filter (SEC-10 enforcement) | contained `unsafe` sys-FFI shim — the safe `add_filter`/`remove_filter` are commented out of webkit2gtk-rs 2.0.2 (`user_content_manager.rs:53,147`) because gir could not bind `WebKitUserContentFilter` (`src/auto/` has no `user_content_filter*.rs`; enabling `v2_24` would not un-comment them), but `webkit2gtk-sys` is complete (`lib.rs:5411` store new, `:5467` save, `:5477` save_finish, `:5511` add_filter); removal via the safe `remove_filter_by_id` (`user_content_manager.rs:154`, `v2_26`) | waiting for a bindings release; any cancel-capable request-level signal (none exists — see §egress) |
| Egress telemetry | `WebViewExt::connect_resource_load_started` (`web_view.rs:2523`, ungated) + `WebResource::connect_failed` (`web_resource.rs:118`) | `connect_local`/`connect_closure` on any signal (banned outright — see §egress) |
| CSP belt inject/swap | `UserContentManager::{add_script, remove_script}` (safe bindings, `user_content_manager.rs:58` ungated, `:166` `v2_32`) | `initialization_script` (single-caller contract, `nav_policy.rs:146-150`); `remove_all_scripts` (`:131`) |
| Settings/signals | safe webkit2gtk bindings: `set_enable_developer_extras` (`settings.rs:1475`), `set_zoom_level` (`web_view.rs:1980`), `set_zoom_text_only` (`settings.rs:1953`), `connect_context_menu` (`:2074`), `connect_permission_request` (`:2428`), `connect_script_dialog` (`:2649`, `v2_24`), `connect_print` (`:2461`, ungated) | anything sys-level |
| On-screen keyboard (B13) | the **existing P1 document-start engine**: one more block in `inject::build_injection` (`inject.rs:29-61`), reaching the webview through `main.rs`'s single `initialization_script` call (`main.rs:1041-1048`) | squeekboard / onboard (no layer-shell under cage on any version — §B13); RT-16's `inject_css`/`inject_js` (needs the live-reinjection path `inject.rs:12-18` says does not exist, and is deferred out of P2); a second `initialization_script` call (single-caller contract, `inject.rs:12-13`) |
| Keep-awake | `systemd-inhibit` child process (below) | `gtk::Application::inhibit` or a `zbus` dependency |

### Feature/floor accounting

```toml
[target.'cfg(target_os = "linux")'.dependencies]
webkit2gtk     = { version = "2.0.2", features = ["v2_32"] }
webkit2gtk-sys = { version = "2.0.2", features = ["v2_24"] }
```

`webkit2gtk` features are cumulative (`v2_32 = ["v2_30", "ffi/v2_32"]` chaining to
`v2_2 = []`, `webkit2gtk-2.0.2/Cargo.toml`), so `["v2_32"]` subsumes P2-A:63-64's proposed
`["v2_16"]`. The sys chain is `v2_24 = ["v2_22"]`
(`webkit2gtk-sys-2.0.2/Cargo.toml:123`). Highest webkit2gtk gate called is `remove_script`
(`user_content_manager.rs:166`, `v2_32`); highest sys gate is the filter-store extern set
(`webkit2gtk-sys-2.0.2/src/lib.rs:5405-5478`, `v2_24`). No `v2_40`-gated symbol is called and
no `ResponsePolicyDecision` is reintroduced — **P2-A:71-74's hand-forward is re-derived and
discharged.**

A direct `webkit2gtk-sys` dependency declares `features = ["v2_24"]`, **not** `ffi/v2_24`:
`ffi/…` is the `webkit2gtk` crate's alias for *its* sys dependency's feature.

**This is a declared minimum, not a build-enforced floor.** tauri 2.11.5 declares
`webkit2gtk` with `features = ["v2_40"]` in its linux target block
(`tauri-2.11.5/Cargo.toml:339-341`), and `tauri-runtime-wry-2.11.4` additionally enables wry's
non-default `linux-body` feature (`webkit2gtk/v2_40`); Cargo unifies features across the
graph, so the compiled crate carries `v2_40` whatever we declare. Nothing in the build stops a
future edit from calling a `v2_40`-gated symbol; it would compile and would break only on a
distro below 2.40. **Enforcement is code review against this line.** Runtime risk today is nil
(Debian 12 ships ≥ 2.40, C7). *Rejected, recorded:* a `cargo tree -e features -i webkit2gtk`
CI check — it reports, it does not enforce; it would print `v2_40` on a green build and fail
nothing.

*Correction to this spec's own draft claim:* wry's own dependency is `webkit2gtk` with
`features = ["v2_38"]` (`wry-0.55.1/Cargo.toml:226-228`), not `v2_40`. The earlier attribution
was wrong; the conclusion ("declare what our code calls, do not lean on a
dependency-of-a-dependency's feature choice") is what survives, and the corrected evidence is
what makes it a real reason rather than a rhetorical one.

The `webkit2gtk` declaration is **shared** with P2-D's D10 and P2-C's C17, which need no
feature above `["v2_32"]`. Whichever sub-project lands first writes the line; the second
reconciles by union. **No ordering edge between B, C and D exists or is declared.**

## Components

### `egress.rs` — SEC-10, two layers

Windows subscribes `WebResourceRequested` (all contexts, `egress.rs:88`) and 403s anything
`NavPolicy::resource_allowed` denies (`egress.rs:104`, `:122-127`; module doc `:1-14`).
WebKitGTK has **no cancel-capable request-level API** reachable from this process, so Linux
splits enforcement from observation.

**`WebKitWebResource` exposes no cancel-capable signal.** `sent-request`
(`web_resource.rs:233`) is a past-tense, void-return notification — it cannot cancel and must
not be connected in an attempt to. The gboolean-returning `send-request` is a
`WebKitWebPage` (web-process-extension) signal; `webkit2gtk-2.0.2` has no `web_page.rs` and
grep for `send-request` over `webkit2gtk-2.0.2` and `webkit2gtk-sys-2.0.2` returns zero hits.
The gir-generated bindings enumerate each type's complete signal set (control:
`download.rs` binds `WebKitDownload`'s full set), so "unbound" here means "absent", not
"reachable by name".

**Blanket rule for P2-B: no dynamic signal connection anywhere.** No
`glib::ObjectExt::connect_local`, no `connect_closure`. Every signal P2-B uses is a typed,
generated binding. This is checkable by grep, which a runtime existence probe is not, and it
closes a one-character trap: `sent-request` exists, has a `UNIT` return, and a mis-typed
dynamic connection panics *inside signal emission* (`glib-0.18.5/src/object.rs:~2580,2590,2605,2613`)
— under the launcher that is a crash-restart loop, not a degraded control.

*Rejected, recorded:* enforcing via `resource-load-started` + a `send-request` cancel. It was
the parent's named mechanism and it is the right traceability instinct, but the cancel half
does not exist (above), so the design would have degraded to observe-only on **every** device
— a fail-open SEC-10 gate under C5. *Also rejected:* a web-process extension exposing
`WebKitWebPage::send-request` — the only route to a full-fidelity cancel, but it is a new
cdylib plus an IPC hop for the live allowlist, and wry already claims the extensions directory
(`wry-0.55.1/src/webkitgtk/mod.rs:283`) under the `linux-body` feature tauri-runtime-wry
enables; that is a sub-project, not a section. *Also rejected:* an in-process allowlisting
proxy via `WebsiteDataManager::set_network_proxy_settings` (`website_data_manager.rs:589`) —
for HTTPS a proxy sees only the CONNECT target, i.e. **the same host granularity the content
filter already gives**, for an entire new HTTP/CONNECT component (Q2).

---

**Layer 1 — WebKit content filter (the enforcement authority).**

`compile_filter` lives in **`kiosk-core::nav`**, next to `allowlist.rs`. It is pure decision
logic, which is where the layering rule (C1) puts it; `kiosk-main` keeps only the sys-FFI shim
that hands the emitted JSON to the `UserContentManager`, which is the observation/enforcement
edge. C1 is therefore *satisfied* for the compiler rather than strained; it remains strained
only by the existence of a second matcher at all, which is what the soundness test below
exists to hold in check.

The emitted rule set is a **single block rule with `url-filter` `^(https?|wss?)://`**,
followed by `ignore-previous-rules` entries for the allowed set. The narrowed block rule is
load-bearing in three ways:

- **Custom-scheme origins are dissolved, not pinned.** `tauri://localhost`,
  `kioskasset://localhost` and `ipc://localhost` (P2-A:96-110) never match an anchored
  `^(https?|wss?)://`, so the splash, error page, `offline.html`, `safe.html` and the offline
  mp4 are untouched **whether or not WebKit's content-rule engine applies to custom schemes** —
  an unknown no tier 1–4 artifact settles. An ignore-rule bucket would have depended on the
  same unknown it was meant to cover, and this is the one defect class that fires on first
  boot (Q4).
- **Hostless subresources are not blocked.** `data:`, `blob:`, `about:` never match, so there
  is no stricter-than-Windows divergence to declare. This mirrors `resource_allowed`'s rule 1
  by construction: it returns `true` for `!is_remote_origin(url)` (`nav_policy.rs:131-134`),
  and `is_remote_origin` returns `false` for custom-scheme hosts and every hostless URL
  (`nav_policy.rs:233-243`, `None => false`, as amended by P2-A:96-110).
- **`ws`/`wss` are covered** because `resource_allowed` polices them scheme-included
  (`nav_policy.rs:120-130`). Whether the content blocker sees WebSocket handshakes at all is
  unknown at tier 1–4; including them costs nothing and closes the case if it does. Declared
  as a residual, not asserted.

*Rejected, recorded:* the `.*` block-all rule with three `ignore-previous-rules` origin
buckets (app origin, asset origin, content origin). The first two are unnecessary under the
narrowed rule; the third was wrong on parity grounds independently — a populated allowlist does
**not** implicitly admit the home origin's other paths
(`allowlist.rs:387-397`, `a_populated_allowlist_does_not_implicitly_allow_the_home_origin`:
"*Only the exact home URL is implicit — not its whole origin*"), so a blanket content-origin
allowance would have been *looser* than Windows.

The compiler therefore emits rules for exactly what `Allowlist::allows` implements: rule 4
(the configured patterns), rule 2 (the exact home URL, widened to its origin — inside the
declared host-granularity divergence, stated here rather than allowed to pass as a new one),
and rule 3 (the origin lock when the configured list is empty).

**Accepted pattern shapes, and what is emitted:**

| Component | Accepted | Emitted |
|---|---|---|
| scheme | literal ∈ {`http`,`https`,`ws`,`wss`} | literal |
| host | literal, **or** leading `*.` + literal suffix | `regex::escape(host)` / `[a-z0-9-]+(\.[a-z0-9-]+)*\.` + escaped suffix |
| port | explicit in the pattern, else the scheme default | **exact**: `(:443)?` / `(:80)?` / `:8443` — never `[0-9]+` |
| path / query | — | not compiled (the declared divergence) |

Anything else — mid-label wildcard (`api-*.example.com`), wildcard scheme (`*://`), named or
regex group in the host (`:sub.example.com`, all of which *compile* as live allowlist entries)
— **emits no rule** and raises `config.warn("egress.filter_pattern", pattern)`. No rule ⇒
blocked ⇒ the safe direction. Mid-label wildcards are refused rather than guessed: URLPattern's
mid-label `*` semantics are not derivable from tier 1–4, and the codebase already records the
widest shape as an operator footgun config validation should reject
(`allowlist.rs:703-718`). `ponytail:` accept mid-label wildcards later, once the implication
test below covers them.

*Rejected, recorded:* a permissive wildcard class (`[^/]*`) and a wildcarded port
(`(:[0-9]+)?`). Both were false-allows, reproduced against the battery: `(:[0-9]+)?` admits
`https://app.example.com:8443/` where URLPattern blocks it — an absent port in a pattern means
the scheme's **default** port, pinned deliberately by `allowlist.rs`'s
`the_port_is_pinned_not_wildcarded` — and a permissive class under `https://*.example.com/*`
admits `https://evil.com\@x.example.com/steal?d=secret`, which parses to host `evil.com` with
the payload in the query. That is the exact threat SEC-10 exists for.

**Soundness, stated precisely.** Let `H(u) = (scheme, host, port)`. For every URL `u` the
content blocker matches:

> **regex matches `u` ⇒ ∃ an allowlist pattern `p` accepted by the table above (or the home
> URL), with `H(u) ∈ H(AllowSet(p))`.**

It is an implication in one direction only — over-blocking is permitted and enumerated below;
it is over `H(u)` only, not full URLs (the declared path/query divergence); and it holds only
for accepted patterns, refusal being the safe direction for the rest. *Rejected, recorded:* the
stronger `AllowSet(regex) ⊆ AllowSet(URLPattern)`, which is false for this compiler by
construction.

**The claim is carried by a test, not by this prose.** The corpus implication test lives
**inside `allowlist.rs`'s own `#[cfg(test)] mod tests`** (`allowlist.rs:144-145`), which is
where the adversarial battery lives and is not compiled into the rlib `kiosk-main` links — a
test in `kiosk-main` could not read the corpus, and a hand-copied corpus would be a second
source of truth for the battery. Colocating it means a new battery row reaches the implication
test on the day it is added. The assertion is in **`H(u)` terms**: for every corpus URL the
compiled regex matches, some accepted pattern (or the home URL) has the same
`(scheme, host, port)`. It is **not** `re.is_match(u) ⇒ allow.allows(u)` — that is the
withdrawn full-URL claim and it cannot pass, falsified by the path divergence itself
(`allowlist.rs:641`: pattern `https://app.example.com/kiosk/*`, URL
`https://app.example.com/kiosk-admin/x` — allowlist blocks, host-granularity filter matches)
and by the home-origin widening (`:387-397`). The corpus is https-only today; **add one
`http://` row and one `ws://`/`wss://` row** so the non-https arms the compiler and the block
rule both accept are actually exercised. `regex` is a `[dev-dependencies]` entry on
`kiosk-core` only, used solely to evaluate the emitted pattern in the test — WebKit compiles
the JSON at runtime, we never match with it — and `regex 1.12.4` is already in `Cargo.lock`,
so no crate joins the graph or the shipped binary (C6).

**Enumerated over-blocks (safe direction, availability cost, declared under C3):** userinfo
(`https://evil.com@x.example.com/` — URLPattern allows, filter blocks), uppercase host
(`https://X.EXAMPLE.COM/x`), and the raw-string spelling variants the WHATWG parser folds —
backslash-`@`, tab, `%2e`, U+3002, punycode↔unicode pairs (`allowlist.rs:286-301,497-564,517-528`).
Every normalisation divergence lands here rather than in the allow direction. Whether WebKit
matches `url-filter` against the raw markup URL or the canonicalised request URL is unknown at
tier 1–4 and **does not matter**: on the raw string these over-block; on the canonical string
they do not arise, because the canonicaliser has already folded them. Both answers sit on the
safe side of the implication.

**Install and swap.** Store at `data_dir/content-filters/`; async `save` → `add_filter` on the
webview's existing `UserContentManager` through the sys shim. On every `ConfigApplied`, compile
under a fresh id, `add_filter`, then `remove_filter_by_id` the previous — never a gap with no
filter while a page is live.

**Cost, stated rather than glossed:** a contained `unsafe` sys-FFI shim, a direct
`webkit2gtk-sys` dependency, an async save callback, and filter-id lifecycle across
`ConfigApplied`. The C6 justification is that every alternative above is larger and that the
do-nothing option fails C5.

**Pinned residual.** `http://tauri.localhost`, `http://kioskasset.localhost` and
`http://ipc.localhost` **do** match the narrowed block rule, and the app-origin bucket is
deleted. On Linux the app origin is the custom-scheme form — `main.rs:46-53` marks the
`http://` form as the *Windows* WebView2 workaround and P2-A:96-110 resolves Linux to
`tauri://localhost` — so this should never fire, but P2-A deliberately keeps both forms in
`is_remote_origin` with no `cfg`, so the two documents are one refactor apart from disagreeing.
Carried by A's smoke scenarios 3 and 7, which fail loudly if it fires. `ponytail:` a single
ignore rule `^https?://(tauri|kioskasset|ipc)\.localhost/` would close it for one line and no
unknown; recorded as a suggestion, not a condition.

---

**Layer 1's observe-only companion — `resource-load-started`.**

`connect_resource_load_started` (`web_view.rs:2523`, ungated) fires per resource with
`&WebResource, &URIRequest`. Connect `WebResource::connect_failed` (`web_resource.rs:118`) and,
when `!resource_allowed(uri)`, emit `telem.nav_blocked(REASON_EGRESS, uri)` — the same label
Windows emits (`egress.rs:22`, made `pub(crate)`), the same per-request granularity, the same
`nav.blocked` rate bucket (`egress.rs:112-118`'s explicit no-second-limiter doctrine; 20/burst
pinned at `crates/kiosk-core/src/logging/ratelimit.rs:182`).

**MEASURED (WebKitGTK 2.52.3, weston headless, 2026-08-27) — it does not.** Both callbacks were
instrumented and scenario 8 run in both configurations: with the native filter active, the four
off-list URLs never reach `resource-load-started` at all; with the filter absent, all four reach
it, fail, and emit `nav.blocked{egress}`. The signal and the enforcement are therefore mutually
exclusive. **Host-scoped blocks are enforced but silent on Linux: zero `nav.blocked{egress}` is
the healthy reading.** Scenario 8's healthy arm now asserts enforcement from outside the process
(an off-allowlist but *served* URL that must never appear in the fixture access log) and asserts
the observer only in the degraded arm. A second residual was found in the same run: the CSP belt
does **not** block off-list egress when the filter is unavailable — see `packaging/smoke/README.md`,
"Egress: three measured residuals". The original open question is retained below for context.

**Whether a content-blocked load reaches this signal at all is runtime and is pinned by smoke
scenario 8(b), not asserted.** If it does not, host-scoped blocks are enforced but *silent* on
Linux — a declared divergence, recorded before merge rather than discovered in the field.

*Reconciliation with P2-A, written down rather than left silent:* P2-A:227-232 records that
`load-changed`/`load-failed` are `WebKitWebView`-level signals tracking the **main frame's**
load only. `resource-load-started` is a different signal with a different subject — its
parameters are a resource and a request, not a frame load — so nothing about A's assumption
transfers either way, and P2-B does not weaken it. Blast radius is bounded by the role: this
signal is observe-only, so if it turns out to be main-frame-scoped the cost is missing
telemetry, not missing enforcement. Enforcement is the filter.

---

**Layer 2 — CSP belt.**

Kept on Q1: parent §7:700 names "plus an injected restrictive CSP" in the same clause as the
enforcement mechanism, so it is a named requirement. Stated honestly: with Layer 1 healthy the
belt restricts almost nothing Layer 1 does not. It earns its place in exactly one state —
Layer 1 absent — and that state is now gated and asserted (scenario 8(d)).

**The "looser by construction" property is withdrawn.** It was false in both halves. First,
the URLPattern component accessors return the component's `pattern_string`
(`urlpattern-0.3.0/src/lib.rs:431,446,451`), not a hostname, and several live allowlist entries
yield source expressions that are not valid CSP — `api-*.example.com` (a mid-label wildcard is
a CSP `host-part` parse error), `*://example.com` (`*` is not a scheme), `https://:sub.example.com`
(parses as scheme + empty host + port). An unparseable source expression is *ignored* by the
CSP parser while the rest of the list still applies, so the host would be absent from the belt
and present in the allowlist — the belt **tighter** than the authority, silently. That is
verbatim the bug D2b refused to ship (`nav_policy.rs:169-184`: "*would make a legitimately-
allowlisted subresource pass the native filter and then get silently blocked by this CSP*").
Second, CSP constrains dimensions an origin allowlist does not: a `default-src` with no
`'unsafe-inline'` (P1's reference policy at `nav_policy.rs:186-197` gives it to `style-src`
only, `:190`) blocks every inline `<script>`, every inline handler and every `eval` on an
*allowlisted* page, none of which `resource_allowed` restricts at all.

The derivation therefore returns `Option<String>`, and the property is carried by one branch:

1. **Expressibility gate.** If **any** allowlist pattern is not CSP-expressible — mid-label
   host wildcard, non-literal scheme, named or regex group in the host, non-numeric port —
   return `None`: **inject no CSP at all**, plus `config.warn("egress.csp_skipped", pattern)`
   naming the offender and `config.error("egress.csp_absent")` for the missing policy. Absent
   ⇒ blocks nothing ⇒ trivially never tighter. One branch, loud, no partial policies. This is
   deliberately the same three refused shapes Layer 1's compiler refuses, so the two layers
   agree on what they cannot express.
2. **Origin sources** otherwise: allowlist origins ∪ {content origin, app origin, asset origin}
   ∪ `data:` ∪ `blob:`. The `data:`/`blob:` sources are not decoration — P1's `csp_policy`
   carries `img-src … data:` and `font-src … data:` (`nav_policy.rs:189,191`) precisely to keep
   bundled assets working, and dropping them would be the same silent breakage in a new
   direction. Needs one new accessor in `kiosk-core`, next to the matcher it must agree with:
   `Allowlist::origins() -> Vec<String>`, built from the compiled patterns' own components,
   host-tested beside the battery so the belt cannot drift tighter than the matcher.
3. **Non-origin dimensions opened**, because the authority does not restrict them:
   `'unsafe-inline'` and `'unsafe-eval'` on `script-src`/`style-src`.
4. **Three restrictions kept and reclassified:** `object-src 'none'; base-uri 'none';
   frame-ancestors 'none'`. These *are* tighter than the allowlist. They are not derivation
   output — they are a deliberate hardening decision, declared in the divergence list. Not
   silent, which is the whole of D2b's complaint.

**Injection.** A document-start `UserScript` that appends the `<meta http-equiv>` to
`document.head`, creating `head` if absent and re-checking on `readystatechange`. Swapped on
`ConfigApplied` via a kept `UserScript` handle — `remove_script(&old)`
(`user_content_manager.rs:166`, `v2_32`), `add_script(&new)` (`:58`, ungated); **never**
`remove_all_scripts` (`:131`), which would destroy wry's own injected bootstrap script living
in the same manager (`wry-0.55.1/src/webkitgtk/mod.rs:721-738`; the IPC *handler* is a
`register_script_message_handler` at `:655` and would survive, but the bootstrap JS would not).

Which idiom actually takes effect is **settled by blocking scenario 8(d), not by argument** —
per Q5, whether the mechanism works at all is not a plan-time item.

*Rejected, recorded:* the in-product `securitypolicyviolation` listener and its
`#[cfg(not(windows))]` Tauri command. Their only value was observability that the
`resource-load-started` companion now provides, and 8(d)'s assertion lives in the **fixture**
instead. Consequence: P2-B touches neither `main.rs:990`'s `tauri::generate_handler!` nor
`capabilities/default.json`, and declares no edge onto the siblings that do.

---

**Coverage honesty.** Off-origin egress — the exfil case — is blocked by Layer 1 at
`(scheme, host, port)` and, when the belt is present, by the belt as well. Path-scoped blocks
are **not** enforced on Linux; that is the declared looser divergence, and it is
security-inconsequential rather than merely convenient: with the full-fidelity matcher Windows
uses, pattern `https://cdn.example.com/assets/*` allows
`https://cdn.example.com/assets/x?d=SECRET` and `https://cdn.example.com/assets/SECRET.gif`,
so a page that may reach an in-pattern path may already exfiltrate in that path's query string
or filename. What Linux loses is least-privilege hygiene, not the exfiltration boundary — and
the operator intent behind a path-scoped entry survives where it is enforceable, because
P2-A's nav guard runs the full URLPattern on every frame's navigations. CSP's own structural
gaps — pre-installed service workers, preload timing (`nav_policy.rs:152-167`) — are exactly
the gaps a network-layer filter does not have, which is why Layer 1 is the authority; whether
WebKit applies content rules to service-worker-initiated fetches is **pinned by smoke**
(scenario 8(c)), not assumed. Windows is untouched: the native filter remains its sole
boundary and the belt is **not** injected there.

### `hardening.rs` — control mapping, 1:1 where WebKit has the concept

| Windows (`windows_impl`) | Linux |
|---|---|
| `SetZoomFactor` (controller) | `WebViewExt::set_zoom_level` (`web_view.rs:1980`) plus an explicit `set_zoom_text_only(false)` (`settings.rs:1953`, ungated) for full-content zoom parity — a one-line setter, decided here rather than deferred |
| `SetAreDefaultContextMenusEnabled(false)` | `connect_context_menu` → return `true` (suppress; `web_view.rs:2074`) |
| devtools off | `set_enable_developer_extras(false)` explicitly (`settings.rs:1475`); wry only enables it when `attributes.devtools` is set (`wry-0.55.1/src/webkitgtk/mod.rs:443-446`), whose default is `true` under `debug_assertions` and `false` otherwise (`wry-0.55.1/src/lib.rs:834-837`), and tauri-runtime-wry calls `with_devtools(…unwrap_or(true))` under `#[cfg(any(debug_assertions, feature = "devtools"))]` (`tauri-runtime-wry-2.11.4/src/lib.rs:5209-5211`). The explicit set is belt against a feature-flag mistake |
| autofill/password-save off | documented **no-op** — WebKitGTK ships no password manager/autofill store and `settings.rs` exposes no such setter. An honest no-op beats an invented setting |
| script dialogs: none ever paints; `beforeunload` auto-leave (`hardening.rs:259-272`) | `connect_script_dialog` → return `true` always (`web_view.rs:2649`, `v2_24`; no dialog chrome exists to paint); `ScriptDialogType::BeforeUnloadConfirm` (`enums.rs:3297`) → `confirm_set_confirmed(true)` (`script_dialog.rs:28`), leaving the page, matching Windows |
| printing off (H1): the document-start `window.print` override (`inject.rs:46-48`, pinned by `inject.rs:75-77`) is the whole of it — WebView2 exposes no API to disable printing (parent §7, WebView2Feedback #3545) — plus Ctrl+P in the §7.2 swallow list | **B14.** `connect_print` → return `true` (`web_view.rs:2461`, `Fn(&Self, &PrintOperation) -> bool`, **ungated** — no `#[cfg(feature = …)]`, so B10's declared floor is unchanged). Same shape as the `context-menu` and `script-dialog` rows above. Parent §7 removes *both* entry points on purpose and either alone is insufficient: the JS override is page-world, the native signal catches whatever reaches the engine without going through `window.print`. Ctrl+P is the compositor's under cage, not the app's (§7.2) |
| `PermissionRequested` → `classify_permission_kind(i32)` → `permission_allowed` (`hardening.rs:72`, `nav_policy.rs:219-228`) | `connect_permission_request` (`web_view.rs:2428`) → classify by the request's **runtime type** (WebKit subtypes are GObject classes, not an enum): `GeolocationPermissionRequest` → `Geolocation`; `NotificationPermissionRequest` → `Notifications`; `UserMediaPermissionRequest` → `classify_user_media` (below); everything else (`DeviceInfo`, `MediaKeySystem`, `PointerLock`, `WebsiteDataAccess`, unknown) → `Other` → deny. `request.allow()`/`deny()` (`permission_request.rs:27,34`) + return `true` |

*Rejected, recorded:* mirroring Windows' script-dialog budget. `SCRIPT_DIALOG_BUDGET`
(`hardening.rs:102`) is an explicit no-op — `hardening.rs:283-295` carries its own
`ponytail:` saying so, and `:295`'s `let _over_budget` is never read. Mirroring it ports dead
code, and the divergence is nil because Windows' counter has no observable effect either.
Linux suppresses unconditionally.

*Rejected, recorded:* naming `InstallMissingMediaPluginsPermissionRequest` in the downcast
list. It carries `deprecated = "Since 2.40"` (`auto/mod.rs:83-89`), `v2_40` is present in this
build, and CI runs `cargo clippy --workspace --all-targets -- -D warnings`
(`.github/workflows/ci.yml:24`), so naming it is a hard CI failure. It needs no arm: the
`_ => Other` catch-all already denies it, which is the required behaviour. Recorded here so
nobody helpfully adds it back.

**`classify_user_media` — the one classifier in B that decides a security default**, so its
arms are exhaustive by construction and host-tested individually. `user_media_permission_request.rs`
exposes exactly `is_for_audio_device` (`:37`) and `is_for_video_device` (`:44`) and no
display-capture predicate, so `(false, false)` is reachable from the bindings alone, downcasts
to `UserMediaPermissionRequest` successfully, and never reaches the `_ => Other` catch-all:

```rust
fn classify_user_media(audio: bool, video: bool) -> Verdict {
    match (audio, video) {
        (false, false) => Verdict::Deny,                          // display/screen capture, unknown
        (true,  false) => Verdict::Kind(PermissionKind::Microphone),
        (false, true ) => Verdict::Kind(PermissionKind::Camera),
        (true,  true ) => Verdict::Both,   // require camera AND microphone
    }
}
```

`(false,false)` denies unconditionally — it is not `Camera`, and a kiosk with `camera=true` for
a video-call page must not thereby grant screen capture. `(true,true)` requires **both**
`permissions.camera` and `permissions.microphone`, because `PermissionKind` is one-of and
silently picking either is a fail-open; this is outcome-equivalent to Windows rather than a
divergence, since WebView2 raises `CAMERA` and `MICROPHONE` as separate `PermissionRequested`
events (`hardening.rs:78-83`), each checked separately. The GObject downcasts stay confined to
the signal handler; the mapping is a pure `fn`, host-tested like `classify_permission_kind`.

Same live `SharedNavPolicy`, same default-deny, same telemetry shape: `config.warn` on a failed
apply (`hardening.rs:191,216`), no per-denial event (none exists on Windows either —
`hardening.rs:307-321`). **Divergence (stricter):** webkit2gtk-rs 2.0.2 has no clipboard
permission request type at all — the nine `*permission_request.rs` files are the complete set
and none is clipboard — so `Permissions::clipboard_read` (`crates/kiosk-core/src/config/schema.rs:89`)
is unsatisfiable on Linux and clipboard read is always denied. Documented here and in a **new**
doc comment on `schema.rs:89` (the field has none today; adding it is a change this spec makes).
`ponytail:` revisit only on a bindings/floor bump.

**Declared assumption, pinned:** that returning `true` from `connect_context_menu` /
`connect_script_dialog` / `connect_permission_request` / `connect_print` suppresses the default
handler, and that `confirm_set_confirmed(true)` means "leave the page". The bindings give
signatures only; no tier 1–4 artifact settles the semantics. Pinned by smoke scenarios 10 and
11, both blocking: every one of these fails *visibly* (chrome paints, a prompt appears, a print
dialog appears, a flipped permission does not take), not silently in the field.

**Honest limit on B14's gate.** With the JS override in place a page cannot call `window.print`
at all (`Object.defineProperty(…, writable:false, configurable:false)`, `inject.rs:46-48`), so
from the main document the native handler is *unfalsifiable* — an assertion there would pass
whether or not `connect_print` is wired, which is not a test. Scenario 10's print arm therefore
drives it from a page-created `about:blank` iframe, where whether the injection reaches the
child frame at all is itself unknown at tier 1–4; the assertion is on the outcome ("no print
dialog paints"), which fails if *neither* entry point covers it. That is exactly the
belt-and-braces the parent's "both entry points are removed" wording asks for, and it is why
B14 ships despite being cheap to mistake for redundant.

### On-screen keyboard — bundled, injected document-start (B13)

Parent §7's touch-keyboard row, Linux cell (erratum rev 2.1), assigns this to P2-B. Two facts
pick the route; neither is a preference.

**squeekboard and onboard cannot display themselves under cage.** cage exposes **no
layer-shell protocol on any version** — verified on 0.1.4, the C7 floor, whose complete
`*_create(` surface is `cage.c:297-455` (no `zwlr_layer_shell_v1`, no `zwp_input_method_v2`,
no `zwp_virtual_keyboard_v1`, no `zwp_text_input_v3`), and on 0.1.5, where
`strings /usr/bin/cage | grep -iE 'layer_shell|input_method|text_input'` returns rc=1. An OSK
is by construction an overlay over a fullscreen client, and without layer-shell it has nowhere
to put itself. **Input *injection* is not the missing half:**
`wlr_virtual_keyboard_manager_v1_create` **is** present on cage 0.1.5 (absent on 0.1.4) — that
is the `zwp_virtual_keyboard_manager_v1` global. Display is what is impossible, on both
versions. *Recorded because it was wrong once and will be re-derived otherwise:* the claim that
any separate-process OSK would break P2-D's per-process `ActivityClock` holds for the
XTEST/onboard-under-Xwayland route only and is **false** for a `zwp_virtual_keyboard_v1`
client, which injects at the seat so the compositor delivers real `wl_keyboard` (hence real
GDK) events to the focused client. It must not be reused as an argument.

**The engine already ships, and it is the one P1 built for exactly this shape.**
`inject::build_injection` (`inject.rs:29-61`) is pure and host-tested — it assembles a `String`
of JS and touches no webview — and its one caller is `main.rs`'s `.setup()`, which passes the
result to `WebviewWindowBuilder::initialization_script` (`main.rs:1041-1048`), run before any
page script on every navigation. Controls that must survive every navigation **without
re-injection** already live there: the cursor-autohide timer (`inject.rs:50-57`). An always-on
keyboard is that same shape.

**This is why B13 does not wait on RT-16, and it is the whole reason the two were separable.**
`initialization_script` may be called only once per webview and is set at build time from the
just-booted config — *"there is no live-reinjection path, by design"* (`inject.rs:12-18`). An
operator-supplied `inject_js` needs precisely that path, which is why it is `UNIMPLEMENTED`
(`validate.rs:16-17`) and why the owner has **deferred RT-16 out of P2**. A bundled, always-on
keyboard needs no reinjection at all, so it is the strictly easier half of the same file and
carries none of RT-16's dependency.

`pinpad.html` is the in-repo precedent for an app-owned key grid: a `<button>` grid writing
into a `<div>` (`pinpad.html:43-56,73-78`), no text field anywhere.

**Who this is for, verified.** `grep -nE '<input|<textarea|contenteditable'
crates/kiosk-main/bundled/*.html` → **zero hits across all five pages** (`error.html`,
`offline.html`, `pinpad.html`, `safe.html`, `splash.html`). No app-owned surface in this
product has a text input; the one input path the kiosk owns ships its own keys. B13 therefore
exists for **deployed sites**, not for app-owned surfaces — nothing P2-G installs is broken
today by its absence, and nothing in this repo regresses if it misbehaves.

**What ships.** One `keyboard.js` asset, `include_str!`d by `inject.rs` and appended by
`build_injection` as one more block. It is kept **out of `bundled/`**: that directory is the
served frontend dist (`tauri.conf.json:6` `"frontendDist": "./bundled"`, reachable as
`APP_ORIGIN/<page>` via `bundled_url`, `main.rs:59-61`), and the keyboard is injected code, not
a navigable page — shipping it there would expose a second, pointless surface.

**Platform gating keeps `build_injection` pure.** The block is selected by a **third parameter**
(`on_screen_keyboard: bool`), not by `cfg!` inside the function, so both arms stay host-testable
on the one job that runs tests (ubuntu, `ci.yml:25`); `main.rs` passes
`cfg!(target_os = "linux")` at the single call site (`main.rs:1046-1048`). With the flag
`false` the emitted string is byte-identical to today's, which is a host assertion, not a claim
— **C8 (Windows stays green) holds by test.**

**Shape, and why it needs nothing from either CSP layer.** The block is appended **last** and
wrapped in its own `try{…}catch(e){}` IIFE, so a defect in it cannot prevent the blocks before
it (selection, drag/drop, print override, autohide) from having already run. The markup is
built with `document.createElement`, direct `.style` property writes and `addEventListener` —
**no `<style>` element, no inline handler, no external asset, no `data:` URI, no font** — so
there is nothing for a deployed site's own `script-src`/`style-src` to refuse and nothing
Layer 2's belt has to permit. (Layer 2 *does* open `'unsafe-inline'` on `script-src` and
`style-src` — §Layer 2 point 3 — so the belt would not have blocked it either. The point is
that B13 does not depend on that decision, which is the confirmation `nav_policy.rs:169-184`'s
CSP note demands of anything new arriving in the injection path.)

**Show/hide.** `focusin`/`focusout` on `document`, capture phase (`true`, matching
`inject.rs:42-43`) — `focus`/`blur` do not bubble and would miss every field. Show when the
target is a text-entry surface: `<textarea>`, an element with `isContentEditable`, or an
`<input>` whose effective type is text-entry (`text`, `search`, `url`, `tel`, `email`,
`password`, `number` — not `button`/`checkbox`/`radio`/`file`/`range`/`color`/`submit`). Hide
on `focusout`. The keys must never take focus: `pointerdown` → `preventDefault()` so the field
keeps focus and caret, and the keystroke is applied on that same event. **"Always-on" means "no
config knob", not "always visible"** — with no focused text field the keyboard is not in the
DOM at all.

*Rejected, recorded:* a `display.on_screen_keyboard` config key. `initialization_script` is
built once at boot (`inject.rs:12-18`), so it could only ever be a boot-time knob, for a
control that already self-gates on focus; it would add a schema field, a `validate.rs` entry
and an RT-08 row to buy nothing. *Also rejected:* rendering the keyboard as an iframe on a
bundled page. It would be cross-origin to the deployed site and could not reach the focused
field, and Layer 2's belt sets `frame-ancestors 'none'` (§Layer 2 point 4) — the app-origin
frame would be blocked in exactly the configuration where the belt is present.

**Key delivery, and its ceiling.** For `<input>`/`<textarea>`: splice at
`selectionStart`/`selectionEnd`, restore the caret, then dispatch
`new InputEvent('input',{bubbles:true})` (and `change` on hide) so a framework-managed field
observes the write. For contenteditable: `document.execCommand('insertText', …)`. Backspace,
shift and a symbols layer are the minimum usable set; there is no autocomplete, no IME and no
non-Latin layout. **Synthetic events carry `isTrusted === false`, and no `KeyboardEvent` is
delivered at all**, so a site that gates on trusted key events, or that reads `keydown` instead
of `input`, will not update. That is a real ceiling, recorded here rather than discovered on a
device; H4b is where it surfaces per site.

**What it must not do.** No security-relevant decision passes through page-world JS. B13 adds
**no Tauri command and no ACL entry** — P2-B still touches neither `main.rs:990`'s
`tauri::generate_handler!` nor `capabilities/default.json` (§Layer 2's rejected listener) — so
there is no IPC surface for a page to reach. It does not participate in the exit gesture
(SEC-02) or the idle clock (SEC-06); both stay native. It does not need to: a real finger on
the panel produces the GDK touch events P2-D observes, so typing registers as activity
natively, with no page-JS path into the timer.

**Interaction with the existing injected controls.** `input.allow_text_selection = false` (the
default, `schema.rs:188`) installs `*{user-select:none}` with an `input,textarea` carve-out
(`inject.rs:34-38`), which is already right for the keys. When the operator opts in (`true`)
that rule is omitted entirely and a long-press on a key could start a selection, so the
keyboard sets `user-select:none` on its own container **either way** — the same
"applies regardless of the text-selection choice" rule `inject.rs:26-28` already states for
drag/drop and print. The autohide timer is unaffected: it keys on `mousemove`
(`inject.rs:50-57`) and the keyboard is touch-driven.

**Failure mode (C4).** `build_injection` is pure string assembly and cannot fail, so no boot
path exists to block; the `try/catch` exists only to keep a keyboard defect from costing the
controls injected before it. There is no native error channel here — `initialization_script`
reports nothing back — so a keyboard that fails to appear is **silent**. That is the honest
reason its gate is a checklist row plus one smoke arm rather than a `config.warn`: there is
nothing for the Rust side to observe.

**Gate.** P2-G's **H4b** — *"verify the deployed site's text-entry surfaces on the device
class; record whether any input has no usable keyboard, per device class"* — is the carrier.
With B13 landing, what H4b records changes from "whether any input has no keyboard at all" to
"whether the bundled keyboard appears and types into each text-entry surface on that site".
Declared as an edge onto G; **P2-B does not rewrite G's row.** Mechanically, smoke scenario 10
gains an arm that can actually fail (below), and `build_injection`'s two arms are host-tested.

**Limitation, stated plainly.** An in-page keyboard can only serve in-page fields. It cannot
serve native UI or browser chrome — there is none in this product, which is why the limitation
is survivable — and it is page-world code running on a page the deployed site controls. It is
a **usability feature, not a security control**; nothing in SEC-* depends on it, and no
divergence in it changes an enforcement boundary.

**Windows, and the scope line.** Parent §7's Windows cell already sanctions *"the bundled JS
on-screen keyboard"* as one of the two Windows routes, and Windows has the identical PF-02 gap
(`grep -rniE 'tabtip|InputPane' crates/` → zero hits), so Linux is not diverging downward. A
shared bundled asset is the obvious future consolidation — the JS is platform-free by
construction. **P2-B ships the Linux wiring only**, declares no Windows edge, and changes no
Windows behaviour. `ponytail:` lift `keyboard.js` to a shared asset when someone takes PF-02
on Windows; nothing here has to move for that to happen.

### Downloads — builder line, `scheme_guard.rs` stays a stub

`on_download` denies every `DownloadEvent::Requested` → `false`, emitting `nav.blocked{download}`
once (`REASON_DOWNLOAD`, `scheme_guard.rs:46`, made `pub(crate)` alongside `REASON_EGRESS`).
Returning `false` really cancels on Linux: `wry-0.55.1/src/webkitgtk/web_context.rs:355-358`,
`else { download.cancel(); }`. The cancel lands at `decide-destination`, i.e. after response
headers — the same point Windows' `DownloadStarting` fires, so the parity claim is derived, not
asserted. External schemes were already covered in A (`nav::decide`'s scheme allowlist rides
the nav guard, P2-A:175-177). `scheme_guard.rs`'s `#[cfg(not(windows))]` stub message
(`scheme_guard.rs:58-63`) updates to say downloads are covered by the builder hook.

### M4 / OD-8 — PDF default-block: not a live control for this deployment

Owner disposition, recorded rather than implemented. **P2-B adds no PDF enforcement, no PDF
detection and no PDF assertion**, and the reason is upstream of the platform: the content
origin is operator-controlled and serves no `application/pdf`, so the navigation this control
exists to intercept does not occur. Two independent reasons it would not bite on Linux even if
it did: WebKitGTK ships **no built-in PDF viewer** — `grep -rni pdf` over `webkit2gtk-2.0.2`,
`webkit2gtk-sys-2.0.2` and `wry-0.55.1/src/webkitgtk/` returns **zero hits in all three**,
while the Edge viewer's Print/Save toolbar is M4's entire stated rationale (parent §7 PDF row,
§12/OD-8) — and P2-B already denies **every** download at `WebviewWindowBuilder::on_download`
(§Downloads above), so the one route a non-rendered `application/pdf` response can take is
already closed. `content.pdf_view` is inert in both positions on Linux: no viewer to route to
when `true`, nothing to block when `false`.

`scheme_guard::pdf_decision` stays `#[allow(dead_code)]` and host-tested, wired to nothing on
either platform (`scheme_guard.rs:36-40`, tests at `:203-226`). **The recorded reason changes**
from "descoped, parity" to: *no call site is needed while content is operator-controlled and no
engine on either platform exposes a viewer toolbar.* The `#[cfg(not(windows))]` stub message
(`scheme_guard.rs:58-63`) says so alongside its downloads update.

**The assumption this rests on, made visible.** It rests on the deployment owning its content.
If the fleet is ever pointed at a site the operator does not control, M4 becomes live again —
on Windows first, where the Edge viewer and its toolbar do exist. Recorded so the assumption is
inspectable, **not as a task**: no follow-up item, no owner, no schedule.

### A-filter re-derivation (discharging P2-A rev 3's recorded invariant)

P2-A:223-226 hands forward: "*this filter assumes P2-A installs exactly one
`navigation_handler` and subscribes no RESPONSE or download decision. P2-B adds both … P2-B
must re-derive this filter*." B adds **neither**. There is no `RESPONSE` policy subscription
(PDF unwired), and `on_download`'s route is `WebContext::connect_download_started` →
`Download::connect_decide_destination` (`wry-0.55.1/src/webkitgtk/web_context.rs:307-320`),
never `decide-policy`. So A's stateless `FrameLoadInterruptedByPolicyChange` drop-filter gains
no new producer *class* and stands unchanged. The obligation was re-derivation, not agreement.

Whether a deny-cancelled download surfaces as `load-failed` at all, and what the FSM sees, is
pinned by smoke scenario 9 — including the comparison the spec cannot answer from bindings: it
must **not** be looser than Windows, where a cancelled download's
`NavigationCompleted(IsSuccess=false)`, if it fires, legitimately reaches the FSM. If smoke
shows Linux swallowing an event Windows delivers, the resolution is recorded there, not guessed
here.

### Keep-awake — `systemd-inhibit` child

Windows asserts `ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED` once, for process
life, no undo, with the return value discarded (`main.rs:949-966`, `:964`). Linux
(`#[cfg(target_os = "linux")]`, same `display.keep_awake` gate, `schema.rs:144`) spawns
`systemd-inhibit --what=idle:sleep --who=kiosk-browser --why="kiosk display" --mode=block cat`
with piped stdin:

```rust
let mut child = Command::new("systemd-inhibit").args([...]).stdin(Stdio::piped()).spawn()?;
let _inhibit_pipe = child.stdin.take();          // THIS is what holds the inhibitor open
std::thread::spawn(move || {                     // Telemetry is Send + Clone (P2-A:76-78)
    let status = child.wait();
    telem.config_warn("display.keep_awake", &format!("inhibitor exited: {status:?}"));
});
```

**The binding must be `let _inhibit_pipe = …`, never `let _ = child.stdin.take();`.** The
latter drops the pipe immediately, `cat` gets EOF, the inhibitor is released within
milliseconds, and the watcher thread dutifully reports it — a control that silently does
nothing while looking instrumented. Taking the pipe *before* `wait()` is likewise load-bearing,
not stylistic: `Child::wait` closes stdin before waiting, which would kill the very inhibitor
being held (both halves reproduced under review). Exit symmetry then comes from the pipe:
kiosk-main dying EOFs it, `cat` exits, logind releases the inhibitor — the analogue of Windows
resetting execution state on thread exit. The `Child` is held for the thread's life, which is
the process's life, so nothing accumulates and nothing needs joining at shutdown. Spawn failure
keeps its own `Err` arm → `config.warn`.

`config.warn`, not `eprintln`: best-effort still (C4, never blocks boot) but **observable**,
which `eprintln` in a systemd-launched process is not.

*Rejected, recorded:* a `try_wait()` liveness check on the health sampler. `tokio::spawn(health::run(…))`
is at `main.rs:923`, *before* the keep-awake block at `main.rs:957`, and `tokio::time::interval`'s
first tick completes immediately (`health.rs:34`) — so the check would inspect a child that does
not exist yet and the warn would never fire, on every boot. It is also one-shot, missing a later
death. The watcher thread reports at the instant of death, at any time, with no reordering of
`main.rs` and no new argument to a function already carrying
`#[allow(clippy::too_many_arguments)]`.

*Rejected, recorded:* `gtk::Application::inhibit` — tao owns a real `gtk::Application` but the
field is `pub(crate)` (`tao-0.35.3/src/platform_impl/linux/event_loop.rs:58`), so the route is
unreachable from `kiosk-main` through tao's public API. (The GTK-inhibit-needs-a-session-manager
argument is an external-tier expectation and nothing depends on it.) *Rejected:* `zbus` — a
heavyweight dependency for one D-Bus call (C6); `ponytail:` revisit if P2-C/D acquire a D-Bus
need of their own.

**Honest relabel: under P2-G's runbook this child is defence-in-depth with no current effect.**
Both axes of `--what=idle:sleep` have nothing to inhibit on a conforming device: P2-G masks
`sleep.target suspend.target hibernate.target hybrid-sleep.target`, so logind cannot suspend a
machine whose sleep targets are masked, lock or no lock; and a `what=idle` lock blocks logind's
`IdleAction`, which P2-G asserts is already the default `ignore`, while nothing on the
compositor side raises the idle hint that would reach logind. **Parent §11's precondition
"confirm cage honours idle-inhibit before relying on it" is therefore answered negatively** and
recorded in P2-G's risk section: cage's `wlr_idle_inhibit_v1` is a Wayland *client* protocol
gating cage's own idle notifier, which nothing consumes; it is unrelated to logind inhibitor
locks, and there is no idle timeout for it to inhibit.

The child is kept anyway, and for one reason only: it is the only thing that still functions if
an operator unmasks a sleep target, and it costs one `cat`. It is **not** credited as
discharging PF-07 / M8 / H5 — the parent scopes that honestly ("*`systemd-inhibit` blocks
suspend only, display blanking is compositor-owned — PRIMARY is configuring cage/wlroots not to
blank*", parent §7 keep-awake row), and the PRIMARY half is P2-G's image contract. P2-G's
hardware row correspondingly relabels its `systemd-inhibit --list` check as a regression check
on this spawn path — it proves a lock is held, not that the lock does anything — and takes its
keep-awake evidence from the 24 h observation instead.

## Smoke additions (extend A's harness; 8–11 blocking, 12 blocking with labelled preconditions)

8. **egress**, against the allowlisted local httpd:
   - **(a)** all four request classes SEC-10 enumerates (`egress.rs:1-14`, parent §7:700) to an
     off-allowlist host — `<img src>`, CSS `url()`, `fetch()`, `navigator.sendBeacon` — each
     asserted individually as blocked; **and** a bundled `data:` image renders (the hostless
     rule, which the narrowed block rule must not touch).
   - **(b)** block-observability and the path divergence, on a *second* allowlisted host with a
     path-scoped pattern: an in-pattern path loads; an **off-pattern path also loads** (the
     declared looser divergence, asserted so it is pinned rather than assumed); and a blocked
     off-list request emits exactly one `nav.blocked{egress}`. If a content-blocked load never
     reaches `resource-load-started`, host-scoped blocks are silent on Linux and that residual
     is recorded in this spec *before merge*, not discovered in the field.
   - **(c)** a service-worker-initiated off-list fetch → blocked. Pins whether WebKit's
     content-rule engine covers SW-initiated requests.
   - **(d)** the degrade path, with **no product flag**: the fixture creates
     `data_dir/content-filters` as a **regular file**, so `create_dir_all` fails
     (`mkdir(2)` → `EEXIST`, then std's `is_dir()` check) for **every uid, including root**.
     Asserts exactly one `config.error{egress.filter_absent}`; that an off-list `fetch()` is
     **still blocked** by the belt, recorded by the fixture page's own
     `securitypolicyviolation` listener writing to a DOM node the harness reads; and that the
     kiosk boots and serves. One fixture gates three things: the belt is in force, Layer 1's
     degrade path runs, and the `config.error` escalation fires.
     *Rejected, recorded:* `chmod 000` on the directory. Root ignores DAC denial
     (`CAP_DAC_OVERRIDE`, reproduced), and root is the **only supported principal** — P2-C
     declares a non-root manual run unsupported, P2-G ships root by default, and GitHub Actions
     `container:` jobs are root. The mechanism would have proved the path on a configuration
     nobody deploys. *Also rejected:* a `--no-egress-filter` product flag — shipped code whose
     only function is to disable the sole SEC-10 control on Linux, reachable from the command
     line the launcher constructs (C5/Q4).
9. **downloads:** click a `Content-Disposition: attachment` link → no file appears, exactly one
   `nav.blocked{download}`, kiosk stays on page; the load-event sequence is captured and
   recorded against the A-filter question above.
10. **dialog/chrome/injected controls:** an `alert()`-loop page does not wedge the kiosk and
    paints nothing; right-click produces no context menu; a `beforeunload` page navigates away
    **without prompting**. All three arms asserted explicitly — this is the pin for the
    return-value/`confirm_set_confirmed` assumption. Two arms added in rev 3, both on fixture
    pages the harness already serves from the allowlisted local httpd:
    - **(d) keyboard (B13).** A page with one `<input type="text">`: focus it → the keyboard's
      container is in the DOM; click a key → the input's `value` gained that character and one
      `input` event fired; blur → the container is gone. Fails if the block is not injected, if
      the focus predicate is wrong, if a key steals focus (the value would not change), or if
      the site-CSP-independence claim above is wrong. Cheap: one page, four assertions, no new
      fixture machinery.
    - **(e) print (B14).** A page that creates an `about:blank` iframe and calls
      `iframe.contentWindow.print()` → **no print dialog paints** and the kiosk stays on the
      page. Written this way deliberately: calling `window.print()` from the main document
      cannot fail (the override is non-writable, non-configurable), so it would assert nothing
      about `connect_print`. This arm fails if *neither* entry point covers the child frame,
      which is the composition parent §7 requires.
11. **permissions:** a `geolocation.getCurrentPosition` + `getUserMedia` probe page → denied
    under default-deny, **and allowed** when the fixture config flips `permissions.camera`.
    The positive arm is what proves our handler is the one deciding — an unhandled request
    would default-deny silently and identically.
12. **keep-awake.** *Preconditions* (properties of the bus-less container, not assertions):
    spawn succeeds, and the child exits non-zero. *Assertion:* exactly one
    `config.warn{display.keep_awake}` carrying the child's exit status, emitted by the watcher
    thread — which fails if the thread is missing, if the pipe was not taken before `wait()`,
    or if the warn is wired to `eprintln`. *Non-regression:* the kiosk is unaffected. The
    positive hold assertion belongs to P2-G's hardware checklist with cage, relabelled there
    per §Keep-awake.

## Testing

- **Host tests, `kiosk-core` (per-PR ubuntu CI):** `compile_filter` — accepted-shape emissions
  (literal host, `*.`-prefixed host, exact port, four schemes), refusal of the three
  inexpressible shapes with `config.warn`, block-rule form; `Allowlist::origins()`; and the
  **corpus implication test inside `allowlist.rs`'s own `#[cfg(test)] mod tests`**, asserting in
  `H(u)` terms over the adversarial battery, with new `http://` and `ws://`/`wss://` rows and
  both false-allow URLs found under review added as explicit rows. `regex` is a
  `[dev-dependencies]` entry on `kiosk-core` only.
- **Host tests, `kiosk-main`:** CSP derivation — the expressibility gate returns `None` for each
  refused shape, `data:`/`blob:` present, no path components survive, superset property stated
  against `resource_allowed`; permission classifier — the full mapping table including
  `classify_user_media`'s four arms and the `Other`-deny arm; `REASON_*` label pins. **B13:**
  `build_injection`'s new arm — with `on_screen_keyboard = true` the emitted script contains the
  keyboard block; with `false` it does **not**, which is the C8 pin (the Windows string is
  unchanged), and the three existing tests (`inject.rs:67-95`) keep passing with the added
  argument. Both arms run on the ubuntu job, which is why the flag is a parameter rather than a
  `cfg!` (§B13).
- **Smoke:** scenarios 8–12 above, plus A's 1–7 re-run **with the filter installed** — named
  here as the pin for the custom-scheme assumption rather than left as a generic regression
  sweep, with scenario 3 (bundled offline page) and scenario 7 (`safe.html` from the app
  origin) carrying it.
- Existing gates unchanged: clippy `-D warnings` on the ubuntu `lint-test` job
  (`.github/workflows/ci.yml:24`), `cargo test --workspace` (`:25`), the Linux
  `cargo check -p kiosk-main`, and the Windows release build (`:30-42`). **There is no Windows
  clippy job**; P2-B does not add one (P2-F owns CI changes).

## Error handling

Best-effort doctrine throughout, matching D2b: a failed setter or script swap logs
`config.warn` and never blocks boot; the two egress layers degrade independently;
`try_send`/rate-capping via existing buckets only.

**The C4-vs-C5 rule, named rather than left to collide at implementation time:**

- **Absence of Layer 1 ⇒ `config.error("egress.filter_absent")`** (`Telemetry::config_error`,
  `telemetry.rs:86`), not `config.warn` (`:163`). Compile failure, save failure and
  `add_filter` failure all take it.
- **Absence of Layer 2 ⇒ `config.error("egress.csp_absent")`** when the expressibility gate
  returns `None`. Per-pattern refusals in either layer stay `config.warn` — one bad pattern is
  an operator typo; no policy at all is a different event. The escalation level is the one
  `Allowlist::invalid_patterns` (`allowlist.rs:65-69`) already documents for this class:
  "*so the config layer can raise a `config.error` for the operator rather than failing
  silently*".
- **Neither is boot-blocking.** A kiosk that will not boot is a worse failure than one that
  boots loudly degraded, and the degraded state is not defenceless: P2-A's nav guard still
  enforces the full URLPattern on every frame's *navigations*; what is lost is subresource
  egress. That is exactly the posture P2-A shipped and labelled (`p2a:42`), so the residual is
  a known, named state rather than a new one.
- **Residual, carried by the operator:** a device in this state runs remote content with no
  subresource egress enforcement. It is distinguishable from healthy at `error` level, and
  8(d) proves the path fires. Because a device with a valid signed config and a failed filter
  is *not* in safe mode — `safe.html` never paints — and spool upload depends on provisioning
  within the retention window, the compensating control is provisioning-time and lives in
  P2-G: **H8's cold-install step reads the local spool after first boot and asserts it contains
  no `egress.filter_absent` and no `egress.csp_absent` before sign-off.**

## Open decisions to resolve at plan time

- Exact sys-FFI shim shape (store lifetime, save-callback thread, `add_filter` pointer
  handling) against `webkit2gtk-sys-2.0.2` — and whether the shim can reuse the crate's own
  GObject wrappers via `glib::translate` rather than raw pointers end-to-end.
- The content-rules `url-filter` regex dialect limits (WebKit documents a restricted subset).
  The four emitted forms in the compiler table must be **verified expressible** in it; a form
  that is not fails the compile loudly (`config.error`, naming the form), never silently to
  allow.
- Config-apply → filter/belt refresh ordering relative to the home navigation that follows
  config apply in the effect stream — confirm against the driver.

*Removed from this list under Q5, because "whether the mechanism works at all" is not a
plan-time item:* meta-CSP injection timing and whether a swapped user script applies to the
current document. Both are settled by blocking scenario 8(d). *Removed as obsolete:* the
glob→regex conversion question — allowlist patterns are **URLPattern, not globs**
(`allowlist.rs:26-27,31,119-122`) — and the `zoom-text-only` interaction, decided above.

## Scope / defer

Unchanged from A: launcher/systemd (P2-C), idle/gesture (P2-D), video soak (P2-E), update+CI
harness automation (P2-F), packaging/image/logind/hardware (P2-G).

Rev 3 adds nothing to the deferral list. **B13 is in scope and built here**; what stays out is
RT-16's `inject_css`/`inject_js` (deferred out of P2 by the owner — B13 does not depend on it,
§B13) and the Windows half of PF-02 (parent §7's Windows cell already sanctions the same
bundled keyboard there; P2-B ships the Linux wiring only and declares no Windows edge).
**M4/OD-8 needs no deferral either**: it is not a live control for this deployment (§M4), adds
no code, and carries no follow-up item, owner or schedule.

Recorded ponytails: mid-label host wildcards in the filter compiler (accept once the
implication test covers them); the one-line `^https?://(tauri|kioskasset|ipc)\.localhost/`
ignore rule for the `http://` app-origin form; `zbus` if a second D-Bus consumer appears;
clipboard-read on a future bindings/floor bump; lifting `keyboard.js` to a shared asset if
Windows takes PF-02.

## Amendment register — rev 3

Rev 2's register (B1–B12) is in the review record
(`docs/superpowers/reviews/2026-08-07-p2b-p2g-adversarial-review/P2B-R3-writer.md`, §Final
register) and is unchanged. Rev 3 adds two changes and restates one.

| ID | Change | Final state | Depends on |
|---|---|---|---|
| **B13** | Bundled on-screen keyboard | **In P2-B, built here.** One `keyboard.js` asset + one block in `inject::build_injection` behind a third `on_screen_keyboard: bool` parameter; `main.rs` passes `cfg!(target_os = "linux")` at the existing single `initialization_script` call site. Focus-gated show/hide, no config knob, no IPC, no CSP dependence. Usability feature, not a security control. Gate: P2-G **H4b** + smoke 10(d) + the two host arms | P1's shipped injection engine only (`inject.rs`, `main.rs:1041-1048`). **Not** RT-16. Edge onto P2-G: H4b's recorded question changes (G's row is not rewritten here) |
| **B14** | WebKitGTK `print` signal | `connect_print` → `true` (`web_view.rs:2461`, ungated), completing H1's native half beside the P1 JS override. One row in the hardening mapping; no floor change | B10's declared minimum (unchanged). Gate: smoke 10(e) |
| **B8** | PDF parity (restated) | **Not a live control for this deployment.** No enforcement, no detection, no assertion, no follow-up item. `pdf_decision` stays `#[allow(dead_code)]` with its reason rewritten (§M4). Rev 2's "unowned and escalated" text is withdrawn | Owner ruling that the content origin is operator-controlled |
