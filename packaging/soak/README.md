# P2-E offline-video soak

Scenario 18 is the Debian 12 offline-video soak. The runner owns compositor
startup and process orchestration; this directory owns the assertions and
artifacts.

The positive precondition is mandatory: boot with kiosk-offline.mp4 absent and
wait for exactly one media.error in the durable spool. Restore the asset, then
start the soak. The soak passes only when the process remains alive, the spool
contains zero later media.error events, there are zero launcher restarts
(including exit 80), and the webview_rss_mb series and loop count remain within
the run's recorded baseline bound.

The media contingency activates mechanically when scenario 18 records
media.error{kind:"stall"} with ms_since_wrap < 12000. The page keeps the
native-GL path out of scope until double-buffering also fails on hardware.

The authoritative Windows parameters live in the P2-E design specification
(section E8). scenario-18-w1.md and scenario-18-w2.md copy that table; P2-F
references these IDs and does not duplicate their parameters.

## Artifacts

rss-series.jsonl starts with the t=0 webview_rss_mb baseline. The run also
retains the complete spool, launcher restart records, loop counters and the
compositor log. The W2 artifact records its steady-state floor as a
first-class number; E5's memory-cap enforcement is gated on that measurement.

Before enabling E5, run `packaging/soak/check-w2-floor.sh w2-floor.json` from
the Windows-runner artifact. It fails closed when the artifact is missing or
malformed and exits with status 2 when the measured floor is at least 750 MB.
The tag release workflow performs the same check against the fresh successful
endurance run; no artifact means no release.

## Environment

The runner must provide KIOSK_BIN, KIOSK_LAUNCHER, KIOSK_CONFIG_DIR,
KIOSK_DATA_DIR, WAYLAND_DISPLAY, and a compositor. Do not set
WEBKIT_DISABLE_COMPOSITING_MODE for scenario 18. Missing prerequisites are an
environment failure, not a passing scenario.
