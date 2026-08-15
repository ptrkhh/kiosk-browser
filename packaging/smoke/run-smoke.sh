#!/usr/bin/env bash
# P2-A smoke harness (merge gate). Scenarios 1-5 (this file's scope; 6-7 are a later
# P2-A task). All blocking. Human-run in-session; deliberately NOT wired into
# ci.yml — automating it is P2-F, which re-homes these scenario bodies into
# crates/kiosk-smoke (F owns A 1-7 · B 8-12 · C 13-15 · D 16-17). Compositor
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
KIOSK_MAIN="${KIOSK_MAIN:-$SMOKE_DIR/../../target/debug/kiosk-main}"
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

PASS_COUNT=0
FAIL_COUNT=0
RESULTS=()   # "N|name|PASS" or "N|name|FAIL" per scenario, printed as the summary table

log() { echo "[smoke] $*" >&2; }

# ---------------------------------------------------------------------------
# Compositor lifecycle -- the ONE pair of functions P2-F reuses verbatim.
# ---------------------------------------------------------------------------
start_compositor() {
  mkdir -p /tmp/.X11-unix   # weston's xwayland module needs this dir to bind X0; harmless if already present
  weston --backend=headless-backend.so --socket="$WAYLAND_DISPLAY" --idle-time=0 \
    --xwayland --shell=kiosk-shell.so --width="$WAYLAND_OUT_W" --height="$WAYLAND_OUT_H" \
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
  kill "${WESTON_PID:-}" 2>/dev/null || true
  wait "${WESTON_PID:-}" 2>/dev/null || true
  rm -rf "$RUNTIME_DIR" "$SERVE_DIR" "$CONFIG_DIR"
}
trap stop_compositor EXIT

# ---------------------------------------------------------------------------
# Fixture httpd -- serves a REFRESHABLE COPY of fixtures/ (never the checked-in
# source), so a scenario can swap which signed config is at /config.json without
# touching the repo, and scenario 3 can stop/restart it cleanly.
# ---------------------------------------------------------------------------
start_fixtures() {
  rm -rf "$SERVE_DIR"; mkdir -p "$SERVE_DIR"
  cp "$SMOKE_DIR"/fixtures/*.html "$SERVE_DIR/"
  # `-u`: unbuffered stdout/stderr. Without it, http.server's access log (written
  # via stderr) is FULLY buffered once redirected to a file (only a TTY gets
  # Python's line-buffering default) -- every httpd_get_count check below would
  # read a truncated/empty log until the buffer happened to fill or the process
  # exited. Found the hard way: every httpd-log assertion in the first real run
  # false-failed with this omitted, despite the underlying page loads genuinely
  # succeeding (confirmed independently via the spool). See task-9-report.md.
  ( cd "$SERVE_DIR" && exec python3 -u -m http.server "$FIXTURE_PORT" ) >"$RUNTIME_DIR/httpd.log" 2>&1 &
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

# Which signed config variant is live at /config.json (spec §5.2, genuinely
# signed via kioskctl -- see README.md "Fixtures"). $1: config.json | config-reload.json | config-iframe.json
stage_config() {
  cp "$SMOKE_DIR/fixtures/$1" "$SERVE_DIR/config.json"
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
start_kiosk() {
  local backend="$1" ini="${2:-kiosk.ini}"
  rm -rf "$DATA_DIR"; mkdir -p "$DATA_DIR"
  rm -rf "$CONFIG_DIR"; mkdir -p "$CONFIG_DIR"
  cp "$SMOKE_DIR/fixtures/$ini" "$CONFIG_DIR/kiosk.ini"
  cp "$SMOKE_DIR/fixtures/kiosk-credential.json" "$CONFIG_DIR/kiosk-credential.json"
  chmod 600 "$CONFIG_DIR/kiosk-credential.json"   # SEC-09: the boot gate fails closed on anything wider
  cp "$SMOKE_DIR/fixtures/kiosk-offline.mp4" "$CONFIG_DIR/kiosk-offline.mp4"

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
  # cleanly in isolation every time -- see task-9-report.md's Concerns) was
  # traced to this gap -- pkill returns as soon as the signal is DELIVERED, not
  # once the process has actually exited and released its IPC sockets/shared
  # memory, and the very next scenario starts a fresh WebKit within
  # milliseconds of this function returning.
  for _ in $(seq 1 25); do
    pgrep -f WebKitWebProcess >/dev/null 2>&1 || pgrep -f WebKitNetworkProcess >/dev/null 2>&1 || break
    sleep 0.2
  done
}

kiosk_alive() { kill -0 "${KIOSK_PID:-0}" 2>/dev/null; }

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
httpd_get_count() { httpd_log | grep -cF "\"GET $1 HTTP/1."; }

httpd_get_at_least() { [ "$(httpd_get_count "$1")" -ge "$2" ]; }
event_at_least() { [ "$(event_count "$1")" -ge "$2" ]; }
event_with_reason_at_least() { [ "$(event_count_with_reason "$1" "$2")" -ge "$3" ]; }

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

note_blocked() {
  log "  BLOCKED: $1"
}

run_scenario() {
  local n="$1" name="$2" fn="$3"
  SCENARIO_OK=1
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
    check "window X" "$(echo "$geo" | sed -n 's/^X=//p')" 0
    check "window Y" "$(echo "$geo" | sed -n 's/^Y=//p')" 0
    check "window WIDTH == headless output width" "$(echo "$geo" | sed -n 's/^WIDTH=//p')" "$WAYLAND_OUT_W"
    check "window HEIGHT == headless output height" "$(echo "$geo" | sed -n 's/^HEIGHT=//p')" "$WAYLAND_OUT_H"
  fi

  stop_kiosk
}

# ---------------------------------------------------------------------------
# Scenario 2: off-list navigation blocked; target=_blank (in-allowlist navigates
# in place, off-allowlist blocked). See README.md/task-9-report.md for why the
# target=_blank sub-checks are driven but cannot be genuinely exercised here.
# ---------------------------------------------------------------------------
scenario_2() {
  stage_config config.json
  start_kiosk wayland
  wait_until 15 httpd_get_at_least /home.html 1

  check "kiosk-main alive after boot" "$(kiosk_alive && echo yes || echo no)" yes

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
    note_blocked "target=_blank in-allowlist: no GET /allowed-target.html observed (got $blank_allow_gets) -- WebKit's own popup gate (javascript-can-open-windows-automatically, untouched on Linux -- hardening.rs is a no-op here) requires a trusted user gesture a script .click() cannot supply; this container has no input devices (see task-9-report.md Concerns). NOT counted as scenario failure."
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

  stop_kiosk
}

# ---------------------------------------------------------------------------
# Scenario 3: offline fallback. Stop the fixture httpd, drive a reload (the page
# itself calls location.reload() -- a genuine top-level navigation attempt, not
# a simulation), assert the failure is observed and the app survives.
# ---------------------------------------------------------------------------
scenario_3() {
  stage_config config-reload.json
  start_kiosk wayland kiosk-reload.ini
  wait_until 15 httpd_get_at_least '/home.html?probe=reload' 1

  check "kiosk-main alive after boot" "$(kiosk_alive && echo yes || echo no)" yes
  check "initial GET of home.html?probe=reload succeeded" \
    "$([ "$(httpd_get_count '/home.html?probe=reload')" -ge 1 ] && echo yes || echo no)" yes

  stop_fixtures
  log "  fixture httpd stopped; waiting for the page's self-reload to hit a closed port"
  wait_until 12 event_at_least nav.error 1   # reload fires at T+2s after load; localhost connection-refused is fast

  check "nav.error observed (the reload's connection failure)" "$([ "$(event_count nav.error)" -ge 1 ] && echo yes || echo no)" yes
  check "kiosk-main still alive (fell to the offline page, did not crash)" "$(kiosk_alive && echo yes || echo no)" yes

  log "  offline.html's app-origin load and its kioskasset://localhost mp4-URL computation are" \
      "deterministic page-local JS (verified by reading offline.html; not runtime-introspectable" \
      "from this shell harness -- no remote-debugging channel is wired). See task-9-report.md."
  # Best-effort screenshot (design spec's gate section: non-blocking) was tried
  # and dropped: weston-screenshooter hits `screenshot_create_shm_buffer:
  # Assertion 'width > 0' failed` against the headless backend every time
  # (confirmed in isolation, not a transient flake) -- it does not query a
  # memory-surface output's geometry the way it does a real one. Not worth
  # carrying a call that is guaranteed to abort on this backend.

  stop_kiosk
  start_fixtures   # restart for scenario 4/5
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

  pkill -f WebKitWebProcess
  log "  sent SIGTERM to WebKitWebProcess"
  wait_until 10 event_at_least webview.crash 1

  check "kiosk-main (the supervising process) survived the renderer crash" "$(kiosk_alive && echo yes || echo no)" yes
  check "exactly one webview.crash" "$(event_count webview.crash)" 1
  log "  webview.crash kind: $(spool_events | grep -F '"event":"webview.crash"' | sed -n 's/.*"kind":"\([^"]*\)".*/\1/p')"

  wait_until 10 httpd_get_at_least /home.html 2
  check "a fresh GET /home.html after the crash (recovery navigate-home)" \
    "$([ "$(httpd_get_count /home.html)" -ge 2 ] && echo yes || echo no)" yes

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
main() {
  start_compositor
  start_fixtures

  run_scenario 1 "boot -> splash -> remote home commits" scenario_1
  run_scenario 2 "off-list nav blocked + target=_blank" scenario_2
  run_scenario 3 "offline fallback" scenario_3
  run_scenario 4 "renderer crash recovery" scenario_4
  run_scenario 5 "iframe blocking (main-frame pin)" scenario_5

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

main
