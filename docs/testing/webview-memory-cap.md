# Webview memory-cap release note

The health sampler reports `health.sample.webview_rss_mb` as the sum of the
supervised process's descendant working sets. When
`maintenance.max_webview_mem_mb` is non-zero, five consecutive samples strictly
over the cap exit with code 80; the surviving launcher records the restart and
restarts the app. A sample at or below the cap resets the consecutive run.

After upgrading to an E4 build, use the fleet's one-week `webview_rss_mb` p99
to choose a cap near twice that value within `[256, 8192]`, or set `0` to
disable it. The Windows 18-W2 floor is a release gate. The tag workflow
downloads the fresh successful endurance run's `windows-w2-*` artifact and
runs `packaging/soak/check-w2-floor.sh`; a missing/malformed artifact or a
floor at least 750 MB blocks the release. No W2 floor artifact is present in
this workspace yet, so the current tree is not release-ready for E5.
