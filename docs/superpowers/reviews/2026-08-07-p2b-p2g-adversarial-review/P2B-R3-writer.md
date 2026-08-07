# P2-B — WRITER, Round 3

No frame dispute. Banked in my favour and not re-argued: the path-scope divergence, and the
keep-awake replacement (with the `let _inhibit_pipe =` binding note **adopted verbatim** into the
spec, next to the `sent-request` sentence — same one-character-trap family, same remedy).

**Dispositions: REVISE 4 (NB-1, NB-2, NB-3, NB-4) · REBUT 0 · CONCEDE 0 · RISK 0.**
Plus B3 settled and the floor declared final.

---

## NB-1 — REVISE

**Both false-allows reproduced this turn**, same path as `allowlist.rs:119-122`
(`process_construct_pattern_input` + `UrlPattern::parse`) against `regex` 1:

```
== PORT: pattern https://app.example.com/*
  https://app.example.com/          URLPattern=true   R2regex=true   exactport=true
  https://app.example.com:443/      URLPattern=true   R2regex=true   exactport=true
  https://app.example.com:8443/     URLPattern=false  R2regex=true  <-- FALSE ALLOW   exactport=false
  https://app.example.com:9443/x    URLPattern=false  R2regex=true  <-- FALSE ALLOW   exactport=false

== WILDCARD HOST: pattern https://*.example.com/*
  https://evil.com\@x.example.com/steal?d=secret  URLPattern=false loose=true <-- FALSE ALLOW  strict=false
  https://evil.com#x.example.com/                 URLPattern=false loose=true <-- FALSE ALLOW  strict=false
  https://a.b.example.com/x                       URLPattern=true  loose=true                  strict=true
  https://x.example.com.evil.com/                 URLPattern=false loose=false                 strict=false
```

My R2 worked regex `(:[0-9]+)?` wildcards the port; `allowlist.rs`'s
`the_port_is_pinned_not_wildcarded` pins the opposite. And I ruled `*.example.com` expressible
while omitting its row from the very table that was supposed to carry the soundness argument —
the omitted row is the one containing an exfiltration URL with the payload in the query. The
Critic is right on both, and right that a property claimed and not held is worse than one never
claimed. My R2 table proved a property about the patterns I chose to tabulate.

**Two further over-blocks my probe surfaced that the objection did not name** — both safe
direction, both recorded so the compiler's cost is fully on the table:

```
  https://evil.com@x.example.com/   URLPattern=true   strict=false   (userinfo; over-block)
  https://X.EXAMPLE.COM/x           URLPattern=true   strict=false   (host case; over-block)
```

**Corrected compiler.** Accept a narrow, provable shape; refuse everything else, loudly.

| Component | Accepted | Emitted |
|---|---|---|
| scheme | literal ∈ {`http`,`https`,`ws`,`wss`} | literal |
| host | literal, **or** leading `*.` + literal suffix | `regex::escape(host)` / `[a-z0-9-]+(\.[a-z0-9-]+)*\.` + escaped suffix |
| port | explicit in pattern, else the scheme default | **exact**: `(:443)?` / `(:80)?` / `:8443` — never `[0-9]+` |
| path / query | — | not compiled (the banked divergence) |

Anything else — mid-label wildcard (`api-*.example.com`), wildcard scheme (`*://`), named or
regex group in host (`:sub.example.com`) — **emits no rule** and raises
`config.warn("egress.filter_pattern", pattern)`. No rule ⇒ blocked ⇒ safe direction. I withdraw
R2's claim that mid-label wildcards are expressible: I cannot derive URLPattern's mid-label `*`
semantics from tier 1–4, so I refuse rather than guess. This also aligns the two layers — B3's
CSP gate refuses exactly the same three shapes — and it is aligned with the codebase's own view
of the widest of them: `allowlist.rs:703-718` already records `*://*/*` as an operator footgun
that "config validation should reject". `ponytail:` accept mid-label wildcards later if an
operator needs one, once the implication test below covers them.

**The corrected soundness claim, stated precisely.** Let `H(u) = (scheme, host, port)`. For every
URL `u` the content blocker matches:

> **regex matches `u` ⇒ ∃ an allowlist pattern `p`, accepted by the table above, with
> `H(u) ∈ H(AllowSet(p))`.**

Scoped exactly: it is an implication (one direction only — over-blocking is permitted and
enumerated above); it is over `H(u)` only, not full URLs (the banked path/query divergence); and
it holds only for patterns the compiler accepted, refusal being the safe direction for the rest.
That is narrower than R2's `AllowSet(regex) ⊆ AllowSet(URLPattern)`, and unlike it, it is true.

**And it is proved by a test, not by this prose.** I adopt remedy (iii) as the primary fix:

```rust
// kiosk-main, host test — the corpus is the existing battery's URLs (allowlist.rs).
#[test] fn compiled_filter_never_allows_what_the_allowlist_blocks() {
    for (patterns, home) in CORPUS_CASES {
        let allow = Allowlist::new(patterns, home);
        let re    = compile_filter(patterns, home).expect("compiles");
        for u in CORPUS_URLS {                      // every URL in allowlist.rs's battery
            if re.is_match(u) {
                assert!(allow.allows(u).is_allowed(), "FALSE ALLOW: {u}");
            }
        }
    }
}
```

`Allowlist` and `allows()` are `pub` in kiosk-core, so no new surface is needed. This is the
check that would have caught both of my false-allows without anyone tabulating anything, and it
re-proves itself on every future pattern change — the Q5 point. Both NB-1 URLs join the corpus
as explicit rows.

---

## NB-2 — REVISE

Correct on both counts, and correct that I never re-declared them: my R1 disposition was "MOOT,
the premise died with B2", and B2 is alive again. R2's B2 section does not contain the words
`tauri://`, `kioskasset`, or `hostless`. That is exactly the failure mode the frame's "declare
dependencies now" rule exists to prevent, and I walked into it by mooting rather than parking.

**One change disposes of both, and it is smaller than the remedies offered.** Do not add ignore-rule
buckets — **narrow the block-all rule's `url-filter` from `.*` to `^(https?|wss?)://`.**

- **(a) Custom-scheme origins.** `tauri://localhost`, `kioskasset://localhost`, `ipc://localhost`
  never match the block rule, so the splash, error page, `offline.html`, `safe.html` and the mp4
  are untouched **whether or not WebKit's content-rule engine applies to custom schemes**. The
  assumption is *dissolved rather than pinned* — which matters here more than anywhere, because
  it is the one defect that fires on first boot (Q4) and the one I have no tier 1–4 artifact to
  settle. An ignore-rule bucket would have depended on the same unknown it was meant to cover.
- **(b) Hostless subresources.** `data:`, `blob:`, `about:` never match either, so they are not
  blocked. No stricter divergence, nothing to declare, and it now mirrors `resource_allowed`'s
  rule 1 *by construction*: that function returns `true` for `!is_remote_origin(url)`
  (`nav_policy.rs:131-134`), and `is_remote_origin` returns `false` for both custom-scheme hosts
  and every hostless URL (`nav_policy.rs:233-243`, `None => false`, as amended by P2-A:96-110).
  The filter's block rule and the shipped predicate now agree on the same set.
- `ws`/`wss` are in the block rule because `resource_allowed` polices them (the allowlist is
  matched scheme-included — `nav_policy.rs:120-130`). Whether the content blocker sees WebSocket
  handshakes is unknown at tier 1–4; including them costs nothing and closes it if it does.
  Declared as a residual, not asserted.

**Consequential simplifications** (the reason this is the lazy fix, not just a safe one): the
app-origin and asset-origin ignore buckets are **deleted** — nothing to express, nothing to get
wrong. The content-origin bucket is **also deleted**, because it was wrong on parity grounds
independently: a populated allowlist does *not* implicitly admit the home origin's other paths
(`allowlist.rs:386-397`, `a_populated_allowlist_does_not_implicitly_allow_the_home_origin`), so a
blanket content-origin allowance would have been looser than Windows. The compiler therefore
emits rules for exactly what `Allowlist::allows` implements: rule 4 (the patterns), rule 2 (the
exact home URL, widened to its origin — inside the banked host-granularity divergence, and I say
so in the spec rather than letting it pass as a new one), and rule 3 (the origin lock when the
configured list is empty).

**Pinning for what remains.** A's scenarios 1–7 re-run with the filter installed is named in the
spec as **the pin for assumption (a)** rather than left as a generic regression sweep, per the
remedy — with 3 and 7 (bundled offline page, `safe.html` from the app origin) called out as the
assertions that carry it. A bundled `data:` image is added to scenario 8's fixture for (b).

---

## NB-3 — REVISE

Accepted without reservation, and the remedy is better than mine on three axes at once. A
`--no-egress-filter` flag would be product code in the shipped binary whose only function is to
disable the sole SEC-10 control on Linux, reachable from the command line the launcher builds
(C5/Q4). `--windowed` (`main.rs:968`) is not precedent for that.

**Replacement, adopted:** scenario 8(d) makes `data_dir/content-filters/` unwritable. The save
then fails through B2's already-designed best-effort path (`config.error` per NB-4, Layer 2
alone), which is the exact state 8(d) exists to exercise. No product code, no flag, and it tests
the real degradation rather than a synthetic one — so 8(d) now gates B3 **and** B2's degrade path
in one scenario, which is also the NB-4 gate. Three things, one fixture, zero new surface.

---

## NB-4 — REVISE

Accepted: R2 applied C4 to the last remaining control and never said which constraint wins. Both
`warn` paths can hold at once, and `Telemetry::config_warn` (`telemetry.rs:163`) spools and that
is all.

**The rule, named.** `Telemetry::config_error` exists at `telemetry.rs:86`, and
`Allowlist::invalid_patterns` (`allowlist.rs:65-69`) documents exactly this escalation intent
("so the config layer can raise a `config.error` for the operator rather than failing silently").
So:

- **Absence of Layer 1 ⇒ `config.error("egress.filter_absent")`, not `config.warn`.** Compile
  failure, save failure, or `add_filter` failure all take it.
- **Absence of Layer 2 ⇒ `config.error("egress.csp_absent")`** when the expressibility gate
  returns `None`. Per-pattern refusals stay `config.warn` — one bad pattern is an operator typo,
  no policy at all is a different event.
- **Neither is boot-blocking.** Stated explicitly in the spec so C4 and C5 cannot collide at
  implementation time. Justification: a kiosk that will not boot is a worse failure than one that
  boots loudly degraded, and the degraded state is not defenceless — P2-A's nav guard still
  enforces the full URLPattern on every frame's *navigations*; what is lost is subresource egress.
  That is precisely the posture P2-A shipped and labelled (P2-A:42, "do not field a Linux device
  before P2-B"), so the residual is a known, named state rather than a new one.
- **Residual, carried by the operator:** a device in this state runs remote content with no
  subresource egress enforcement. It is distinguishable from healthy at `error` level, which is
  the whole point of the escalation, and 8(d) proves the path fires.

---

## B3 — SETTLED: keep it. Floor and feature declaration, final.

**Keep.** Q1 decides it: parent §7:700 names "plus an injected restrictive CSP" in the same
clause as the filter, so it is a named requirement and not mine to YAGNI. I accept the Critic's
Q2 point that it does real work in exactly one state — and note that NB-3/NB-4 have now made that
state a *gated, asserted* one (8d), which is a better answer than cutting a requirement the
parent names.

I checked whether the `v2_32` cost is avoidable and it is not: the gate comes from `remove_script`
(`user_content_manager.rs:163-173`), needed for the `ConfigApplied` swap; `remove_all_scripts` is
forbidden (destroys wry's bootstrap, `wry/webkitgtk/mod.rs:721-738`), and every alternative
reintroduces a Tauri command. **The floor saving from cutting B3 is 2.32 → 2.26, which is worth
nothing in practice** — both are below the v2_40 already in the build via
`tauri-runtime-wry-2.11.4`'s `linux-body`, and below Debian 12's shipped WebKitGTK. Paying a
zero-cost floor bump to keep a parent-named control is the right trade.

**Final declaration** (the `ffi/v2_24` spelling correction accepted — LOW, and correct: `ffi/…`
is the `webkit2gtk` crate's alias for *its* sys dependency, not how a direct dependency declares
its own feature):

```toml
[target.'cfg(target_os = "linux")'.dependencies]
webkit2gtk     = { version = "2.0.2", features = ["v2_32"] }
webkit2gtk-sys = { version = "2.0.2", features = ["v2_24"] }
```

**Called-symbol floor = 2.32, final.** Chains verified: `webkit2gtk` `v2_32→v2_30→…→v2_2`
(`webkit2gtk-2.0.2/Cargo.toml`), so `["v2_32"]` subsumes P2-A's `["v2_16"]`; sys
`v2_24 = ["v2_22"]` at `webkit2gtk-sys-2.0.2/Cargo.toml:123`. Highest gate called is `v2_32`; no
`v2_40`-gated symbol; no `ResponsePolicyDecision`. **P2-A:71-74's hand-forward is discharged and
no longer open.**

---

## Final register

| ID | Change | State after R3 |
|---|---|---|
| B1 | `resource-load-started` | Observe-only. Per-resource `connect_failed` → `nav.blocked{egress}`; block-observability smoke-pinned (8b). Ungated symbols only |
| B2 | Content filter (sys-FFI shim) | **Enforcement authority.** Block rule narrowed to `^(https?\|wss?)://` (NB-2); compiler accepts literal/`*.` hosts + exact ports only, refuses the rest loudly (NB-1); app/asset/content-origin buckets **deleted**; soundness carried by the corpus implication test |
| B3 | CSP belt | **KEPT** (Q1). `Option<String>` expressibility gate; `None` ⇒ no CSP + `config.error`; `'unsafe-inline'`/`'unsafe-eval'` opened; three restrictions declared hardening. Gated by 8(d) |
| B4 | hardening mapping | Clean pass, unchanged |
| B5 | permission classifier | `classify_user_media(audio,video)`; `(false,false) ⇒ Deny`, `(true,true) ⇒ camera && microphone` |
| B6 | clipboard-read divergence | Clean pass, unchanged |
| B7 | downloads via `on_download` | Clean pass, unchanged |
| B8 | PDF parity | Ownership claim struck; M4 escalated as an unowned P2-row item. Not P2-B's to fix |
| B9 | keep-awake | Watcher thread; `let _inhibit_pipe = child.stdin.take();` **spelled in the spec**, never `let _ =` |
| B10 | feature/floor | **DISCHARGED.** `webkit2gtk ["v2_32"]` + direct `webkit2gtk-sys ["v2_24"]`; floor 2.32 |
| B11 | A-filter re-derivation | Clean pass; `send-request` claim deleted |
| B12 | smoke 8–12 | 8(a) all four SEC-10 classes + a bundled `data:` image; 8(b) block-observability; 8(d) unwritable filter dir — gates B3, B2's degrade path and NB-4's `config.error`, no product flag; A's 1–7 re-run named as the pin for NB-2(a); 12 preconditions relabelled |

**Divergence list, final** (C3, both directions): *looser* — subresource path/query unenforced at
Layer 1 (banked); *stricter* — clipboard-read always denied (B6); *stricter, over-block only* —
userinfo, uppercase-host and raw-string spelling variants under a wildcard-host pattern (NB-1,
enumerated, safe direction); *deliberate hardening tighter than the allowlist* — `object-src`,
`base-uri`, `frame-ancestors` in B3.

**Withdrawn this round:** R2's `AllowSet(regex) ⊆ AllowSet(URLPattern)` claim, the `(:[0-9]+)?`
port form, the "mid-label wildcards are expressible" claim, the `.*` block-all rule, the three
origin ignore-buckets, and the 8(d) product flag.
