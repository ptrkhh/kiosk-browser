# Scenario 18-W1 — breach → restart

This body is the authoritative Windows memory-cap case from P2-E section E8.

| | **18-W1** (breach → restart) |
|---|---|
| Runner | \`windows-latest\` |
| Page | deliberately leaking, **is \`content.url\`** |
| \`maintenance.max_webview_mem_mb\` | **256** |
| \`logging.health_sample_s\` | **10** (dwell = 50 s) |
| \`kiosk.healthy_run_s\` (\`kiosk.ini\`) | **30** |
| \`maintenance.nightly_reload\` | **unset** |
| \`content.clear_data_on_reset\` | off |
| \`--safe\` | no |
| Asserts | \`webview_rss_mb\` climbs and is reported; breach → **exit 80** → launcher restart with \`watchdog.restart{code: 80}\` on the spool; **no \`watchdog.safe_mode\`** |

The runner records the RSS series and the complete spool. It must distinguish
the cap restart from a crash restart and must fail if safe mode appears.
