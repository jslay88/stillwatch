#!/bin/bash
# Run a command in a KVM guest whose kernel has vgem.
#
# GitHub's kernel has no DRM render node, so kwin_wayland --virtual on the
# runner composites with QPainter and ScreenShot2 returns Cancelled. The Arch
# kernel's vgem module gives KWin a primary node and Mesa llvmpipe an OpenGL
# context. The job container stays unprivileged: only /dev/kvm is passed in.

set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
cmd=${1:-cargo xtask coverage}

die() {
    echo "coverage-guest: $*" >&2
    exit 1
}

pick_kernel() {
    local running kver image
    if [[ -n ${STILLWATCH_GUEST_KERNEL:-} ]]; then
        printf '%s\n' "$STILLWATCH_GUEST_KERNEL"
        return 0
    fi
    running="/usr/lib/modules/$(uname -r)/vmlinuz"
    if [[ -f $running ]] && compgen -G "/usr/lib/modules/$(uname -r)/kernel/drivers/gpu/drm/vgem/vgem.ko*" >/dev/null; then
        printf '%s\n' "$running"
        return 0
    fi
    shopt -s nullglob
    for image in /usr/lib/modules/*/vmlinuz; do
        kver=$(basename "$(dirname "$image")")
        if compgen -G "/usr/lib/modules/$kver/kernel/drivers/gpu/drm/vgem/vgem.ko*" >/dev/null; then
            printf '%s\n' "$image"
            shopt -u nullglob
            return 0
        fi
    done
    shopt -u nullglob
    return 1
}

if [[ ! -e /dev/kvm ]]; then
    die "/dev/kvm is missing (the job needs --device=/dev/kvm, not --privileged)"
fi

if ! pick_kernel >/dev/null 2>&1 || ! command -v qemu-system-x86_64 >/dev/null || ! command -v virtiofsd >/dev/null || ! command -v mkinitcpio >/dev/null || ! command -v gcc >/dev/null; then
    command -v pacman >/dev/null || die "need qemu, virtiofsd, mkinitcpio, gcc, and a kernel with vgem"
    pacman -S --noconfirm --needed linux qemu-system-x86 virtiofsd gcc || true
fi

kimage=$(pick_kernel) || die "no kernel image with a vgem module"
kver=$(basename "$(dirname "$kimage")")
echo "coverage-guest: kernel $kver ($kimage)"

if [[ ! -f /usr/lib/modules/$kver/modules.dep ]]; then
    depmod "$kver"
fi

mkdir -p target
gcc -O2 -o target/guest-poweroff .github/scripts/poweroff.c

initramfs=$root/target/guest-initramfs.img
mkconf=$root/target/guest-mkinitcpio.conf
cat >"$mkconf" <<'EOF'
MODULES=(virtio_pci virtiofs)
BINARIES=()
FILES=()
HOOKS=(base)
COMPRESSION="zstd"
EOF
mkinitcpio -k "$kver" -g "$initramfs" -c "$mkconf"
virtiofs_ko=$(find "/usr/lib/modules/$kver" -name 'virtiofs.ko*' -print -quit)
if [[ -n $virtiofs_ko ]]; then
    lsinitcpio "$initramfs" >target/guest-initramfs.list
    grep -q virtiofs target/guest-initramfs.list || die "initramfs is missing the virtiofs module"
fi

envfile=$root/target/guest.env
statusfile=$root/target/guest-status
rm -f "$statusfile"
{
    printf 'export PATH=%q\n' "$PATH"
    printf 'export HOME=%q\n' "${HOME:-/root}"
    printf 'export CI=%q\n' "${CI:-true}"
    printf 'export STILLWATCH_REQUIRE_DBUS=%q\n' "${STILLWATCH_REQUIRE_DBUS:-1}"
    printf 'export STILLWATCH_REQUIRE_KWIN=%q\n' "${STILLWATCH_REQUIRE_KWIN:-1}"
    printf 'export LIBGL_ALWAYS_SOFTWARE=1\n'
    printf 'export STILLWATCH_WORKSPACE=%q\n' "$root"
    printf 'export STILLWATCH_GUEST_CMD=%q\n' "$cmd"
    printf 'export STILLWATCH_GUEST_STATUS=%q\n' "$statusfile"
    printf 'export STILLWATCH_POWEROFF=%q\n' "$root/target/guest-poweroff"
    var=""
    for var in CARGO_HOME RUSTUP_HOME CARGO_TERM_COLOR CARGO_INCREMENTAL CARGO_TARGET_DIR RUSTFLAGS RUSTDOCFLAGS; do
        if [[ -n ${!var:-} ]]; then
            printf 'export %s=%q\n' "$var" "${!var}"
        fi
    done
    printf 'export LANG=%q\n' "${LANG:-C.UTF-8}"
} >"$envfile"

sock=$root/target/guest-vfs.sock
rm -f "$sock"
virtiofsd --sandbox none --socket-path "$sock" --shared-dir / --inode-file-handles=never \
    >target/guest-virtiofsd.log 2>&1 &
vfs_pid=$!
cleanup() {
    if [[ -n ${qemu_pid:-} ]] && kill -0 "$qemu_pid" 2>/dev/null; then
        kill "$qemu_pid" 2>/dev/null || true
        wait "$qemu_pid" 2>/dev/null || true
    fi
    if kill -0 "$vfs_pid" 2>/dev/null; then
        kill "$vfs_pid" 2>/dev/null || true
        wait "$vfs_pid" 2>/dev/null || true
    fi
}
trap cleanup EXIT

for _ in $(seq 1 100); do
    [[ -S $sock ]] && break
    sleep 0.1
done
[[ -S $sock ]] || die "virtiofsd did not create $sock (see target/guest-virtiofsd.log)"

mem_kib=$(awk '/MemAvailable/ {print $2}' /proc/meminfo)
guest_mib=$((mem_kib / 1024 - 2048))
if ((guest_mib < 2048)); then
    guest_mib=2048
fi
if ((guest_mib > 8192)); then
    guest_mib=8192
fi
cpus=$(nproc)
if ((cpus > 4)); then
    cpus=4
fi

echo "coverage-guest: ${guest_mib}MiB, ${cpus} cpus, cmd: $cmd"
set +e
timeout 60m qemu-system-x86_64 \
    -enable-kvm -cpu host -m "$guest_mib" -smp "$cpus" \
    -kernel "$kimage" \
    -initrd "$initramfs" \
    -append "console=ttyS0,115200 loglevel=4 root=rootfs rootfstype=virtiofs rw init=$root/.github/scripts/guest-init stillwatch_env=$envfile panic=1" \
    -object "memory-backend-memfd,id=mem,size=${guest_mib}M,share=on" \
    -numa node,memdev=mem \
    -chardev socket,id=char0,path="$sock" \
    -device vhost-user-fs-pci,queue-size=1024,chardev=char0,tag=rootfs \
    -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \
    -display none -serial stdio -monitor none \
    -no-reboot &
qemu_pid=$!
wait "$qemu_pid"
qemu_status=$?
set -e
echo "coverage-guest: qemu exit $qemu_status"

if [[ ! -f $statusfile ]]; then
    die "guest wrote no status (qemu exit $qemu_status)"
fi
guest_status=$(tr -d '[:space:]' <"$statusfile")
echo "coverage-guest: guest status $guest_status"
[[ $guest_status == 0 ]]
