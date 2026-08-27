#!/usr/bin/env bash
set -euo pipefail

if [ "${1:-}" = --probe-only ]; then
  command -v cage >/dev/null
  cage -v
  test -f packaging/linux/kiosk.service
  grep -q '^StartLimitIntervalSec=0$' packaging/linux/kiosk.service
  grep -q '^WantedBy=multi-user.target$' packaging/linux/kiosk.service
  echo "container probe passed"
  exit 0
fi

: "${KIOSK_DEB:?set KIOSK_DEB to the package under test}"
test -f "$KIOSK_DEB"
DEBIAN_FRONTEND=noninteractive dpkg -i "$KIOSK_DEB" </dev/null
test -x /usr/lib/kiosk/kiosk-launcher
test -x /usr/lib/kiosk/kiosk-main
test -d /etc/kiosk
test -d /var/lib/kiosk
test "$(stat -c '%a' /etc/kiosk)" = 750
test "$(stat -c '%a' /var/lib/kiosk)" = 750
test -f /usr/share/kiosk/kiosk.ini.example
test ! -e /etc/kiosk/kiosk.ini
test ! -e /etc/kiosk/kiosk-credential.json
test ! -e /etc/kiosk/kiosk-offline.mp4
systemctl --root=/ is-enabled kiosk.service | grep -qx enabled
systemd-analyze verify packaging/linux/kiosk.service

control_dir="$(mktemp -d)"
trap 'rm -rf "$control_dir"' EXIT
dpkg-deb -e "$KIOSK_DEB" "$control_dir"
! grep -R -q 'BEGIN PRIVATE KEY' "$control_dir"
! grep -R -q 'shlibs:Depends' "$control_dir/control"
echo "container assertions passed"
