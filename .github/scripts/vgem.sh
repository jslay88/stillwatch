#!/bin/sh
# Gives the job container a vgem DRM device, so kwin_wayland --virtual
# composites with OpenGL (Mesa llvmpipe) instead of QPainter. ScreenShot2 only
# captures under OpenGL.
#
# The runner kernel ships vgem in linux-modules-extra, which isn't installed.
# It's installed and loaded on the host through the mounted docker socket. The
# container needs --device-cgroup-rule="c 226:* rmw" to open the node.
set -eu

pacman -S --noconfirm --needed docker >/dev/null
docker run --rm --privileged --pid=host archlinux:latest \
    nsenter -t 1 -m -u -n -i -- sh -euc '
        modprobe vgem 2>/dev/null || {
            apt-get update -qq
            DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "linux-modules-extra-$(uname -r)" >/dev/null
            modprobe vgem
        }'

mkdir -p /dev/dri
for card in /sys/class/drm/card*; do
    [ "$(basename "$(readlink -f "$card/device/driver")")" = vgem ] || continue
    IFS=: read -r major minor <"$card/dev"
    node="/dev/dri/$(basename "$card")"
    [ -e "$node" ] || mknod -m 666 "$node" c "$major" "$minor"
    echo "vgem at $node"
    exit 0
done
echo "vgem loaded but no card showed up in /sys/class/drm" >&2
exit 1
