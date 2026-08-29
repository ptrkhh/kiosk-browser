#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STAGE="${KIOSK_DEB_STAGE:-$ROOT/packaging/linux/.stage}"
OUT="${1:-$ROOT/kiosk_0.1.0_amd64.deb}"

test -x "$ROOT/target/release/kiosk-main"
test -x "$ROOT/target/release/kiosk-launcher"
rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN" "$STAGE/usr/lib/kiosk" "$STAGE/usr/share/kiosk" \
  "$STAGE/lib/systemd/system"

install -m 0755 "$ROOT/target/release/kiosk-main" "$STAGE/usr/lib/kiosk/kiosk-main"
install -m 0755 "$ROOT/target/release/kiosk-launcher" "$STAGE/usr/lib/kiosk/kiosk-launcher"
for page in error.html offline.html pinpad.html safe.html splash.html; do
  install -m 0644 "$ROOT/crates/kiosk-main/bundled/$page" "$STAGE/usr/lib/kiosk/$page"
done
install -m 0644 "$ROOT/packaging/linux/kiosk.service" "$STAGE/lib/systemd/system/kiosk.service"
install -m 0644 "$ROOT/packaging/linux/kiosk.ini.example" "$STAGE/usr/share/kiosk/kiosk.ini.example"
install -m 0755 "$ROOT/packaging/linux/kiosk-provision-credential" \
  "$STAGE/usr/lib/kiosk/kiosk-provision-credential"
install -m 0755 "$ROOT/packaging/linux/DEBIAN/postinst" "$STAGE/DEBIAN/postinst"
install -m 0755 "$ROOT/packaging/linux/DEBIAN/prerm" "$STAGE/DEBIAN/prerm"
install -m 0755 "$ROOT/packaging/linux/DEBIAN/postrm" "$STAGE/DEBIAN/postrm"
install -m 0644 "$ROOT/packaging/linux/DEBIAN/control.in" "$STAGE/DEBIAN/control"

# Keep the substvar flow explicit. dpkg-deb -b over a hand-written control
# would otherwise leave the literal ${shlibs:Depends} in the package. The
# helper expects a debian/control source stanza, so run it from a tiny throwaway
# build context rather than weakening the package metadata.
SHLIBDEPS_DIR="$(mktemp -d)"
trap 'rm -rf "$SHLIBDEPS_DIR"' EXIT
mkdir -p "$SHLIBDEPS_DIR/debian"
install -m 0644 "$STAGE/DEBIAN/control" "$SHLIBDEPS_DIR/debian/control"
(cd "$SHLIBDEPS_DIR" && dpkg-shlibdeps -O \
  -e "$STAGE/usr/lib/kiosk/kiosk-main" \
  -e "$STAGE/usr/lib/kiosk/kiosk-launcher") > "$STAGE/DEBIAN/substvars"
(cd "$SHLIBDEPS_DIR" && dpkg-gencontrol -pkiosk -P"$STAGE" \
  -c"$STAGE/DEBIAN/control" -l"$ROOT/packaging/linux/debian/changelog" \
  -T"$STAGE/DEBIAN/substvars" -O"$STAGE/DEBIAN/control.generated")
mv "$STAGE/DEBIAN/control.generated" "$STAGE/DEBIAN/control"
if grep -q '${shlibs:Depends}' "$STAGE/DEBIAN/control"; then
  echo "refusing to package literal shlibs substvar" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUT")"
dpkg-deb -b "$STAGE" "$OUT"
echo "built $OUT"
