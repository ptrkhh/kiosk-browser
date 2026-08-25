#!/usr/bin/env bash
# Run a deliberate missing-decoder case in an environment built without
# gstreamer1.0-libav. Do not remove packages from a developer's host.
set -euo pipefail

: "${GST_INSPECT:=gst-inspect-1.0}"
: "${1:?usage: no-libav.sh COMMAND [ARGS...]}"

if command -v "$GST_INSPECT" >/dev/null 2>&1 &&
  "$GST_INSPECT" avdec_h264 >/dev/null 2>&1; then
  echo "missing-libav precondition failed: avdec_h264 is installed" >&2
  exit 1
fi

shift
exec "$@"
