# Scenario 18-W2 — nightly reload resets RSS

This body is the authoritative Windows baseline case from P2-E section E8.

| | **18-W2** (nightly reload resets RSS) |
|---|---|
| Runner | \`windows-latest\` |
| Page | deliberately leaking, **is \`content.url\`** |
| \`maintenance.max_webview_mem_mb\` | **0** (off) |
| \`logging.health_sample_s\` | default (60) |
| \`kiosk.healthy_run_s\` (\`kiosk.ini\`) | default (120) |
| \`maintenance.nightly_reload\` | **a few minutes ahead** |
| \`content.clear_data_on_reset\` | **off** (default is \`true\`, so the fixture must set it \`false\`) |
| \`--safe\` | **no** |
| Asserts | zero restarts; post-reload \`webview_rss_mb\` < pre-reload peak; **post-reload URL == the leaking page**; **steady-state \`webview_rss_mb\` recorded** as a first-class artifact number (E5's floor gate) |

Four preconditions silently void this assertion if omitted:

1. The device is \`Online\` when the timer fires.
2. The leaking page is \`content.url\`, and the post-reload URL is asserted to
   remain that page.
3. \`content.clear_data_on_reset\` is false, so the drop is attributable to the
   reload rather than profile clearing.
4. The process is not started with \`--safe\`.

The runner writes the steady-state floor to \`w2-floor.json\` with the fixture,
date and sample cadence.
