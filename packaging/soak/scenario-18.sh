#!/usr/bin/env bash
# P2-E scenario 18. The surrounding smoke runner supplies process/compositor
# lifecycle; this script owns the positive media.error precondition and the
# zero-error soak assertion.
set -euo pipefail

: "${KIOSK_BIN:?set KIOSK_BIN to the release kiosk-main binary}"
: "${KIOSK_LAUNCHER:?set KIOSK_LAUNCHER to the release launcher binary}"
: "${KIOSK_CONFIG_DIR:?set KIOSK_CONFIG_DIR to the disposable config directory}"
: "${KIOSK_DATA_DIR:?set KIOSK_DATA_DIR to the disposable data directory}"
: "${KIOSK_SOAK_RUNNER:?set KIOSK_SOAK_RUNNER to the compositor-aware runner}"

duration_s="${KIOSK_SOAK_DURATION_S:-7200}"
artifact_dir="${KIOSK_SOAK_ARTIFACT_DIR:-$KIOSK_DATA_DIR/soak-artifacts}"
mkdir -p "$artifact_dir"

count_event() {
  local event="$1"
  find "$KIOSK_DATA_DIR/spool" -name '*.jsonl' -type f -print0 2>/dev/null |
    xargs -0r cat |
    grep -cF "\"event\":\"$event\"" || true
}

missing_asset="$KIOSK_CONFIG_DIR/kiosk-offline.mp4"
if [ -e "$missing_asset" ]; then
  echo "scenario 18 precondition failed: $missing_asset must be absent" >&2
  exit 1
fi

# The runner must boot the real binary and wait for the durable error. A
# successful process start without this observation is not evidence that the
# ACL/IPC path is live.
"$KIOSK_SOAK_RUNNER" precondition \
  --main "$KIOSK_BIN" \
  --launcher "$KIOSK_LAUNCHER" \
  --config "$KIOSK_CONFIG_DIR" \
  --data "$KIOSK_DATA_DIR"

precondition_errors="$(count_event media.error)"
if [ "$precondition_errors" -ne 1 ]; then
  echo "scenario 18 precondition failed: expected one media.error, got $precondition_errors" >&2
  exit 1
fi

"$KIOSK_SOAK_RUNNER" soak \
  --main "$KIOSK_BIN" \
  --launcher "$KIOSK_LAUNCHER" \
  --config "$KIOSK_CONFIG_DIR" \
  --data "$KIOSK_DATA_DIR" \
  --duration-s "$duration_s" \
  --artifact-dir "$artifact_dir"

post_errors="$(count_event media.error)"
if [ "$post_errors" -ne 1 ]; then
  echo "scenario 18 failed: expected no media.error after the precondition, got $post_errors total" >&2
  exit 1
fi

test -s "$artifact_dir/rss-series.jsonl"
test -s "$artifact_dir/compositor.log"
echo "scenario 18 passed; artifacts: $artifact_dir"
