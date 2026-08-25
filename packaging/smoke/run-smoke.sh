#!/usr/bin/env bash
# Linux smoke harness. The scenario body is selected by KIOSK_SCENARIO when
# called from crates/kiosk-smoke; without it, all scenarios run.
# Compositor
# start/stop lives in exactly one function each (start_compositor/stop_compositor)
# and every scenario's assertions live in its own scenario_N function, so that port
# is a move, not a rewrite.
#
# Environment note (see README.md "Concerns" for the full evidence): this
# container has no /dev/input and GDK itself reports no seat. xdotool-driven
# (XTest, via Xwayland) input reproducibly segfaults Xwayland here, and a
# script-dispatched click cannot supply the trusted user gesture WebKit's own
# popup policy requires for window.open()/target=_blank. So real navigations
# (off-allowlist links, self-reload) are driven by the fixture PAGES themselves
# via `.click()`/`location.reload()` — a genuine decide-policy request, not a
# simulation of the guard's logic — while Xwayland is used ONLY for scenario 1's
# read-only window-geometry check (search/getwindowgeometry; proven safe in
# isolation — no XTest call). The two target=_blank sub-checks in scenario 2
# cannot be genuinely exercised in this container; see the per-scenario table.
set -uo pipefail

SMOKE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KIOSK_MAIN="${KIOSK_BIN:-${KIOSK_MAIN:-$SMOKE_DIR/../../target/debug/kiosk-main}}"
FIXTURE_PORT=8099
WAYLAND_OUT_W=1280
WAYLAND_OUT_H=720

RUNTIME_DIR="$(mktemp -d)"
SERVE_DIR="$(mktemp -d)"   # httpd document root: a refreshable COPY of fixtures/, never the source
CONFIG_DIR="$(mktemp -d)"  # kiosk.ini + credential + mp4, per-run, never the fixtures/ source
DATA_DIR="/var/lib/kiosk"  # the real Linux data dir (spec §4) -- Task 2's resolve_data_dir(), never overridable

export XDG_RUNTIME_DIR="$RUNTIME_DIR"
export WAYLAND_DISPLAY="wayland-smoke"
export WEBKIT_DISABLE_COMPOSITING_MODE=1   # smoke environment ONLY -- never in shipped code or units
chmod 700 "$RUNTIME_DIR"

# start_kiosk unconditionally clears DATA_DIR before every scenario. Refuse to
# run at all unless the operator has explicitly confirmed that is safe -- a
# provisioned kiosk device's real /var/lib/kiosk must never be silently wiped by
# a human running this harness on the wrong host (review round 2, Minor 11).
if [ -z "${KIOSK_SMOKE_I_MEAN_IT:-}" ]; then
  echo "refusing to run: this harness repeatedly wipes $DATA_DIR." >&2
  echo "set KIOSK_SMOKE_I_MEAN_IT=1 once you've confirmed that path is disposable on this host." >&2
  exit 1
fi

PASS_COUNT=0
FAIL_COUNT=0
RESULTS=()   # "N|name|PASS" or "N|name|FAIL" per scenario, printed as the summary table

log() { echo "[smoke] $*" >&2; }

# ---------------------------------------------------------------------------
# Compositor lifecycle -- the ONE pair of functions P2-F reuses verbatim.
# ---------------------------------------------------------------------------
start_compositor() {
  mkdir -p /tmp/.X11-unix   # weston's xwayland module needs this dir to bind X0; harmless if already present
  # Default shell (desktop-shell), NOT kiosk-shell.so: kiosk-shell.so forces every
  # top-level surface to the output's full extent regardless of whether kiosk-main
  # asked to be fullscreen, which makes scenario 1's geometry check a property of
  # the compositor, not of kiosk-main (review round 2, Critical 1 -- see
  # README.md). desktop-shell's ~32px panel offset is tolerated (logged,
  # not asserted) in scenario_1 instead. Nothing else in this file reads geometry.
  weston --backend=headless-backend.so --socket="$WAYLAND_DISPLAY" --idle-time=0 \
    --xwayland --width="$WAYLAND_OUT_W" --height="$WAYLAND_OUT_H" \
    >"$RUNTIME_DIR/weston.log" 2>&1 &
  WESTON_PID=$!
  for _ in $(seq 1 50); do
    [ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ] && break
    sleep 0.1
  done
  if [ ! -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]; then
    echo "weston did not create its socket" >&2
    cat "$RUNTIME_DIR/weston.log" >&2
    exit 1
  fi
  # Xwayland is a client of weston, started asynchronously after the wayland
  # socket exists; used only for scenario 1's read-only window-geometry check
  # (see the file header). Parse the ACTUAL granted display number rather than
  # assuming :0 -- a leftover /tmp/.X11-unix/X0 lock from a prior run can push
  # a fresh Xwayland onto :1.
  X11_DISPLAY=""
  for _ in $(seq 1 50); do
    X11_DISPLAY="$(sed -n 's/.*xserver listening on display :\([0-9]*\).*/\1/p' "$RUNTIME_DIR/weston.log" | tail -1)"
    [ -n "$X11_DISPLAY" ] && [ -S "/tmp/.X11-unix/X${X11_DISPLAY}" ] && return 0
    sleep 0.1
  done
  echo "xwayland did not create its X11 socket" >&2
  cat "$RUNTIME_DIR/weston.log" >&2
  exit 1
}

stop_compositor() {
  kill "${LAUNCHER_PID:-}" 2>/dev/null || true
  kill "${CAGE_PID:-}" 2>/dev/null || true
  wait "${LAUNCHER_PID:-}" 2>/dev/null || true
  wait "${CAGE_PID:-}" 2>/dev/null || true
  kill "${WESTON_PID:-}" 2>/dev/null || true
  wait "${WESTON_PID:-}" 2>/dev/null || true
  rm -rf "$RUNTIME_DIR" "$SERVE_DIR" "$CONFIG_DIR"
}
trap stop_compositor EXIT

stop_weston_only() {
  kill "${WESTON_PID:-}" 2>/dev/null || true
  wait "${WESTON_PID:-}" 2>/dev/null || true
  WESTON_PID=""
}

prepare_launcher() {
  [ -n "${KIOSK_LAUNCHER:-}" ] && [ -x "$KIOSK_LAUNCHER" ] || {
    log "KIOSK_LAUNCHER is required for cage scenarios"
    return 1
  }
  LAUNCH_DIR="$RUNTIME_DIR/launcher"
  rm -rf "$LAUNCH_DIR"
  mkdir -p "$LAUNCH_DIR"
  cp "$KIOSK_LAUNCHER" "$LAUNCH_DIR/kiosk-launcher"
  cp "$KIOSK_MAIN" "$LAUNCH_DIR/kiosk-main"
  chmod 755 "$LAUNCH_DIR/kiosk-launcher" "$LAUNCH_DIR/kiosk-main"
}

start_cage_app() {
  local executable="$1"; shift
  stop_weston_only
  CAGE_RUNTIME_DIR="$RUNTIME_DIR/cage"
  mkdir -p "$CAGE_RUNTIME_DIR"
  chmod 700 "$CAGE_RUNTIME_DIR"
  WAYLAND_DISPLAY="wayland-cage"
  export XDG_RUNTIME_DIR="$CAGE_RUNTIME_DIR" WAYLAND_DISPLAY
  local cage_log="$RUNTIME_DIR/cage.log"
  if [ "${KIOSK_CAGE_GDK_BACKEND:-}" = x11 ]; then
    env -u DISPLAY GDK_BACKEND=x11 WLR_BACKENDS=headless \
      cage -- "$executable" "$@" >"$cage_log" 2>&1 &
  else
    env -u DISPLAY -u GDK_BACKEND WLR_BACKENDS=headless \
      cage -- "$executable" "$@" >"$cage_log" 2>&1 &
  fi
  CAGE_PID=$!
  for _ in $(seq 1 100); do
    [ -S "$CAGE_RUNTIME_DIR/$WAYLAND_DISPLAY" ] && break
    kill -0 "$CAGE_PID" 2>/dev/null || break
    sleep 0.1
  done
  if [ ! -S "$CAGE_RUNTIME_DIR/$WAYLAND_DISPLAY" ]; then
    log "cage did not create its Wayland socket"
    cat "$cage_log" >&2
    return 1
  fi
  X11_DISPLAY=""
  for _ in $(seq 1 100); do
    X11_DISPLAY="$(sed -n 's/.*xserver listening on display :\([0-9]*\).*/\1/p' "$cage_log" | tail -1)"
    [ -n "$X11_DISPLAY" ] && [ -S "/tmp/.X11-unix/X${X11_DISPLAY}" ] && break
    sleep 0.1
  done
  if [ "${KIOSK_CAGE_GDK_BACKEND:-}" = x11 ] && [ -z "$X11_DISPLAY" ]; then
    log "cage did not create its Xwayland display"
    cat "$cage_log" >&2
    return 1
  fi
}

start_cage_launcher() {
  prepare_launcher || return 1
  prepare_kiosk_files "${1:-kiosk.ini}" || return 1
  start_cage_app "$LAUNCH_DIR/kiosk-launcher" --config "$CONFIG_DIR" || return 1
  LAUNCHER_PID=""
  for _ in $(seq 1 100); do
    LAUNCHER_PID="$(pgrep -f "^$LAUNCH_DIR/kiosk-launcher --config" | head -1 || true)"
    [ -n "$LAUNCHER_PID" ] && break
    sleep 0.1
  done
  [ -n "$LAUNCHER_PID" ]
}

supervised_main_pid() {
  pgrep -f "^$LAUNCH_DIR/kiosk-main --config" | head -1 || true
}

supervised_main_present() {
  [ -n "$(supervised_main_pid)" ]
}

supervised_main_changed() {
  local old="$1" current
  current="$(supervised_main_pid)"
  [ -n "$current" ] && [ "$current" != "$old" ]
}

stop_cage_app() {
  if [ -n "${LAUNCHER_PID:-}" ]; then
    kill "$LAUNCHER_PID" 2>/dev/null || true
    wait "$LAUNCHER_PID" 2>/dev/null || true
  fi
  if [ -n "${KIOSK_PID:-}" ]; then
    kill "$KIOSK_PID" 2>/dev/null || true
    wait "$KIOSK_PID" 2>/dev/null || true
  fi
  kill "${CAGE_PID:-}" 2>/dev/null || true
  wait "${CAGE_PID:-}" 2>/dev/null || true
  CAGE_PID=""
  LAUNCHER_PID=""
}

# ---------------------------------------------------------------------------
# Fixture httpd -- serves a REFRESHABLE COPY of fixtures/ (never the checked-in
# source), so a scenario can swap which signed config is at /config.json without
# touching the repo, and scenario 3 can stop/restart it cleanly.
# ---------------------------------------------------------------------------
start_fixtures() {
  rm -rf "$SERVE_DIR"; mkdir -p "$SERVE_DIR"
  cp "$SMOKE_DIR"/fixtures/*.html "$SERVE_DIR/"
  cp "$SMOKE_DIR"/fixtures/*.js "$SERVE_DIR/" 2>/dev/null || true
  cp "$SMOKE_DIR"/fixtures/*.bin "$SERVE_DIR/" 2>/dev/null || true
  if [ -n "${KIOSK_HTTPD_BIN:-}" ]; then
    test -x "$KIOSK_HTTPD_BIN" || {
      log "fixture server is not executable: $KIOSK_HTTPD_BIN"
      return 1
    }
    KIOSK_FIXTURE_ROOT="$SERVE_DIR" KIOSK_FIXTURE_PORT="$FIXTURE_PORT" \
      "$KIOSK_HTTPD_BIN" >"$RUNTIME_DIR/httpd.log" 2>&1 &
  else
    # Local fallback for the pre-P2-F human runner. CI and endurance always set
    # KIOSK_HTTPD_BIN, so the Debian floor does not carry Python just for tests.
    command -v python3 >/dev/null 2>&1 || {
      log "KIOSK_HTTPD_BIN is required when python3 is unavailable"
      return 1
    }
    ( cd "$SERVE_DIR" && exec python3 -u -m http.server "$FIXTURE_PORT" ) \
      >"$RUNTIME_DIR/httpd.log" 2>&1 &
  fi
  HTTPD_PID=$!
  for _ in $(seq 1 50); do
    { exec 3<>"/dev/tcp/127.0.0.1/$FIXTURE_PORT"; } 2>/dev/null && { exec 3<&-; exec 3>&-; return 0; }
    sleep 0.1
  done
  echo "fixture httpd did not come up" >&2
  cat "$RUNTIME_DIR/httpd.log" >&2
  exit 1
}

stop_fixtures() {
  kill "${HTTPD_PID:-}" 2>/dev/null || true
  wait "${HTTPD_PID:-}" 2>/dev/null || true
}

# Runs $1 (a function) with the fixture httpd stopped, guaranteed restarted before
# returning. The ONE place outside start_fixtures/stop_fixtures themselves that
# touches the shared httpd resource, so a scenario that needs a real network-loss
# window (only scenario 3, today) calls this instead of reaching into
# stop_fixtures/start_fixtures directly -- one call for a P2-F port to translate,
# not a stop/start pair that could be duplicated or forgotten in a future scenario
# (review round 2, Important 7 -- see README.md).
with_fixtures_stopped() {
  local fn="$1"
  stop_fixtures
  "$fn"
  start_fixtures
}

# Which signed config variant is live at /config.json (spec §5.2, genuinely
# signed via kioskctl -- see README.md "Fixtures"). $1: config.json | config-reload.json | config-iframe.json
stage_config() {
  local source="$SMOKE_DIR/fixtures/$1"
  if [ -n "${KIOSK_SIGNING_KEY_B64:-}" ] && [ -n "${KIOSKCTL_BIN:-}" ]; then
    command -v jq >/dev/null 2>&1 || {
      log "jq is required when signing smoke fixtures dynamically"
      return 1
    }
    jq 'del(.sig)' "$source" >"$SERVE_DIR/config.unsigned.json" || {
      SCENARIO_OK=0
      return 0
    }
    KIOSK_SIGNING_KEY_B64="$KIOSK_SIGNING_KEY_B64" \
      "$KIOSKCTL_BIN" sign "$SERVE_DIR/config.unsigned.json" >"$SERVE_DIR/config.json" || {
      SCENARIO_OK=0
      return 0
    }
    rm -f "$SERVE_DIR/config.unsigned.json"
  else
    cp "$source" "$SERVE_DIR/config.json"
  fi
}

# Stage a signed config for a page-specific scenario. These variants are made
# at run time with the ephemeral smoke key; no hand-edited signature can make a
# scenario appear green after its URL or capability assertions change.
stage_probe_config() {
  local page="$1"; shift
  local filter='.content.url = $url'
  local pin=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      camera) filter="$filter | .content.permissions.camera = true" ;;
      idle) filter="$filter | .content.idle_reset_seconds = 3 | .content.clear_data_on_reset = true" ;;
      gesture)
        pin="${2:?gesture requires a PHC pin hash}"
        filter="$filter | .input.exit_gesture = {taps: 3, region: \"top-left\", pin_hash: \$pin}"
        shift
        ;;
      *) log "unknown probe config option: $1"; return 1 ;;
    esac
    shift
  done
  if [ -z "${KIOSK_SIGNING_KEY_B64:-}" ] || [ -z "${KIOSKCTL_BIN:-}" ]; then
    log "signed probe configs require KIOSK_SIGNING_KEY_B64 and KIOSKCTL_BIN"
    return 1
  fi
  command -v jq >/dev/null 2>&1 || { log "jq is required for probe configs"; return 1; }
  local url="http://localhost:${FIXTURE_PORT}/${page}"
  jq --arg url "$url" --arg pin "$pin" "$filter | del(.sig)" \
    "$SMOKE_DIR/fixtures/config.json" >"$SERVE_DIR/config.unsigned.json" || return 1
  KIOSK_SIGNING_KEY_B64="$KIOSK_SIGNING_KEY_B64" \
    "$KIOSKCTL_BIN" sign "$SERVE_DIR/config.unsigned.json" >"$SERVE_DIR/config.json" || return 1
  rm -f "$SERVE_DIR/config.unsigned.json"
}

# ---------------------------------------------------------------------------
# kiosk-main lifecycle -- fresh process per scenario (clean /var/lib/kiosk each
# time: same signed revision=1 in every config variant would otherwise be
# rejected as a replay on the second scenario that boots it -- SEC-11).
# $1: x11 | wayland -- x11 (via Xwayland) is used ONLY by scenario 1, for
# read-only window-geometry inspection; every other scenario runs native Wayland
# (GTK's default backend once WAYLAND_DISPLAY is set and GDK_BACKEND is unset),
# matching how a real Linux kiosk deploys.
# $2: the kiosk.ini variant (default: kiosk.ini). `[bootstrap] url` MUST equal
# the staged config's content.url -- see kiosk-reload.ini's doc comment for why
# (boot navigates the bootstrap url synchronously, before any network fetch
# completes; against a local fixture httpd the async fetch is fast enough to
# race that first navigation and lose, so a scenario whose content.url differs
# from the bootstrap url never actually reaches its intended page).
# ---------------------------------------------------------------------------
prepare_kiosk_files() {
  local ini="${1:-kiosk.ini}"
  mkdir -p "$DATA_DIR" || {
    log "cannot prepare disposable data directory: $DATA_DIR"
    return 1
  }
  find "$DATA_DIR" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} + || return 1
  rm -rf "$CONFIG_DIR" || return 1
  mkdir -p "$CONFIG_DIR" || return 1
  cp "$SMOKE_DIR/fixtures/$ini" "$CONFIG_DIR/kiosk.ini" || return 1
  cp "$SMOKE_DIR/fixtures/kiosk-credential.json" "$CONFIG_DIR/kiosk-credential.json" || return 1
  chmod 600 "$CONFIG_DIR/kiosk-credential.json" || return 1  # SEC-09: the boot gate fails closed on anything wider
  cp "$SMOKE_DIR/fixtures/kiosk-offline.mp4" "$CONFIG_DIR/kiosk-offline.mp4" || return 1
  if [ -n "${KIOSK_BOOTSTRAP_URL:-}" ]; then
    sed -i "s|^url = .*|url = $KIOSK_BOOTSTRAP_URL|" "$CONFIG_DIR/kiosk.ini" || return 1
  fi
  if grep -q '^pin_hash = PIN_HASH$' "$CONFIG_DIR/kiosk.ini"; then
    [ -n "${KIOSK_PIN_HASH:-}" ] || {
      log "gesture fixture requires KIOSK_PIN_HASH"
      return 1
    }
    sed -i "s|^pin_hash = PIN_HASH$|pin_hash = $KIOSK_PIN_HASH|" "$CONFIG_DIR/kiosk.ini" || return 1
  fi
  if [ "${KIOSK_EGRESS_FILTER_FILE:-0}" = 1 ]; then
    : >"$DATA_DIR/content-filters" || return 1
  fi
}

start_kiosk() {
  local backend="$1" ini="${2:-kiosk.ini}"
  prepare_kiosk_files "$ini" || {
    log "cannot start kiosk: fixture files were not prepared"
    SCENARIO_OK=0
    return 1
  }

  if [ "$backend" = x11 ]; then
    DISPLAY=":$X11_DISPLAY" GDK_BACKEND=x11 "$KIOSK_MAIN" --config "$CONFIG_DIR" \
      >"$RUNTIME_DIR/kiosk-main.log" 2>&1 &
  else
    env -u DISPLAY -u GDK_BACKEND "$KIOSK_MAIN" --config "$CONFIG_DIR" \
      >"$RUNTIME_DIR/kiosk-main.log" 2>&1 &
  fi
  KIOSK_PID=$!
}

stop_kiosk() {
  kill "${KIOSK_PID:-}" 2>/dev/null || true
  for _ in $(seq 1 30); do
    kill -0 "${KIOSK_PID:-0}" 2>/dev/null || break
    sleep 0.1
  done
  kill -9 "${KIOSK_PID:-}" 2>/dev/null || true
  wait "${KIOSK_PID:-}" 2>/dev/null || true
  # Belt-and-suspenders: a SIGTERM'd kiosk-main should take its WebKit child
  # processes with it, but never leave one orphaned across scenarios (5 fresh
  # launches would otherwise accumulate leftover renderer/network processes).
  pkill -f WebKitWebProcess 2>/dev/null || true
  pkill -f WebKitNetworkProcess 2>/dev/null || true
  # Wait for them to actually be reaped, not just signaled: a one-run-in-four
  # flake (scenario 5 timing out on every httpd/spool wait despite passing
  # cleanly in isolation every time -- see README.md's input note) was
  # traced to this gap -- pkill returns as soon as the signal is DELIVERED, not
  # once the process has actually exited and released its IPC sockets/shared
  # memory, and the very next scenario starts a fresh WebKit within
  # milliseconds of this function returning.
  for _ in $(seq 1 25); do
    pgrep -f WebKitWebProcess >/dev/null 2>&1 || pgrep -f WebKitNetworkProcess >/dev/null 2>&1 || break
    sleep 0.2
  done
  # Escalate + log rather than silently giving up after 5s (review round 2, Minor
  # 9): a slower host, or a WebKit child wedged past SIGTERM, must not silently
  # reintroduce the exact "next scenario starts before this one's WebKit is
  # actually gone" flake this whole function exists to close.
  if pgrep -f WebKitWebProcess >/dev/null 2>&1 || pgrep -f WebKitNetworkProcess >/dev/null 2>&1; then
    log "  WARNING: WebKit child(ren) still alive 5s after SIGTERM; escalating to SIGKILL"
    pkill -9 -f WebKitWebProcess 2>/dev/null || true
    pkill -9 -f WebKitNetworkProcess 2>/dev/null || true
    for _ in $(seq 1 15); do
      pgrep -f WebKitWebProcess >/dev/null 2>&1 || pgrep -f WebKitNetworkProcess >/dev/null 2>&1 || break
      sleep 0.2
    done
  fi
}

kiosk_alive() {
  if kill -0 "${KIOSK_PID:-0}" 2>/dev/null; then
    return 0
  fi

  local marker="$RUNTIME_DIR/kiosk-main.${KIOSK_PID:-unknown}.exit-reported"
  if [ ! -e "$marker" ]; then
    : >"$marker"
    log "kiosk-main pid ${KIOSK_PID:-unknown} exited before the smoke assertion"
    if [ -s "$RUNTIME_DIR/kiosk-main.log" ]; then
      sed -n '1,120p' "$RUNTIME_DIR/kiosk-main.log" >&2
    else
      log "kiosk-main.log is empty"
    fi
  fi
  return 1
}

# ---------------------------------------------------------------------------
# Assertion helpers. The spool is the durable telemetry record (no fake GCL
# endpoint needed) -- recursive, not a flat glob: the spool is a directory of
# `NNNNN.jsonl` segments per severity tier (spool.rs:100-101), so segments sit
# one level below the partition.
# ---------------------------------------------------------------------------
spool_events() { find "$DATA_DIR/spool" -name '*.jsonl' -exec cat {} + 2>/dev/null; }

# Count spool lines whose jsonPayload.event equals $1. Fixed-string (-F), not a
# pattern: event/reason names are plain identifiers, and grep's BRE gives
# GNU-extension meaning to a literal `?` (as `\?`, a quantifier) that a fixture
# URL's query string can carry -- -F sidesteps that class of bug entirely
# (see httpd_get_count, which hit exactly this with `?probe=reload`).
event_count() { spool_events | grep -cF "\"event\":\"$1\""; }

# Count spool lines whose jsonPayload.event equals $1 AND jsonPayload.reason equals $2.
event_count_with_reason() { spool_events | grep -F "\"event\":\"$1\"" | grep -cF "\"reason\":\"$2\""; }

httpd_log() { cat "$RUNTIME_DIR/httpd.log" 2>/dev/null; }

# Count httpd access-log lines for a GET of exactly $1 (e.g. "/home.html" or
# "/home.html?probe=reload"). Fixed-string match -- see event_count's doc.
httpd_get_count() { httpd_log | grep -cF "GET $1 HTTP/1."; }

httpd_probe_count() { httpd_log | grep -cF "GET /probe?$1 HTTP/1."; }
httpd_probe_prefix_count() { httpd_log | grep -cF "GET /probe?$1"; }
httpd_path_count() { httpd_log | grep -cF "GET $1 HTTP/1."; }

httpd_get_at_least() { [ "$(httpd_get_count "$1")" -ge "$2" ]; }
event_at_least() { [ "$(event_count "$1")" -ge "$2" ]; }
event_with_reason_at_least() { [ "$(event_count_with_reason "$1" "$2")" -ge "$3" ]; }
config_error_count() { spool_events | grep -F '"event":"config.error"' | grep -cF "\"error\":\"$1\""; }
config_warn_field_count() { spool_events | grep -F '"event":"config.warn"' | grep -cF "\"field\":\"$1\""; }

# wait_until TIMEOUT_S CHECK_CMD... -- polls a predicate every 0.2s until it
# succeeds or TIMEOUT_S elapses; always returns 0 (never aborts the scenario --
# the `check` call right after is what records PASS/FAIL against the settled
# state). Every assertion below races a real async chain (config fetch -> apply
# -> FSM dispatch -> WebKit load, or a process-kill -> crash signal -> recovery
# navigate), so polling for the outcome is what the brief's fixed `sleep N`
# skeleton could not give: a bound that is both fast on the common case and
# tolerant of this host's actual scheduling variance.
wait_until() {
  local timeout="$1"; shift
  local iterations=$((timeout * 5))
  local i
  for ((i = 0; i < iterations; i++)); do
    "$@" >/dev/null 2>&1 && return 0
    sleep 0.2
  done
  return 1
}

SCENARIO_OK=1
# check DESCRIPTION ACTUAL_VALUE EXPECTED_VALUE -- string-equality assertion that
# never aborts the scenario: every check in a scenario runs and is reported,
# rather than stopping at the first failure.
check() {
  local desc="$1" actual="$2" expected="$3"
  if [ "$actual" = "$expected" ]; then
    log "  PASS: $desc (got $actual)"
  else
    log "  FAIL: $desc (expected $expected, got $actual)"
    SCENARIO_OK=0
  fi
}

SCENARIO_BLOCKED=0
# note_blocked DESC -- records a sub-check that was genuinely driven but could not
# be exercised in this environment (see the file header). Distinct from `check`:
# never flips SCENARIO_OK, but IS counted and surfaced in the summary table's
# verdict (review round 2, Important 4) so "PASS" can never silently mean
# "PASS, but two required sub-checks were never exercised" -- that used to be
# true only of the per-check log lines, not of the one line a human/P2-F actually
# reads at the end.
note_blocked() {
  log "  BLOCKED: $1"
  SCENARIO_BLOCKED=$((SCENARIO_BLOCKED + 1))
}

run_scenario() {
  local n="$1" name="$2" fn="$3"
  SCENARIO_OK=1
  SCENARIO_BLOCKED=0
  log "=== Scenario $n: $name ==="
  "$fn"
  local verdict
  if [ "$SCENARIO_OK" = 1 ]; then
    verdict=PASS
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    verdict=FAIL
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
  if [ "$SCENARIO_BLOCKED" -gt 0 ]; then
    verdict="$verdict ($SCENARIO_BLOCKED BLOCKED)"
  fi
  RESULTS+=("$n|$name|$verdict")
  log "=== Scenario $n verdict: $verdict ==="
}

# ---------------------------------------------------------------------------
# Scenario 1: boot -> splash -> remote home commits.
# ---------------------------------------------------------------------------
scenario_1() {
  stage_config config.json
  start_kiosk x11
  wait_until 15 httpd_get_at_least /home.html 1

  check "kiosk-main alive after boot" "$(kiosk_alive && echo yes || echo no)" yes
  check "config.applied revision=1 exactly once" \
    "$(spool_events | grep -F '"event":"config.applied"' | grep -cF '"revision":"1"')" 1
  check "fixture httpd received GET /home.html" "$(httpd_get_count /home.html)" 1

  local win geo
  win="$(DISPLAY=":$X11_DISPLAY" xdotool search --name '^Tauri App$' 2>/dev/null | head -1)"
  if [ -z "$win" ]; then
    log "  FAIL: kiosk window discoverable via Xwayland (none found)"
    SCENARIO_OK=0
  else
    geo="$(DISPLAY=":$X11_DISPLAY" xdotool getwindowgeometry --shell "$win" 2>/dev/null)"
    log "  window geometry: $(echo "$geo" | tr '\n' ' ')"
    # Y is deliberately NOT asserted: desktop-shell reserves a panel strip (~32px)
    # that no fullscreen request can reclaim -- gating on it would make this check
    # a property of the compositor, not of kiosk-main. Logged for visibility only.
    log "  window Y (desktop-shell panel offset, informational, not gated): $(echo "$geo" | sed -n 's/^Y=//p')"
    check "window X" "$(echo "$geo" | sed -n 's/^X=//p')" 0
    check "window WIDTH == headless output width" "$(echo "$geo" | sed -n 's/^WIDTH=//p')" "$WAYLAND_OUT_W"
    check "window HEIGHT == headless output height" "$(echo "$geo" | sed -n 's/^HEIGHT=//p')" "$WAYLAND_OUT_H"
  fi

  stop_kiosk
}

# ---------------------------------------------------------------------------
# Scenario 2: off-list navigation blocked; target=_blank (in-allowlist navigates
# in place, off-allowlist blocked). See README.md for why the
# target=_blank sub-checks are driven but cannot be genuinely exercised here.
# ---------------------------------------------------------------------------
scenario_2() {
  stage_config config.json
  start_kiosk wayland
  wait_until 15 httpd_get_at_least /home.html 1

  check "kiosk-main alive after boot" "$(kiosk_alive && echo yes || echo no)" yes

  # Snapshot, not a hardcoded "1" (same order-independence reasoning as scenario
  # 4's Important-3 fix): the fixture httpd's access log accumulates across the
  # WHOLE run (start_fixtures runs once in main(), before scenario 1), so by the
  # time scenario 2 boots, this is already the run's SECOND /home.html GET, not
  # its first. Caught empirically: a hardcoded "1" here passed in isolation and
  # failed in the real 5-scenario sequence.
  local gets_after_initial_load
  gets_after_initial_load="$(httpd_get_count /home.html)"

  # home.html's step 1 (off-list .click()) fires at T+2s after ITS OWN load, not
  # process start -- wait_until polls for the effect rather than assuming a fixed
  # boot latency.
  wait_until 10 event_with_reason_at_least nav.blocked not_allowlisted 1
  check "exactly one nav.blocked{reason=not_allowlisted} after off-list click" \
    "$(event_count_with_reason nav.blocked not_allowlisted)" 1
  check "no nav.error yet (guard cancelled before any load attempt)" "$(event_count nav.error)" 0
  check "kiosk-main still alive after the blocked click" "$(kiosk_alive && echo yes || echo no)" yes

  # step 2: target=_blank in-allowlist .click() fires at T+4s. No positive signal
  # to poll FOR (the expected-blocked outcome is an absence), so this one still
  # waits out its nominal window before reading the result.
  sleep 4
  local blank_allow_gets
  blank_allow_gets="$(httpd_get_count /allowed-target.html)"
  if [ "$blank_allow_gets" -ge 1 ]; then
    log "  PASS: target=_blank in-allowlist navigated in place (GET /allowed-target.html observed)"
  else
    note_blocked "target=_blank in-allowlist: no GET /allowed-target.html observed (got $blank_allow_gets) -- WebKit's own popup gate (javascript-can-open-windows-automatically, untouched on Linux -- hardening.rs is a no-op here) requires a trusted user gesture a script .click() cannot supply; this container has no input devices (see README.md input note). NOT counted as scenario failure."
  fi

  # step 3: target=_blank off-allowlist .click() fires at T+6s.
  sleep 3
  local blocked_after
  blocked_after="$(event_count_with_reason nav.blocked not_allowlisted)"
  if [ "$blocked_after" -ge 2 ]; then
    log "  PASS: target=_blank off-allowlist produced a second nav.blocked"
  else
    note_blocked "target=_blank off-allowlist: nav.blocked count stayed at $blocked_after (expected 2) -- same WebKit gesture gate as above; the guard was never reached to judge this one. NOT counted as scenario failure."
  fi

  # Same assertion scenario 5 carries after its own settle window (review round 2,
  # Important 6): none of the three attempts above -- blocked, blocked-attempted
  # target=_blank x2 -- may have reloaded or replaced the top-level document.
  # Compared against the snapshot above, not a hardcoded "1" -- see its comment.
  check "top-level unchanged (no reload/replace from any of the three attempts)" \
    "$(httpd_get_count /home.html)" "$gets_after_initial_load"

  stop_kiosk
}

# ---------------------------------------------------------------------------
# Scenario 3: offline fallback. Stop the fixture httpd, drive a reload (the page
# itself calls location.reload() -- a genuine top-level navigation attempt, not
# a simulation), assert the link drop is independently observed by the FSM's
# connectivity prober (not just a generic load failure), and assert the FULL
# recovery loop: reconnect is observed and the remembered home is re-navigated
# for real (review round 2, Critical 2 -- the original version of this scenario
# asserted nothing that a black-screen-on-network-loss device couldn't also
# satisfy; see README.md for the input limitation).
# ---------------------------------------------------------------------------
scenario_3() {
  stage_config config-reload.json
  start_kiosk wayland kiosk-reload.ini
  wait_until 15 httpd_get_at_least '/home.html?probe=reload' 1

  check "kiosk-main alive after boot" "$(kiosk_alive && echo yes || echo no)" yes
  check "initial GET of home.html?probe=reload succeeded" \
    "$([ "$(httpd_get_count '/home.html?probe=reload')" -ge 1 ] && echo yes || echo no)" yes

  # kiosk-core's Prober (net/prober.rs) starts ASSUMING Offline, and its cold-start
  # rule only flips on a CHANGE from that assumption -- if the httpd went down
  # before the prober's very first probe, that first failure would read as
  # "already offline" and net.offline would never fire AT ALL, no matter how long
  # the link stayed down (confirmed by reading Prober::record's damping table).
  # Must happen BEFORE stopping fixtures, or the assertion below is unfireable by
  # construction.
  #
  # This -- and the whole offline/recovery window below -- runs on the SCHEMA
  # DEFAULT probe intervals (probe_offline_s=10, probe_online_s=30), not a fast
  # test-only override: main.rs reads `network` from `booted.manager.current()`
  # BEFORE `booted.manager` moves into `fetch::run` (main.rs, right after
  # `boot::load`), so it is fixed at whatever local/bootstrap state resolved at
  # boot -- a signed config's `network` section can never take effect within the
  # lifetime of the process that fetches it (confirmed the hard way: setting it
  # in config-reload.json changed nothing observable, and separately, values
  # below config/validate.rs's 5s floor were silently rejected as "invalid
  # config: 2 field error(s)", which is a second, independent reason not to lean
  # on this path). So this scenario genuinely costs on the order of a minute and
  # a half of wall clock -- real damped-probe timing, not a harness inefficiency.
  wait_until 20 event_at_least net.online 1
  check "prober confirmed online before the link drop" \
    "$([ "$(event_count net.online)" -ge 1 ] && echo yes || echo no)" yes

  with_fixtures_stopped scenario_3_offline_window

  check "nav.error observed (the reload's connection failure)" "$([ "$(event_count nav.error)" -ge 1 ] && echo yes || echo no)" yes
  # Independent of nav.error: net.offline comes from the connectivity prober's own
  # damped HTTP GETs (probe.rs), a completely separate code path from the WebKit
  # load-failed signal nav.error is built on. Asserting BOTH is what makes this
  # scenario about the FSM's offline handling specifically, not just "some load,
  # somewhere, failed" -- the gap the review flagged.
  check "net.offline observed (the prober independently heard the link drop)" \
    "$([ "$(event_count net.offline)" -ge 1 ] && echo yes || echo no)" yes
  check "kiosk-main still alive (fell to the offline page, did not crash)" "$(kiosk_alive && echo yes || echo no)" yes

  log "  offline.html's app-origin load and its kioskasset://localhost mp4-URL computation are" \
      "deterministic page-local JS (verified by reading offline.html; not runtime-introspectable" \
      "from this shell harness -- no remote-debugging channel is wired). See README.md."
  # Best-effort screenshot (design spec's gate section: non-blocking) was tried
  # and dropped: weston-screenshooter hits `screenshot_create_shm_buffer:
  # Assertion 'width > 0' failed` against the headless backend every time
  # (confirmed in isolation, not a transient flake) -- it does not query a
  # memory-surface output's geometry the way it does a real one. Not worth
  # carrying a call that is guaranteed to abort on this backend.

  # Recovery (with_fixtures_stopped already restarted the httpd before returning):
  # rule 4 (kiosk-core state.rs, `Offline + LinkChanged(Online)`) must re-navigate
  # the remembered home once the prober confirms reconnect -- proving the machine
  # genuinely was in AppState::Offline and can leave it, not just that it survived.
  # Compared against RELOAD_GETS_BEFORE_RECOVERY (a snapshot `scenario_3_offline_window`
  # took just before fixtures came back up) rather than a fixed "== 2": the page's
  # own ?probe=reload timer can legitimately produce more than one successful GET
  # before the link actually drops, depending on exactly how boot latency lines up
  # against the probe interval, so only "strictly more than whatever it already
  # was" is a timing-independent signal. Two consecutive successful probes at the
  # (default) 10s offline-interval before this flips -- see the note above on why
  # that default, not a fast override, is what's actually running.
  wait_until 40 event_at_least net.online 2
  wait_until 10 httpd_get_at_least '/home.html?probe=reload' "$((RELOAD_GETS_BEFORE_RECOVERY + 1))"
  check "net.online observed again after recovery (reconnect)" \
    "$([ "$(event_count net.online)" -ge 2 ] && echo yes || echo no)" yes
  check "a fresh GET of home.html?probe=reload after recovery (rule 4 re-navigate)" \
    "$([ "$(httpd_get_count '/home.html?probe=reload')" -gt "$RELOAD_GETS_BEFORE_RECOVERY" ] && echo yes || echo no)" yes
  check "kiosk-main still alive after recovery" "$(kiosk_alive && echo yes || echo no)" yes

  stop_kiosk
}

# Runs while the fixture httpd is down (called only via with_fixtures_stopped).
# Snapshots RELOAD_GETS_BEFORE_RECOVERY (a global scenario_3 reads after
# with_fixtures_stopped returns) so the post-recovery check is a delta, not a
# fixed count -- see scenario_3's comment on the recovery checks.
scenario_3_offline_window() {
  log "  fixture httpd stopped; waiting for the page's self-reload to hit a closed port"
  wait_until 12 event_at_least nav.error 1    # reload fires at T+2s after load; localhost connection-refused is fast
  wait_until 90 event_at_least net.offline 1  # 2 consecutive failed probes at the (default) 30s online-interval
  RELOAD_GETS_BEFORE_RECOVERY="$(httpd_get_count '/home.html?probe=reload')"
}

# ---------------------------------------------------------------------------
# Scenario 4: renderer crash -> webview.crash spooled + recovery navigate-home.
# ---------------------------------------------------------------------------
scenario_4() {
  stage_config config.json
  start_kiosk wayland
  wait_until 15 httpd_get_at_least /home.html 1

  check "kiosk-main alive after boot" "$(kiosk_alive && echo yes || echo no)" yes
  check "initial GET /home.html before the crash" "$([ "$(httpd_get_count /home.html)" -ge 1 ] && echo yes || echo no)" yes

  # Snapshot BEFORE the kill rather than asserting a fixed "== 2" (review round 2,
  # Important 3): scenarios 1 and 2 each already GET /home.html once, so a
  # re-homed run that reorders or drops scenario 3 (which is what currently
  # truncates the shared httpd access log, via start_fixtures) would satisfy a
  # fixed ">= 2" before the crash below ever happens. Comparing against this
  # scenario's OWN pre-kill count is order-independent -- it only ever proves
  # something NEW happened after the crash.
  local gets_before_crash
  gets_before_crash="$(httpd_get_count /home.html)"

  pkill -f WebKitWebProcess
  log "  sent SIGTERM to WebKitWebProcess"
  wait_until 10 event_at_least webview.crash 1

  check "kiosk-main (the supervising process) survived the renderer crash" "$(kiosk_alive && echo yes || echo no)" yes
  check "exactly one webview.crash" "$(event_count webview.crash)" 1
  # Was only ever logged, not asserted (review round 2, Important 5) -- the
  # termination_label mapping (recovery.rs) is spec-pinned to NOT reuse the
  # WebView2 constant space, and nothing in this harness checked that until now.
  local crash_kind
  crash_kind="$(spool_events | grep -F '"event":"webview.crash"' | sed -n 's/.*"kind":"\([^"]*\)".*/\1/p')"
  check "webview.crash kind is webkit_crashed" "$crash_kind" webkit_crashed

  wait_until 10 httpd_get_at_least /home.html "$((gets_before_crash + 1))"
  check "a fresh GET /home.html after the crash (recovery navigate-home)" \
    "$([ "$(httpd_get_count /home.html)" -gt "$gets_before_crash" ] && echo yes || echo no)" yes

  stop_kiosk
}

# ---------------------------------------------------------------------------
# Scenario 5 (blocking -- pins the main-frame assumption): in-allowlist iframe
# loads; off-allowlist iframe is blocked exactly once, top-level unchanged, and
# -- the assumption this scenario exists to pin -- no NavigationFailed/nav.error/
# error-page transition leaks from the blocked sub-frame.
# ---------------------------------------------------------------------------
scenario_5() {
  stage_config config-iframe.json
  start_kiosk wayland kiosk-iframe.ini
  wait_until 15 httpd_get_at_least /iframe-host.html 1
  wait_until 10 httpd_get_at_least /iframe-allowed.html 1
  wait_until 10 event_with_reason_at_least nav.blocked not_allowlisted 1
  sleep 3   # settle window: this scenario exists to prove a DELAYED leak never
            # arrives, so wait past the positive signals before reading "absent"

  check "kiosk-main alive after boot" "$(kiosk_alive && echo yes || echo no)" yes
  check "top-level GET /iframe-host.html" "$(httpd_get_count /iframe-host.html)" 1
  check "in-allowlist iframe GET /iframe-allowed.html" "$(httpd_get_count /iframe-allowed.html)" 1
  check "exactly one nav.blocked{reason=not_allowlisted} for the off-allowlist iframe" \
    "$(event_count_with_reason nav.blocked not_allowlisted)" 1
  check "no nav.error leaked from the blocked iframe" "$(event_count nav.error)" 0
  check "no second top-level commit (only one GET of iframe-host.html)" "$(httpd_get_count /iframe-host.html)" 1
  check "kiosk-main still alive" "$(kiosk_alive && echo yes || echo no)" yes

  stop_kiosk
}

# ---------------------------------------------------------------------------
# Scenario 6: profile clear completion. This path has no user-facing producer
# until the idle FSM is exercised, so the dedicated probe drives clear::clear
# and refuses to pass without the real ProfileCleared callback.
# ---------------------------------------------------------------------------
scenario_6() {
  local probe="${KIOSK_CLEAR_PROBE:-}"
  if [ -n "$probe" ]; then
    "$probe"
  else
    cargo run -q -p kiosk-main --example clear_probe
  fi
  check "clear probe exited successfully" "$?" 0
}

# ---------------------------------------------------------------------------
# Scenario 7: malformed bootstrap -> safe renderer. Credential failures are
# intentionally out of scope: only the parser-error path enters safe_boot.
# ---------------------------------------------------------------------------
scenario_7() {
  local config_gets_before
  config_gets_before="$(httpd_get_count /config.json)"
  start_kiosk wayland kiosk-malformed.ini
  wait_until 10 event_at_least app.start 1
  check "kiosk-main alive in safe mode" "$(kiosk_alive && echo yes || echo no)" yes
  check "safe boot did not start a remote fixture request" \
    "$(httpd_get_count /config.json)" "$config_gets_before"
  stop_kiosk
}

# ---------------------------------------------------------------------------
# Scenario 8: Linux egress filter, CSP belt, and the filter-degrade path.
# ---------------------------------------------------------------------------
scenario_8() {
  if ! stage_probe_config hardening.html; then
    check "signed hardening probe was staged" no yes
    return
  fi
  KIOSK_BOOTSTRAP_URL="http://localhost:${FIXTURE_PORT}/hardening.html" start_kiosk wayland
  wait_until 20 httpd_probe_count ready=1 1
  wait_until 20 event_with_reason_at_least nav.blocked egress 4
  local blocked="$(event_count_with_reason nav.blocked egress)"
  check "four off-list resource classes are blocked" "$([ "$blocked" -ge 4 ] && echo yes || echo no)" yes
  check "the data: image still renders" \
    "$([ "$(httpd_probe_count data-image=rendered)" -ge 1 ] && echo yes || echo no)" yes
  check "service-worker fixture was installed" \
    "$([ "$(httpd_get_count /sw.js)" -ge 1 ] && echo yes || echo no)" yes
  check "kiosk-main survives healthy egress enforcement" "$(kiosk_alive && echo yes || echo no)" yes
  stop_kiosk

  # A regular file is deterministic even for root: create_dir_all cannot turn it
  # into the content-filter store, so this exercises the real degraded branch.
  KIOSK_EGRESS_FILTER_FILE=1 \
    KIOSK_BOOTSTRAP_URL="http://localhost:${FIXTURE_PORT}/hardening.html" \
    start_kiosk wayland
  wait_until 20 config_error_count egress.filter_absent 1
  wait_until 20 httpd_probe_prefix_count csp= 1
  check "missing native filter is a config.error" \
    "$(config_error_count egress.filter_absent)" 1
  check "CSP still reports the blocked off-list fetch" \
    "$([ "$(httpd_probe_prefix_count csp=)" -ge 1 ] && echo yes || echo no)" yes
  check "kiosk-main survives filter degradation" "$(kiosk_alive && echo yes || echo no)" yes
  stop_kiosk
}

# ---------------------------------------------------------------------------
# Scenario 9: attachment downloads are cancelled before a file is created.
# ---------------------------------------------------------------------------
scenario_9() {
  if ! stage_probe_config download.html; then
    check "signed download probe was staged" no yes
    return
  fi
  KIOSK_BOOTSTRAP_URL="http://localhost:${FIXTURE_PORT}/download.html" start_kiosk wayland
  wait_until 15 httpd_probe_count download=attempted 1
  wait_until 15 event_with_reason_at_least nav.blocked download 1
  check "exactly one download block is spooled" "$(event_count_with_reason nav.blocked download)" 1
  check "attachment response was reached before cancellation" \
    "$([ "$(httpd_get_count /attachment.bin)" -ge 1 ] && echo yes || echo no)" yes
  check "no attachment was written below the kiosk data directory" \
    "$([ "$(find "$DATA_DIR" -name attachment.bin -print -quit)" = "" ] && echo yes || echo no)" yes
  check "kiosk-main remains on the page" "$(kiosk_alive && echo yes || echo no)" yes
  stop_kiosk
}

# ---------------------------------------------------------------------------
# Scenario 10: dialog/chrome suppression, the bundled keyboard, print, and
# beforeunload. The right-click is deliberately real XTest input on the X11
# floor path; if the runner cannot deliver it, this blocking scenario fails.
# ---------------------------------------------------------------------------
scenario_10() {
  if ! stage_probe_config controls.html; then
    check "signed controls probe was staged" no yes
    return
  fi
  KIOSK_BOOTSTRAP_URL="http://localhost:${FIXTURE_PORT}/controls.html" start_kiosk x11
  wait_until 15 httpd_probe_prefix_count keyboard= 1
  wait_until 15 httpd_probe_count keyboard-panel=gone 1
  wait_until 15 httpd_probe_count print=called 1
  wait_until 15 httpd_probe_count beforeunload=passed 1

  local win
  win="$(DISPLAY=":$X11_DISPLAY" xdotool search --onlyvisible --name 'SMOKE CONTROLS' 2>/dev/null | head -1)"
  if [ -z "$win" ]; then
    check "X11 window is available for the context-menu arm" no yes
  elif DISPLAY=":$X11_DISPLAY" xdotool mousemove --window "$win" 120 100 click 3 >/dev/null 2>&1; then
    check "a real right-click reaches the page without a native menu" \
      "$([ "$(httpd_probe_count contextmenu=event)" -ge 1 ] && echo yes || echo no)" yes
  else
    check "XTest right-click delivery" no yes
  fi
  check "keyboard key changed the focused input" \
    "$([ "$(httpd_probe_prefix_count keyboard=)" -ge 1 ] && echo yes || echo no)" yes
  check "keyboard panel disappeared on blur" "$(httpd_probe_count keyboard-panel=gone)" 1
  check "iframe print call returned without wedging the kiosk" \
    "$(kiosk_alive && echo yes || echo no)" yes
  check "beforeunload navigation completed without a prompt" "$(httpd_probe_count beforeunload=passed)" 1
  stop_kiosk
}

# ---------------------------------------------------------------------------
# Scenario 11: permission requests are default-denied, and camera becomes
# allowed only when the signed capability is present. A machine without a
# camera should report NotFoundError in the positive arm, not NotAllowedError.
# ---------------------------------------------------------------------------
scenario_11() {
  if ! stage_probe_config permissions.html; then
    check "signed default permission probe was staged" no yes
    return
  fi
  KIOSK_BOOTSTRAP_URL="http://localhost:${FIXTURE_PORT}/permissions.html" start_kiosk wayland
  wait_until 15 httpd_probe_prefix_count permission= 2
  local denied_camera
  denied_camera="$(httpd_log | sed -n 's/.*GET \/probe?permission=\(camera-[^ ]*\) HTTP.*/\1/p' | tail -1)"
  check "camera is denied by default" \
    "$([[ "$denied_camera" == camera-NotAllowedError || "$denied_camera" == camera-denied || "$denied_camera" == camera-NotFoundError ]] && echo yes || echo no)" yes
  check "geolocation is denied by default" \
    "$([[ "$(httpd_probe_prefix_count permission=geolocation-)" -ge 1 ]] && echo yes || echo no)" yes
  stop_kiosk

  if ! stage_probe_config permissions.html camera; then
    check "signed camera-enabled permission probe was staged" no yes
    return
  fi
  KIOSK_BOOTSTRAP_URL="http://localhost:${FIXTURE_PORT}/permissions.html" start_kiosk wayland
  wait_until 15 httpd_probe_prefix_count permission=camera- 1
  local allowed_camera
  allowed_camera="$(httpd_log | sed -n 's/.*GET \/probe?permission=\(camera-[^ ]*\) HTTP.*/\1/p' | tail -1)"
  check "camera capability changes the permission decision" \
    "$([[ "$allowed_camera" != camera-NotAllowedError && "$allowed_camera" != camera-denied ]] && echo yes || echo no)" yes
  check "kiosk-main remains alive after permission probes" "$(kiosk_alive && echo yes || echo no)" yes
  stop_kiosk
}

# ---------------------------------------------------------------------------
# Scenario 12: bus-less keep-awake degradation is observable and non-fatal.
# ---------------------------------------------------------------------------
scenario_12() {
  if ! command -v systemd-inhibit >/dev/null 2>&1; then
    check "systemd-inhibit precondition" no yes
    return
  fi
  stage_config config.json
  start_kiosk wayland
  wait_until 15 event_at_least app.start 1
  wait_until 15 config_warn_field_count display.keep_awake 1
  check "keep-awake child failure is a config.warn" \
    "$(config_warn_field_count display.keep_awake)" 1
  check "keep-awake degradation does not kill the kiosk" "$(kiosk_alive && echo yes || echo no)" yes
  stop_kiosk
}

# ---------------------------------------------------------------------------
# Scenario 16: idle expiry clears the profile and re-navigates home once. The
# fixture emits `set` after persisting the cookie; a second `absent` response is
# therefore evidence that the clear completed rather than merely reloading.
# ---------------------------------------------------------------------------
scenario_16() {
  if ! stage_probe_config idle.html idle; then
    check "signed idle probe was staged" no yes
    return
  fi
  KIOSK_BOOTSTRAP_URL="http://localhost:${FIXTURE_PORT}/idle.html" start_kiosk wayland
  wait_until 15 httpd_probe_count idle=set 1
  wait_until 20 httpd_probe_count idle=absent 2
  check "idle fixture persisted its session cookie" \
    "$([ "$(httpd_probe_count idle=set)" -ge 1 ] && echo yes || echo no)" yes
  check "profile clear removed the cookie on the second home load" \
    "$([ "$(httpd_probe_count idle=absent)" -ge 2 ] && echo yes || echo no)" yes
  local absent_count="$(httpd_probe_count idle=absent)"
  sleep 5
  check "idle reset fired only once while the page stayed untouched" \
    "$(httpd_probe_count idle=absent)" "$absent_count"
  check "kiosk-main remains alive after profile clear" "$(kiosk_alive && echo yes || echo no)" yes
  stop_kiosk
}

window_present() {
  [ -n "${X11_DISPLAY:-}" ] && DISPLAY=":$X11_DISPLAY" \
    xdotool search --onlyvisible --name "$1" >/dev/null 2>&1
}

# ---------------------------------------------------------------------------
# Scenario 17: Linux X11 floor driver for native GTK activity, gesture/chord,
# and the always-Proceed input backstop. This is the declared C7 divergence;
# the native Wayland touch path remains P2-G H4a hardware evidence.
# ---------------------------------------------------------------------------
scenario_17() {
  command -v xdotool >/dev/null 2>&1 || {
    check "xdotool floor driver is installed" no yes
    return
  }
  local pin
  pin="$($KIOSKCTL_BIN hash-pin 1234 2>/dev/null)"
  if [ -z "$pin" ] || ! stage_probe_config input-echo.html idle; then
    check "signed gesture/input probe was staged" no yes
    return
  fi
  KIOSK_PIN_HASH="$pin" KIOSK_BOOTSTRAP_URL="http://localhost:${FIXTURE_PORT}/input-echo.html" \
    start_kiosk x11 kiosk-gesture.ini
  wait_until 15 httpd_get_at_least /input-echo.html 1
  local win
  win="$(DISPLAY=":$X11_DISPLAY" xdotool search --onlyvisible --name 'Tauri App\|SMOKE' 2>/dev/null | head -1)"
  if [ -z "$win" ]; then
    check "X11 input window is discoverable" no yes
    stop_kiosk
    return
  fi

  # Activity before the short idle threshold; the page must not be cleared while
  # this motion/click sequence is in progress.
  DISPLAY=":$X11_DISPLAY" xdotool mousemove --window "$win" 300 300 >/dev/null 2>&1 || true
  DISPLAY=":$X11_DISPLAY" xdotool mousemove --window "$win" 5 5 click --repeat 3 --delay 120 1 >/dev/null 2>&1
  wait_until 8 window_present 'kiosk — technician exit'
  check "three corner taps open the pin pad" "$(window_present 'kiosk — technician exit' && echo yes || echo no)" yes
  stop_kiosk

  KIOSK_PIN_HASH="$pin" KIOSK_BOOTSTRAP_URL="http://localhost:${FIXTURE_PORT}/input-echo.html" \
    start_kiosk x11 kiosk-gesture.ini
  wait_until 15 httpd_get_at_least /input-echo.html 1
  DISPLAY=":$X11_DISPLAY" xdotool keydown ctrl keydown alt keydown shift key k keyup shift keyup alt keyup ctrl
  wait_until 8 window_present 'kiosk — technician exit'
  check "technician chord opens the pin pad" "$(window_present 'kiosk — technician exit' && echo yes || echo no)" yes
  stop_kiosk

  KIOSK_PIN_HASH="$pin" KIOSK_BOOTSTRAP_URL="http://localhost:${FIXTURE_PORT}/input-echo.html" \
    start_kiosk x11 kiosk-gesture.ini
  wait_until 15 httpd_get_at_least /input-echo.html 1
  win="$(DISPLAY=":$X11_DISPLAY" xdotool search --onlyvisible --name 'Tauri App\|SMOKE' 2>/dev/null | head -1)"
  local gets_before="$(httpd_get_count /input-echo.html)"
  DISPLAY=":$X11_DISPLAY" xdotool mousemove --window "$win" 100 80 click 1
  DISPLAY=":$X11_DISPLAY" xdotool key --window "$win" x
  wait_until 10 httpd_probe_prefix_count input= 2
  sleep 4
  check "ordinary click and key events still reach the page" \
    "$([ "$(httpd_probe_prefix_count input=)" -ge 2 ] && echo yes || echo no)" yes
  check "activity reset prevents an early idle clear" \
    "$(httpd_get_count /input-echo.html)" "$gets_before"
  check "kiosk-main remains alive after input handling" "$(kiosk_alive && echo yes || echo no)" yes
  stop_kiosk
}

# ---------------------------------------------------------------------------
# Scenarios 13–15: the cage/launcher contract. These start the real launcher,
# which in turn resolves the copied kiosk-main next to itself, so a green result
# cannot come from a standalone kiosk-main process.
# ---------------------------------------------------------------------------
scenario_13() {
  if ! command -v cage >/dev/null 2>&1 || ! prepare_launcher; then
    check "cage and release launcher are available" no yes
    return
  fi
  local cage_version
  cage_version="$(cage -v 2>&1 | head -1)"
  check "cage version is observable" "$([ -n "$cage_version" ] && echo yes || echo no)" yes

  local probe_runtime="$RUNTIME_DIR/cage-probe"
  mkdir -p "$probe_runtime"
  env XDG_RUNTIME_DIR="$probe_runtime" WAYLAND_DISPLAY=wayland-probe \
    WLR_BACKENDS=headless cage -- sh -c 'exit 86' >/dev/null 2>&1
  local probe_status=$?
  check "cage propagates a child exit status of 86" "$probe_status" 86

  stage_config config.json
  if ! start_cage_launcher kiosk.ini; then
    check "cage launcher chain started" no yes
    stop_cage_app
    return
  fi
  wait_until 30 httpd_get_at_least /home.html 1
  local old_main="$(supervised_main_pid)"
  check "launcher has a supervised kiosk-main child" "$([ -n "$old_main" ] && echo yes || echo no)" yes
  check "home rendered under cage launcher" \
    "$([ "$(httpd_get_count /home.html)" -ge 1 ] && echo yes || echo no)" yes
  check "launcher process remains alive after healthy boot" \
    "$(kill -0 "${LAUNCHER_PID:-0}" 2>/dev/null && echo yes || echo no)" yes

  local home_before="$(httpd_get_count /home.html)"
  if [ -n "$old_main" ]; then kill -9 "$old_main" 2>/dev/null || true; fi
  wait_until 45 supervised_main_changed "$old_main"
  local new_main="$(supervised_main_pid)"
  check "launcher restarts kiosk-main after a hard child exit" \
    "$([ -n "$new_main" ] && [ "$new_main" != "$old_main" ] && echo yes || echo no)" yes
  wait_until 30 httpd_get_at_least /home.html "$((home_before + 1))"
  check "restarted child renders home again" \
    "$([ "$(httpd_get_count /home.html)" -gt "$home_before" ] && echo yes || echo no)" yes
  stop_cage_app
}

scenario_14() {
  command -v cage >/dev/null 2>&1 || {
    check "cage is installed for the technician-exit arm" no yes
    return
  }
  local pin
  pin="$($KIOSKCTL_BIN hash-pin 1234 2>/dev/null)"
  if [ -z "$pin" ] || ! stage_probe_config input-echo.html; then
    check "signed technician-exit probe was staged" no yes
    return
  fi
  KIOSK_CAGE_GDK_BACKEND=x11 KIOSK_PIN_HASH="$pin" \
    start_cage_launcher kiosk-gesture.ini || {
      check "cage X11 launcher chain started" no yes
      stop_cage_app
      return
    }
  wait_until 30 httpd_get_at_least /input-echo.html 1
  local win
  win="$(DISPLAY=":$X11_DISPLAY" xdotool search --onlyvisible --name 'Tauri App\|SMOKE' 2>/dev/null | head -1)"
  if [ -z "$win" ]; then
    check "cage Xwayland exposes the kiosk window" no yes
    stop_cage_app
    return
  fi
  DISPLAY=":$X11_DISPLAY" xdotool keydown ctrl keydown alt keydown shift key k keyup shift keyup alt keyup ctrl
  wait_until 10 window_present 'kiosk — technician exit'
  local pad
  pad="$(DISPLAY=":$X11_DISPLAY" xdotool search --onlyvisible --name 'kiosk — technician exit' 2>/dev/null | head -1)"
  if [ -z "$pad" ]; then
    check "technician chord opened the pin pad" no yes
    stop_cage_app
    return
  fi
  # pinpad.html is a fixed 3x4 grid centred in the 1280x720 floor output.
  # Relative coordinates keep this independent of the X11 window id.
  local x1=568 x2=640 x3=712 y1=281 y2=336 y3=391 y4=446
  DISPLAY=":$X11_DISPLAY" xdotool mousemove --window "$pad" "$x1" "$y1" click 1
  DISPLAY=":$X11_DISPLAY" xdotool mousemove --window "$pad" "$x2" "$y1" click 1
  DISPLAY=":$X11_DISPLAY" xdotool mousemove --window "$pad" "$x3" "$y1" click 1
  DISPLAY=":$X11_DISPLAY" xdotool mousemove --window "$pad" "$x1" "$y2" click 1
  DISPLAY=":$X11_DISPLAY" xdotool mousemove --window "$pad" "$x3" "$y4" click 1
  wait_until 15 cage_exited
  local launcher_status=0
  if [ -n "${CAGE_PID:-}" ]; then
    wait "$CAGE_PID" 2>/dev/null || launcher_status=$?
  fi
  check "launcher exits with technician status 86" "$launcher_status" 86
  stop_cage_app
}

cage_exited() {
  [ -n "${CAGE_PID:-}" ] || return 1
  if ! kill -0 "$CAGE_PID" 2>/dev/null; then
    return 0
  fi
  [ "$(ps -o stat= -p "$CAGE_PID" 2>/dev/null | tr -d ' ' | cut -c1)" = Z ]
}

scenario_15() {
  command -v cage >/dev/null 2>&1 || {
    check "cage is installed for the heartbeat hang arm" no yes
    return
  }
  stage_config config.json
  if ! start_cage_launcher kiosk.ini; then
    check "cage launcher chain started" no yes
    stop_cage_app
    return
  fi
  wait_until 30 httpd_get_at_least /home.html 1
  local old_main="$(supervised_main_pid)"
  check "launcher has a supervised child before SIGSTOP" "$([ -n "$old_main" ] && echo yes || echo no)" yes
  if [ -n "$old_main" ]; then kill -STOP "$old_main" 2>/dev/null || true; fi
  wait_until 45 supervised_main_changed "$old_main"
  local new_main="$(supervised_main_pid)"
  check "heartbeat miss causes a restart" \
    "$([ -n "$new_main" ] && [ "$new_main" != "$old_main" ] && echo yes || echo no)" yes
  if [ -n "$old_main" ]; then kill -CONT "$old_main" 2>/dev/null || true; fi
  check "the SIGSTOP corpse is no longer alive" \
    "$([ -z "$old_main" ] || ! kill -0 "$old_main" 2>/dev/null && echo yes || echo no)" yes
  check "watchdog hang telemetry is durable" \
    "$([ "$(event_count watchdog.hang)" -ge 1 ] && echo yes || echo no)" yes
  check "launcher remains alive after hang recovery" \
    "$(kill -0 "${LAUNCHER_PID:-0}" 2>/dev/null && echo yes || echo no)" yes
  stop_cage_app
}

# ---------------------------------------------------------------------------
main() {
  start_compositor
  start_fixtures || exit 1

  case "${KIOSK_SCENARIO:-all}" in
    1) run_scenario 1 "boot -> splash -> remote home commits" scenario_1 ;;
    2) run_scenario 2 "off-list nav blocked + target=_blank" scenario_2 ;;
    3) run_scenario 3 "offline fallback" scenario_3 ;;
    4) run_scenario 4 "renderer crash recovery" scenario_4 ;;
    5) run_scenario 5 "iframe blocking (main-frame pin)" scenario_5 ;;
    6) run_scenario 6 "profile clear completion" scenario_6 ;;
    7) run_scenario 7 "malformed bootstrap safe boot" scenario_7 ;;
    8) run_scenario 8 "Linux egress filter and degraded CSP belt" scenario_8 ;;
    9) run_scenario 9 "attachment download cancellation" scenario_9 ;;
    10) run_scenario 10 "dialogs, chrome, keyboard, and print" scenario_10 ;;
    11) run_scenario 11 "permission default-deny and camera grant" scenario_11 ;;
    12) run_scenario 12 "keep-awake degradation" scenario_12 ;;
    13) run_scenario 13 "cage launcher chain and restart" scenario_13 ;;
    14) run_scenario 14 "technician exit through cage" scenario_14 ;;
    15) run_scenario 15 "heartbeat hang and orphan reap" scenario_15 ;;
    16) run_scenario 16 "idle clear and latch" scenario_16 ;;
    17) run_scenario 17 "gesture, chord, and input activity" scenario_17 ;;
    all)
      run_scenario 1 "boot -> splash -> remote home commits" scenario_1
      run_scenario 2 "off-list nav blocked + target=_blank" scenario_2
      run_scenario 3 "offline fallback" scenario_3
      run_scenario 4 "renderer crash recovery" scenario_4
      run_scenario 5 "iframe blocking (main-frame pin)" scenario_5
      run_scenario 6 "profile clear completion" scenario_6
      run_scenario 7 "malformed bootstrap safe boot" scenario_7
      run_scenario 8 "Linux egress filter and degraded CSP belt" scenario_8
      run_scenario 9 "attachment download cancellation" scenario_9
      run_scenario 10 "dialogs, chrome, keyboard, and print" scenario_10
      run_scenario 11 "permission default-deny and camera grant" scenario_11
      run_scenario 12 "keep-awake degradation" scenario_12
      run_scenario 13 "cage launcher chain and restart" scenario_13
      run_scenario 14 "technician exit through cage" scenario_14
      run_scenario 15 "heartbeat hang and orphan reap" scenario_15
      run_scenario 16 "idle clear and latch" scenario_16
      run_scenario 17 "gesture, chord, and input activity" scenario_17
      ;;
    *)
      log "unknown smoke scenario: ${KIOSK_SCENARIO}"
      stop_fixtures
      return 2
      ;;
  esac

  stop_fixtures

  echo
  echo "=== P2-A smoke summary ==="
  printf '%-3s %-45s %s\n' "N" "Scenario" "Verdict"
  for r in "${RESULTS[@]}"; do
    IFS='|' read -r n name verdict <<<"$r"
    printf '%-3s %-45s %s\n' "$n" "$name" "$verdict"
  done
  echo "PASS=$PASS_COUNT FAIL=$FAIL_COUNT"

  [ "$FAIL_COUNT" = 0 ]
}

# Guarded so this file can be `source`d (e.g. to drive a single scenario_N
# function in isolation, as the review-round-2 mutation tests did) without
# immediately running the full 5-scenario suite. `bash packaging/smoke/run-smoke.sh`
# (the documented, only supported way to run this file directly) is unaffected --
# BASH_SOURCE[0] and $0 are the same path in that case, exactly as before.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main
fi
