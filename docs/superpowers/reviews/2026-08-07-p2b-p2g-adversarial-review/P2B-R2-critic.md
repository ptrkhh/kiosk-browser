# P2-B — CRITIC, Round 2

No frame dispute. Everything below was executed or read this turn; the two probes are
reproducible from the code quoted inline.

## Disposition of Round-1 objections

| OB | Writer's move | My response | Status |
|---|---|---|---|
| OB-1 | CONCEDE; cancel half + `connect_local` + `SignalId::lookup` deleted; B1 demoted to observe-only | Concession is complete and correctly costed. The replacement (reinstated B2) is attacked below as NB-1/NB-2 | **ACCEPTED** |
| OB-2 | CONCEDE both halves; `Option<String>` with an expressibility gate, `'unsafe-inline'`/`'unsafe-eval'` opened, three restrictions reclassified as declared hardening | The gate carries the property: `None` ⇒ no CSP ⇒ trivially never tighter. One branch, no partial policies. Replacement holds | **ACCEPTED** |
| OB-3 | REVISE; blanket "no dynamic signal connection anywhere in P2-B" + an explicit spec sentence naming `sent-request` as non-cancelling | Stronger and cheaper than the probe it replaces. A ban is checkable by grep; a probe is not | **ACCEPTED** |
| OB-4 | REVISE; new blocking scenario 8(d) (filter disabled, belt must still block), fixture-side violation listener, two Open Decisions deleted | The gate is now falsifiable and Q5 is satisfied. But 8(d)'s enabling mechanism is a new defect — see **NB-3** | **ACCEPTED (with NB-3)** |
| OB-5 | REVISE; take `child.stdin` out, hand `Child` to a blocking watcher thread, `config.warn` on death | Verified by execution — the `wait()`-closes-stdin reasoning is exactly right. One required note below | **ACCEPTED** |
| OB-6 | REVISE; `classify_user_media(audio,video)` with `(false,false) ⇒ Deny`, `(true,true) ⇒ camera && microphone` | Correct, and outcome-equivalent to Windows rather than a divergence — WebView2 raises `CAMERA`(2) and `MICROPHONE`(1) as separate `PermissionRequested` events (`hardening.rs:78-83`), each checked separately, so requiring both is the same net verdict | **ACCEPTED** |
| OB-7 | REVISE; 8(a) extended to all four SEC-10 request classes, each blocking; P2-A reconciliation written into the spec | The reconciliation is right on the merits: `resource-load-started`'s subject is a *resource* (`&WebResource, &URIRequest`, `web_view.rs:2523`), not a frame load, so P2-A:227-232's main-frame scoping neither transfers nor is weakened | **ACCEPTED** |
| OB-8 | CONCEDE; ownership claim struck, M4 escalated as an unowned P2-row item | Correct disposition. Inventing an owner would have been worse than the defect. Confirmed against parent §12:930 ("applied", not deferred) | **ACCEPTED** |
| OB-9 | REVISE; 12(a)/(b) relabelled preconditions, real assertion is the watcher-thread warn | Now fails if the thread is missing, if the pipe was not taken, or if the warn is an `eprintln` | **ACCEPTED** |

Nine of nine accepted. I raise nothing further on OB-1…OB-9.

---

## New objections — all against the reinstated B2 and its dependents

### NB-1 — The soundness property `AllowSet(regex) ⊆ AllowSet(URLPattern)` is false for the compiler actually specified (vs B2, HIGH)

The Writer's argument is the right *shape*; the compiler he put on the record does not satisfy
it. I ran the battery's own URLs through both matchers (`urlpattern` 0.3 via
`process_construct_pattern_input` + `UrlPattern::parse`, i.e. `allowlist.rs:119-122`'s exact
path, vs. `regex`). **Two verified false-allows.**

**(a) The port dimension — falsified by the Writer's own worked regex.** He writes
`https://app.example.com/*` → `^https://app\.example\.com(:[0-9]+)?/`. Executed:

```
https://app.example.com:8443/    URLPattern=false   regex=true      <-- FALSE ALLOW
https://app.example.com:443/     URLPattern=true    regex=true
```

`(:[0-9]+)?` wildcards the port. URLPattern does not: an absent port in the pattern means the
scheme's **default** port, pinned deliberately and tested — `allowlist.rs`'s
`the_port_is_pinned_not_wildcarded` ("URLPattern's constructor-string parser sets an absent
port to `""` … it does NOT leave it as a wildcard", asserting `:8443` BLOCKS). The regex must
emit the exact port (`(:443)?` for https, the literal port when the pattern carries one), not
`[0-9]+`. This is in the *port* dimension he claims to compile, not the path dimension he
excluded.

**(b) The wildcard-host dimension — unanalysed, and unsound on the natural translation.** His
battery table has **no wildcard-host row**, yet his own probe output lists
`https://*.example.com/*` (host pattern `*.example.com`) and he explicitly rules
`*.example.com` *is* expressible. Executed, pattern `https://*.example.com/*`:

```
url                                              URLPattern  [^/]*   [a-z0-9.-]*
https://evil.com\@x.example.com/steal?d=secret   false       true    false     <-- FALSE ALLOW
https://evil.com#x.example.com/                  false       true    false     <-- FALSE ALLOW
https://a.b.example.com/x                        true        true    true
https://x.example.com.evil.com/                  false       false   false
```

`https://evil.com\@x.example.com/steal?d=secret` parses to **host `evil.com`, path
`/@x.example.com/steal`** — the battery's `whatwg_spelling_tricks_resolve_to_the_real_host_in_both_directions`
hostile-twin case, `allowlist.rs` `("backslash to evil", r"https://evil.com\@app.example.com")`.
The allowlist blocks it; a wildcard compiled with a permissive class admits it. That is a raw
exfiltration URL to an attacker-controlled host with the payload in the query — precisely the
threat SEC-10 exists for, and it is a *false allow*, not the over-block his §"Host dimension"
table concluded.

The strict class closes both. So the property is recoverable — but it is carried entirely by a
character class the design does not name, in a dimension the soundness argument never examined.

**When.** Any deployment with a wildcard host entry (the common CDN shape) or a non-default
port entry.

**Why it matters.** A single false allow defeats the property, and the property is the whole
justification for reinstating B2. Under C5 this is a fail-open on the SEC-10 gate.

**Remedy, and it is small.** (i) Emit the exact port, never `[0-9]+`. (ii) Pin the wildcard
class to hostname characters (`[a-z0-9-]+(\.[a-z0-9-]+)*`) or refuse wildcard hosts as
inexpressible. (iii) The real fix is one host test, not prose: **run the existing battery
corpus through both matchers and assert the implication** `regex_matches(u) ⇒
allowlist.allows(u)` for every URL in `allowlist.rs`'s adversarial tests. The corpus already
exists; the test is the proof the turn block is trying to give in a table. Without it, the
soundness claim is re-derived by hand at every future pattern change — frame Q5.

**Evidence.** Tier 3 executed (probe, output above); tier 3 read: `crates/kiosk-core/src/nav/allowlist.rs`
(`the_port_is_pinned_not_wildcarded`, `whatwg_spelling_tricks_…`, `an_over_long_host_is_blocked…`).

---

### NB-2 — Two assumptions conceded as MOOT on B1's premise are un-mooted by B2's reinstatement and are not re-declared (vs B2, HIGH)

R1 disposed of two undeclared assumptions with the words "**CONCEDE — and MOOT. The premise
died with B2**" (R1 §Undeclared assumptions 1 and 2). B2 is now alive. Both assumptions are
live again, and R2 addresses neither: `grep` over the R2 turn block finds `data:`/`blob:` at
lines 177 and 186-187 only — both inside the **B3/CSP** section — and zero occurrences of
`tauri://`, `kioskasset`, "app origin", "asset origin", "custom scheme" or "hostless" anywhere
in the B2 section.

**(a) Custom-scheme (app/asset) origins under a block-all rule — HIGH blast radius.** B2's
shape is block-everything first, then `ignore-previous-rules` for allowlist patterns, app/asset
origins, and the content origin. On Linux the app origins are `tauri://localhost` and
`kioskasset://localhost` (P2-A:96-110). Whether WebKit's content-rule engine applies to
custom-scheme loads at all, and whether a `url-filter` can express them, is unknown at tier
1–4 (no headers or GIR on this box; `pkg-config --exists webkit2gtk-4.1` → no). If it applies
and the ignore-rule cannot be expressed, the block-all rule takes out **the splash, the error
page, `offline.html`, `safe.html`, and the offline mp4** — the entire bundled UI, on every
boot. That is not a degraded control, it is the product not starting.

**(b) `data:` / `blob:` / hostless subresources — undeclared stricter divergence.**
`resource_allowed` **allows** every hostless URL by rule 1, deliberately and with its reason on
record (`nav_policy.rs:110-117`, "an inline `data:` image or `blob:` object URL never leaves
the process, so blocking it would only break legitimate bundled assets for zero egress
benefit"). Under block-all with three origin buckets and no hostless bucket, they are blocked
— **tighter than Windows**. R2 declares exactly one divergence, in the looser direction (paths).
This one is stricter, real, and unnamed, which is a C3 defect regardless of severity: R1 itself
conceded "the verifier is right that B's three-bucket filter would have broken them, tighter
than Windows, un-named. That is a divergence B2 would have had to declare."

**Why it matters.** (a) is the largest blast radius in the whole spec (Q4) and is the one
assumption that cannot be discovered late — it fires on first boot. (b) is a silent stricter
divergence that C3 forbids shipping unstated.

**Remedy.** Re-declare both as assumptions with pinning. (a) is already pinned for free if the
smoke re-runs A's scenarios 1–7 with the filter installed — the spec's Testing section says it
does ("A's 1–7 re-run … the belt script and filter must not break A's pages"); it just has to
be named as the pin for *this* assumption rather than left as a regression sweep. (b) needs one
more ignore-rule bucket and one line in the divergence list, or a scenario asserting a bundled
`data:` image renders.

---

### NB-3 — Scenario 8(d) requires a production flag that disables the SEC-10 enforcement authority (vs B12/B3, MED)

8(d) is specified as "the harness starts kiosk-main with the content filter deliberately not
installed (**a harness-only flag**)". The listener moves into the fixture — good, that part is
clean — but the *flag* cannot live in the fixture. It is product code, in the shipped
`kiosk-main`, whose sole function is to turn off the only SEC-10 control on Linux.

`main.rs` has flag precedent (`args.windowed`, `main.rs:968,1033,1084`), so this is not novel
machinery — but `--windowed` does not disable a security boundary, and a kiosk binary that ships
one that does is a fail-open reachable from the command line the launcher constructs. C5/Q4.

**Remedy, cheaper than the flag.** Point the fixture's `data_dir/content-filters/` at an
unwritable path. The filter save then fails through B2's **already-designed** best-effort
degrade path (`config.warn`, Layer 2 stands alone) — which is (i) exactly the state 8(d) exists
to exercise, (ii) the state the Writer names as the one where B3 does real work, and (iii) a
test of the real degradation rather than a synthetic one. No product code, no flag, and it also
gates NB-4 below.

---

### NB-4 — C4 and C5 are not reconciled for the state where *every* egress control is absent (vs B2/B3, MED)

Verified what "loud" means concretely, as asked. `Telemetry::config_warn(&self, field, reason)`
exists (`crates/kiosk-main/src/telemetry.rs:163`) and spools; that is the whole of it. The
kiosk then **continues and loads remote content** (C4, best-effort, never blocks boot).

Two independent `warn`-level paths can hold at once:
- B2's compile/save failure ⇒ `config.warn`, no filter installed;
- B3's expressibility gate returning `None` on any inexpressible pattern ⇒ `config.warn`, no
  CSP injected at all.

Their conjunction is a running kiosk on remote content with **zero** egress enforcement,
indistinguishable in telemetry from healthy except for two warn lines among the boot noise.
C5 says security gates fail closed; C4 says hardening degrades. R2 applies C4 to the last
remaining control and never says which constraint wins.

This is not a hypothetical distinction the codebase lacks a channel for: `Allowlist::invalid_patterns()`
exists at `allowlist.rs:65-69` precisely "*so the config layer can raise a `config.error` for
the operator rather than failing silently*" — a louder level, already built, for exactly this
class of operator-visible policy failure. B2/B3 reach for the quieter one.

**Remedy.** Name the rule: absence of Layer 1 is `config.error`, not `config.warn`, and say
explicitly whether it is boot-blocking (I do not think it should be — a kiosk that will not
boot is worse — but the spec must say so rather than leave C4/C5 to collide at implementation
time).

---

## Findings in the Writer's favour

**The declared path divergence is acceptable, and honestly stated.** The Moderator asked for a
plain verdict; here it is. Three reasons, the second one executed:

1. Traceability: SEC-10's own text and `egress.rs:1-14` frame the threat as *which host a page
   may talk to*; every example the parent gives (`<img src=https://evil/a>`, CSS `url()`,
   `fetch()`, beacon) is off-**origin**.
2. **Path scoping is not an exfiltration control on either platform.** With the full-fidelity
   matcher Windows uses, pattern `https://cdn.example.com/assets/*`:
   ```
   https://cdn.example.com/assets/x?d=SECRET     resource_allowed = true
   https://cdn.example.com/assets/SECRET.gif     resource_allowed = true
   https://cdn.example.com/steal?d=SECRET        resource_allowed = false
   ```
   A page that may reach an in-pattern path may exfiltrate in that path's query string or
   filename. So the *security* delta of the Linux divergence is approximately zero; what is
   lost is least-privilege hygiene, not the exfil boundary.
3. Navigations keep full URLPattern fidelity on every frame via P2-A's nav guard, so the
   operator intent behind a path-scoped entry survives where it is actually enforceable.

The divergence is stated in the direction C3 requires (looser than Windows), names the exact
function it diverges from, and scopes itself to subresources. **No objection.**

**The keep-awake replacement is right, and I verified the load-bearing claim rather than taking
it.** Executed:
```
A (stdin left in Child, then wait()):  child finished = true      <- wait() closed stdin, inhibitor died
B (child.stdin.take() first, wait()):  child finished = false     <- inhibitor held
B, after dropping the taken pipe:      child finished = true      <- releases on EOF, as designed
```
`Child::wait` closing stdin is real, taking the pipe first is load-bearing exactly as claimed,
and the released-on-EOF exit symmetry survives. Thread lifecycle is sound: it blocks for the
child's life, is detached, and dies with the process; nothing needs joining at shutdown.

**One required note, because it is a one-character trap of the same family as OB-3's
`sent-request`:** the binding must be `let _inhibit_pipe = child.stdin.take();` and **not**
`let _ = child.stdin.take();`. The latter drops the pipe immediately, `cat` gets EOF, the
inhibitor is released within milliseconds, and the watcher thread dutifully reports it — a
control that silently does nothing while looking instrumented. Put it in the spec next to the
`sent-request` sentence.

---

## B10 — the floor, re-derived (the Moderator's ask)

The Writer flags that 2.32 is no longer discharged because it rested on `remove_script`/B3.
Establishing it from the reinstated symbol set, all gates read this turn:

| Symbol | Crate | Gate | Owner |
|---|---|---|---|
| `connect_resource_load_started` `web_view.rs:2522` | webkit2gtk | **ungated** | B1 |
| `WebResource::connect_failed` `web_resource.rs:118`, `URIRequest::uri` `uri_request.rs:103` | webkit2gtk | **ungated** | B1 |
| `set_zoom_level` `:1980`, `connect_context_menu` `:2074`, `connect_permission_request` `:2428`, `set_enable_developer_extras` `settings.rs:1475`, `set_zoom_text_only` `settings.rs:1953`, `add_script` `user_content_manager.rs:57` | webkit2gtk | **ungated** | B4, B3 |
| `connect_script_dialog` `web_view.rs:2646` | webkit2gtk | `v2_24` | B4 |
| `remove_filter_by_id` `user_content_manager.rs:151-154` | webkit2gtk | `v2_26` | **B2 (back)** |
| `remove_script` `user_content_manager.rs:163-173` | webkit2gtk | `v2_32` | B3 **only** |
| `webkit_user_content_filter_store_new` / `_save` / `_save_finish`, `webkit_user_content_manager_add_filter` (`webkit2gtk-sys-2.0.2/src/lib.rs:5405-5416, 5467-5478`, and the `add_filter` extern) | webkit2gtk-**sys** | `v2_24` | **B2 (back)** |

**Called-symbol floor = 2.32 while B3 ships; 2.26 if B3 is cut.** Declarations required:
`webkit2gtk = { version = "2.0.2", features = ["v2_32"] }` (cumulative chain
`v2_32→v2_30→…→v2_2` verified in `webkit2gtk-2.0.2/Cargo.toml:54-125`, so it subsumes P2-A's
`["v2_16"]`) **and** a new direct `webkit2gtk-sys = { version = "2.0.2", features = ["v2_24"] }`
(sys chain `v2_24 = ["v2_22"]`, `webkit2gtk-sys-2.0.2/Cargo.toml:123`).

**One correction, LOW.** The turn block writes the sys requirement as `ffi/v2_24`. `ffi/…` is
the *webkit2gtk* crate's alias for its sys dependency's feature; a **direct** `webkit2gtk-sys`
dependency declares `features = ["v2_24"]`. Both may be needed if the shim also touches
`webkit2gtk`-side types, but the spec should not write `ffi/v2_24` against the direct sys dep.

**Consequence the Writer did not draw:** cutting B3 lowers the floor from 2.32 to 2.26. That is
a Q2 argument *for* cutting it, on top of the Q2-marginality he already concedes. I do not
object to keeping B3 — Q1 is real and the parent names it in the same clause — but the
Moderator should note that B3 is the only thing holding the floor at 2.32, and that
P2-A:71-74's hand-forward stays open until B3's shape is fixed either way.

---

## Still open

1. **NB-1** — the compiler's port and wildcard-host translation, and the implication test over
   the battery corpus. Until that test exists the soundness property is asserted, not proved.
2. **NB-2(a)** — custom-scheme origins under block-all. Largest blast radius in the spec and
   the only defect here that fires on first boot.
3. **NB-2(b)** — the `data:`/`blob:` stricter divergence, undeclared.
4. **NB-3 / NB-4** — the 8(d) flag, and the C4-vs-C5 collision when the last control is gone.
5. **B3's fate** — genuinely undecided, by the Writer's own account and mine. It does real work
   in exactly one state (Layer 1 degraded), it is what holds the floor at 2.32, and it needs
   NB-3's mechanism to be gated at all. I do not object to either disposition; I object to
   leaving it unresolved past this round, because B10 cannot be discharged until it is.
6. **M4 / OD-8** — unowned, correctly escalated, not P2-B's to fix.
