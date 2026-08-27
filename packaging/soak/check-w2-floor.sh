#!/usr/bin/env bash
set -euo pipefail

# E5's release gate. This deliberately refuses a missing or malformed artifact;
# an absent Windows run is not evidence that the default cap is safe.
artifact="${1:?usage: check-w2-floor.sh w2-floor.json}"
command -v jq >/dev/null 2>&1 || {
  echo "jq is required to read the W2 floor artifact" >&2
  exit 1
}
test -s "$artifact"

floor="$(jq -er '.steady_state_floor_mb // .floor_mb' "$artifact")"
case "$floor" in
  ''|*[!0-9]*)
    echo "W2 floor must be a non-negative integer MB value" >&2
    exit 1
    ;;
esac

if [ "$floor" -ge 750 ]; then
  echo "E5 blocked: W2 steady-state floor is ${floor} MB (>= 750 MB)" >&2
  exit 2
fi

echo "E5 eligible: W2 steady-state floor is ${floor} MB (< 750 MB)"
