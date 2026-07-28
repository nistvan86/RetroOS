#!/bin/bash
# Build a flashable MBR/BIOS-GRUB HDD image containing RetroOS and its ext4
# filesystem. It packages the single upstream kernel, whose Multiboot header
# selects native VGA when BIOS text is available and falls back to a linear
# framebuffer on firmware without it. Run as root because loop setup, mounting,
# and grub-install need privileges. The script only operates on a new image.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="${1:-$ROOT/retroos-grub-hdd.img}"
KERNEL="${2:-$ROOT/bazel-bin/kernel/kernel.elf}"
EXTRAS="$ROOT/bazel-bin/extras_tar.tar"
GRUB_CFG="$ROOT/tools/grub-hdd-grub.cfg"
IMAGE_SIZE_MIB=1152

for tool in parted losetup partprobe mkfs.ext4 mount umount grub-install sha256sum; do
    command -v "$tool" >/dev/null ||
        { echo "Missing required tool: $tool" >&2; exit 1; }
done

[ "$(id -u)" -eq 0 ] ||
    { echo "Run this script with sudo." >&2; exit 1; }
[ -f "$KERNEL" ] ||
    { echo "Missing kernel ELF: $KERNEL" >&2; exit 1; }
[ -f "$EXTRAS" ] ||
    { echo "Missing $EXTRAS; build //:extras_tar first." >&2; exit 1; }
[ -f "$GRUB_CFG" ] ||
    { echo "Missing $GRUB_CFG." >&2; exit 1; }
[ ! -e "$IMAGE" ] ||
    { echo "Refusing to overwrite existing output: $IMAGE" >&2; exit 1; }

WORK="$(mktemp -d -t retroos-grub-hdd.XXXXXX)"
STAGE="$WORK/stage"
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

mkdir -p "$STAGE" "$MOUNT"

echo "Creating ${IMAGE_SIZE_MIB} MiB raw image: $IMAGE"
truncate -s "${IMAGE_SIZE_MIB}M" "$IMAGE"
parted -s "$IMAGE" \
    mklabel msdos \
    mkpart primary ext4 1MiB 100% \
    set 1 boot on

echo "Attaching image through a loop device"
LOOP="$(losetup --find --show --partscan "$IMAGE")"
partprobe "$LOOP"
PART="${LOOP}p1"
[ -b "$PART" ] ||
    { echo "Partition device was not created: $PART" >&2; exit 1; }

echo "Preparing the ext4 filesystem tree"
tar xf "$EXTRAS" -C "$STAGE"
mkdir -p "$STAGE/boot/grub" "$STAGE/boot/retroos"
install -m 644 "$KERNEL" "$STAGE/boot/retroos/kernel.elf"
install -m 644 "$GRUB_CFG" "$STAGE/boot/grub/grub.cfg"

if [ -d "$STAGE/home/retroos" ]; then
    chmod -R g+w "$STAGE/home/retroos"
fi

echo "Creating and populating ext4"
mkfs.ext4 -q -L RetroOS -d "$STAGE" "$PART"

echo "Installing BIOS GRUB"
mount "$PART" "$MOUNT"
MOUNTED=1
grub-install \
    --target=i386-pc \
    --boot-directory="$MOUNT/boot" \
    --modules="biosdisk part_msdos ext2 multiboot search search_fs_file configfile normal" \
    --no-floppy \
    --recheck \
    "$LOOP"
sync
umount "$MOUNT"
MOUNTED=0
losetup -d "$LOOP"
LOOP=""

sha256sum "$IMAGE" > "$IMAGE.sha256"

if [ -n "${SUDO_UID:-}" ] && [ -n "${SUDO_GID:-}" ]; then
    chown "$SUDO_UID:$SUDO_GID" "$IMAGE" "$IMAGE.sha256"
fi

echo "Created:"
ls -lh "$IMAGE" "$IMAGE.sha256"
