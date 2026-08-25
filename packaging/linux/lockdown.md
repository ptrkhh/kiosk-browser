# Linux kiosk lockdown runbook

Target: Debian 12, cage 0.1.4-4 floor (the in-session probe used cage 0.1.5).
Run every command as root on a newly provisioned or disposable device.

## 1. Provision operator files

The package deliberately ships no /etc/kiosk/kiosk.ini,
kiosk-credential.json, or kiosk-offline.mp4.

    install -d -m 0750 -o root -g root /etc/kiosk /var/lib/kiosk
    install -m 0640 -o root -g root /usr/share/kiosk/kiosk.ini.example /etc/kiosk/kiosk.ini
    /usr/lib/kiosk/kiosk-provision-credential /path/to/fleet-credential.json
    install -m 0644 /path/to/kiosk-offline.mp4 /etc/kiosk/kiosk-offline.mp4
    systemctl enable --now kiosk.service

Verify:

    test -f /etc/kiosk/kiosk.ini
    stat -c '%a %U:%G' /etc/kiosk/kiosk-credential.json
    test -s /etc/kiosk/kiosk-offline.mp4
    systemctl is-enabled kiosk.service

## 2. Console and seat

cage is invoked without -s; cage 0.1.4-4 therefore leaves VT switching
disabled by default. Remove alternate login paths:

    sed -i 's/^#\?NAutoVTs=.*/NAutoVTs=0/;s/^#\?ReserveVT=.*/ReserveVT=0/' /etc/systemd/logind.conf
    systemctl mask getty@.service
    systemctl disable getty.target

Verify:

    systemctl list-units 'getty@*' --all --no-legend | grep -q . && exit 1 || true
    loginctl seat-status seat0

The technician chord is the in-session escape. The image intentionally leaves
no VT/getty route.

## 3. Blanking, sleep and updates

The package conflicts with common idle blankers. Explicitly mask sleep paths
and keep logind idle actions disabled:

    systemctl mask sleep.target suspend.target hibernate.target hybrid-sleep.target
    sed -i 's/^#\?IdleAction=.*/IdleAction=ignore/' /etc/systemd/logind.conf
    systemctl daemon-reload

Verify:

    for unit in sleep.target suspend.target hibernate.target hybrid-sleep.target; do test "$(systemctl is-enabled "$unit")" = masked; done
    dpkg -l | grep -E 'swayidle|xautolock|xscreensaver|light-locker|gnome-screensaver|xfce4-power-manager' && exit 1 || true
    systemctl is-enabled unattended-upgrades 2>/dev/null | grep -Eq 'disabled|masked' || true
    apt-mark hold libwebkit2gtk-4.1-0
    apt-mark showhold | grep -q '^libwebkit2gtk-4.1-0$'

For a frozen screen that has not recovered within 60 seconds, power-cycle.
There is intentionally no VT, getty or SSH recovery route on a conforming
image.

## 4. On-screen keyboard

No OSK package is required. cage 0.1.4-4 and 0.1.5 expose no layer-shell or
input-method surface for an independent OSK; the application therefore injects
its bundled keyboard into the deployed page at document start. It is a
usability feature, not a security boundary. Walk every text-entry surface on
the target site and record the result in H4b of the hardware checklist.

## 5. Service user

The shipped default is root because it needs no additional seat package. The
non-root promotion recipe is:

    adduser --system --group kiosk
    usermod -aG _seatd,video,input,render kiosk
    systemctl enable --now seatd
    chown -R kiosk:kiosk /etc/kiosk /var/lib/kiosk
    sed -i 's/^ExecStart=/User=kiosk\nSupplementaryGroups=video input render _seatd\nExecStart=/' /lib/systemd/system/kiosk.service
    systemctl daemon-reload
    systemctl restart kiosk.service

Verify:

    loginctl seat-status seat0
    stat -c '%a %U:%G' /etc/kiosk/kiosk-credential.json
    systemctl show -p User kiosk.service

## 6. Evidence and recovery

Capture package and image state:

    dpkg -l > "snapshot-$(date +%F).txt"
    dpkg-query -W -f='${binary:Package}\t${Version}\t${db:Status-Abbrev}\n' > "snapshot-$(date +%F).tsv"
    cage -v >> "snapshot-$(date +%F).tsv"

Verify the local durable record before sign-off:

    find /var/lib/kiosk/spool -name '*.jsonl' -type f -exec grep -H -E 'egress\.(filter_absent|csp_absent)' {} + && exit 1 || true
    systemctl status kiosk.service --no-pager
    journalctl -u kiosk.service -b --no-pager | tail -50

H5 remains a real-device 72-hour offline-video soak. The CI harness is not a
substitute for hardware visual and compositor checks.
