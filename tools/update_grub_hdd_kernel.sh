#!/bin/bash
# Replace only the RetroOS kernel in an existing BIOS-GRUB HDD image.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="${1:-$ROOT/retroos-grub-hdd.img}"
KERNEL="${2:-$ROOT/bazel-bin/kernel/kernel.elf}"

[ "$(id -u)" -eq 0 ] ||
    { echo "Run this script with sudo." >&2; exit 1; }
[ -f "$IMAGE" ] ||
    { echo "Missing image: $IMAGE" >&2; exit 1; }
[ -f "$KERNEL" ] ||
    { echo "Missing kernel: $KERNEL" >&2; exit 1; }

WORK="$(mktemp -d -t retroos-kernel-update.XXXXXX)"
MOUNT="$WORK/mount"
LOOP=""
MOUNTED=0

cleanup() {
    if [ "$MOUNTED" -eq 1 ]; then
        umount "$MOUNT" || true
    fi
    if [ -n "$LOOP" ]; then
        losetup -d "$LOOP" || true
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT

mkdir -p "$MOUNT"
LOOP="$(losetup --find --show --partscan "$IMAGE")"
PART="${LOOP}p1"
[ -b "$PART" ] ||
    { echo "Partition device was not created: $PART" >&2; exit 1; }

mount "$PART" "$MOUNT"
MOUNTED=1
[ -d "$MOUNT/boot/retroos" ] ||
    { echo "Image lacks /boot/retroos." >&2; exit 1; }

install -m 644 "$KERNEL" "$MOUNT/boot/retroos/kernel.elf"
sync
cmp "$KERNEL" "$MOUNT/boot/retroos/kernel.elf"

umount "$MOUNT"
MOUNTED=0
losetup -d "$LOOP"
LOOP=""

sha256sum "$IMAGE" > "$IMAGE.sha256"
if [ -n "${SUDO_UID:-}" ] && [ -n "${SUDO_GID:-}" ]; then
    chown "$SUDO_UID:$SUDO_GID" "$IMAGE" "$IMAGE.sha256"
fi

echo "Updated /boot/retroos/kernel.elf in $IMAGE"
sha256sum "$KERNEL" "$IMAGE"
