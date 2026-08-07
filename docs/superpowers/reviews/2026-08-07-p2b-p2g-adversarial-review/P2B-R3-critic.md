# P2-B — CRITIC, Round 3 (closing)

No frame dispute. Four dispositions, two documented corrections, one explicit confirmation.

## Disposition

**NB-1 — ACCEPTED. The narrowed claim is true; I checked the emissions against the battery, not the prose.**
`host → [a-z0-9-]+(\.[a-z0-9-]+)*\.` + escaped suffix, port exact, scheme literal, kills both of
mine: `https://evil.com\@x.example.com/steal?d=secret` fails on `\` and `@`; `:8443` fails on
`(:443)?`. It also holds on the corpus rows he did not have to check —
`a_wildcard_host_pattern_still_resists_the_suffix_and_userinfo_tricks` (`allowlist.rs:652-661`):
`app.example.com.evil.com` (no match, `.evil.com` after suffix), `app.example.com@evil.com` (no
match, `@`), bare apex `example.com` (no match, `*.` requires the dot — the emitted regex
requires a label plus dot before the suffix, matching URLPattern). Refusing mid-label wildcards
rather than guessing URLPattern's semantics is the right call and is consistent with
`allowlist.rs:703-718`'s own view of the widest shape.

The corpus test is the right remedy and the corpus is adequate: the wildcard-host pattern case
already exists (`:653`), as does an explicit-non-default-port pattern (`:602`), so the
cross-product form of his pseudocode reaches the class I found. Two notes the Moderator asked for:

- **Correction, load-bearing (adopt into the test, not a new objection).** The pseudocode asserts
  `re.is_match(u) ⇒ allow.allows(u).is_allowed()`. That is R2's withdrawn full-URL claim, not the
  R3 claim, and **it cannot pass** — it is falsified by the banked path divergence itself:
  pattern `https://app.example.com/kiosk/*`, URL `https://app.example.com/kiosk-admin/x`
  (`allowlist.rs:641`) — allowlist BLOCKS, host-granularity filter matches. Same for the
  home-origin widening: `a_populated_allowlist_does_not_implicitly_allow_the_home_origin`
  (`:387-397`, patterns `["https://somewhere-else.example/*"]`, `HOME =
  https://app.example.com/kiosk`) asserts `https://app.example.com/other` BLOCKS, which the
  widened home-origin rule matches by design. The assertion must be in `H(u)` terms, as his prose
  states it: *for every matched `u`, some accepted pattern (or the home URL) has the same
  `(scheme, host, port)`*. One-line encoding fix inside an already-agreed remedy.
- **Corpus gap.** The battery is https-only: zero `ws://`/`wss://` cases and zero `http://`
  pattern cases (`grep` → 0 each). The compiler accepts four schemes and the block rule covers
  four. Add one `wss://` and one `http://` row so the non-https arms are exercised.

**NB-2 — ACCEPTED. The reasoning holds and the dissolution is better than the pinning I proposed.**
`^(https?|wss?)://` is anchored, so `tauri://localhost`, `kioskasset://localhost`,
`ipc://localhost`, `data:`, `blob:`, `about:` and `blob:https://…` never match — (a) is dissolved
independently of the unknown, and (b) needs no bucket and no divergence, and now mirrors
`resource_allowed`'s rule 1 by construction (`nav_policy.rs:131-134` + `is_remote_origin`
`:233-243`). Deleting three buckets to fix two defects is the right shape.

**Content-origin deletion — CONFIRMED, verbatim.** `allowlist.rs:387-397`,
`a_populated_allowlist_does_not_implicitly_allow_the_home_origin`: "*Only the exact home URL is
implicit — not its whole origin*", asserting `https://app.example.com/other` and
`https://app.example.com/` both BLOCK under a populated list. A blanket content-origin bucket
would indeed have been looser than Windows. His replacement — emit rule 4, rule 2 widened to its
origin, rule 3's origin lock — is the correct set, and the rule-2 widening is genuinely inside
the banked host-granularity divergence rather than a new one.

**Residual, accepted as documented risk (not HIGH, and pinned).** `http://tauri.localhost`,
`http://kioskasset.localhost` and `http://ipc.localhost` **do** match the narrowed block rule,
and the app-origin bucket is now deleted. On Linux the app origin is the custom scheme
(`main.rs:46-53` marks the `http://` form as the *Windows* WebView2 workaround; P2-A:96-110
resolves it to `tauri://localhost`), so this should never fire — but P2-A deliberately keeps both
forms in `is_remote_origin` with no `cfg`, so the two documents are one refactor apart from
disagreeing. It is pinned: A's scenarios 3 and 7 are named as the assertion carrier, and they
fail loudly if it fires. A single ignore rule `^https?://(tauri|kioskasset|ipc)\.localhost/`
would close it for one line and no unknown; I record it as a suggestion, not a condition.

**NB-3 — ACCEPTED.** Unwritable `content-filters/` dir; no product flag; 8(d) gates B3, B2's
degrade path and NB-4's `config.error` in one fixture.

**NB-4 — ACCEPTED.** The rule is named and the split is right. Verified `Telemetry::config_error`
exists at `telemetry.rs:86` and takes one arg — his `config_error("egress.filter_absent")` call
shape matches it (`config_warn` at `:163` is the two-arg one). Non-boot-blocking is the correct
call and is now stated rather than left to collide; the residual is named and level-separated
from healthy.

**B3 / B10 — ACCEPTED, and P2-A:71-74 is genuinely discharged.** `remove_script` is the only
route to the `ConfigApplied` swap without `remove_all_scripts` (forbidden) or a Tauri command, so
`v2_32` is not avoidable while B3 lives. The 2.32→2.26 saving is worthless as argued — both are
runtime floors far below Debian 12's shipped WebKitGTK, and the compiled crate already carries
`v2_40` via `tauri-runtime-wry`'s `linux-body`. Declaration `webkit2gtk ["v2_32"]` +
`webkit2gtk-sys ["v2_24"]` is correct against the symbol table I built in R2 (highest webkit2gtk
gate called: `remove_script` `v2_32`; highest sys gate: the filter-store externs `v2_24`,
`webkit2gtk-sys-2.0.2/src/lib.rs:5405-5478`; sys chain `v2_24 = ["v2_22"]`, `Cargo.toml:123`).
No `v2_40`-gated symbol, no `ResponsePolicyDecision` — A's two conditions, both met. **Discharged.**

## Consistency confirmation

**I confirm.** P2-B is internally consistent, every objection I raised across three rounds is
dispositioned, and **there is no open HIGH.** The two HIGHs (NB-1 soundness, NB-2 un-mooted
assumptions) are closed on evidence I checked myself rather than on assurance. What remains is
two mechanical corrections inside remedies already agreed — the implication test's assertion form
and the https-only corpus — plus one pinned residual (`http://*.localhost` app-origin form) and
one unowned item that is not P2-B's (M4/OD-8, to integration).

I have said twice that SEC-10 was not soundly enforced on Linux. It now is, and I say so as
plainly as I said the opposite: **SEC-10 is soundly enforced on Linux at (scheme, host, port)
granularity by B2, with a declared, defensible and verified-inconsequential path/query divergence,
a fail-closed refusal path for patterns the compiler cannot prove, and `config.error` when the
layer is absent.** The soundness is carried by a corpus test over the existing adversarial
battery rather than by a table — which is the part that makes it hold up after this debate ends.
