#!/bin/bash
# Launch the locally built BIOS-GRUB RetroOS HDD image in QEMU.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
IMAGE="${RETROOS_HDD_IMAGE:-$ROOT/retroos-grub-hdd.img}"
AUDIO_BACKEND="${RETROOS_AUDIO_BACKEND:-pa}"
HOSTFS_DIR="${RETROOS_HOSTFS_DIR:-}"
QEMU_AUDIO_ENV=()
HOSTFS_ARGS=()
HOSTFS_PID=""
HOSTFS_SOCKET=""

if [ "${1:-}" = "--help" ]; then
    echo "Usage: ./run_grub_hdd.sh [extra QEMU arguments]"
    echo
    echo "Environment overrides:"
    echo "  RETROOS_HDD_IMAGE  Raw image path (default: retroos-grub-hdd.img)"
    echo "  RETROOS_MEMORY_MB  Guest RAM in MiB (default: 128)"
    echo "  QEMU_DISPLAY       QEMU display backend (default: sdl)"
    echo "  RETROOS_AUDIO_BACKEND"
    echo "                      QEMU audio backend (default: pa)"
    echo "  RETROOS_HOSTFS_DIR  Host directory exposed at C:\\HOST (disabled by default)"
    echo
    echo "The disk runs with snapshot=on, so the source image is not modified."
    echo "Audio uses QEMU's ICH6 Intel HDA model with an HDA duplex codec."
    exit 0
fi

command -v qemu-system-i386 >/dev/null ||
    { echo "qemu-system-i386 is not installed." >&2; exit 1; }

[ -f "$IMAGE" ] ||
    { echo "Missing HDD image: $IMAGE" >&2; exit 1; }

cleanup() {
    if [ -n "$HOSTFS_PID" ]; then
        kill "$HOSTFS_PID" 2>/dev/null || true
        wait "$HOSTFS_PID" 2>/dev/null || true
    fi
    if [ -n "$HOSTFS_SOCKET" ]; then
        rm -f "$HOSTFS_SOCKET"
    fi
}
trap cleanup EXIT

if [ -n "$HOSTFS_DIR" ]; then
    [ -d "$HOSTFS_DIR" ] ||
        { echo "Missing hostfs directory: $HOSTFS_DIR" >&2; exit 1; }
    command -v python3 >/dev/null ||
        { echo "python3 is required for hostfs." >&2; exit 1; }
    HOSTFS_SOCKET="${TMPDIR:-/tmp}/retroos-hostfs-$$.sock"
    HOSTFS_ARGS=(
        -serial chardev:hostfs
        -chardev "socket,id=hostfs,path=$HOSTFS_SOCKET,server=on,wait=off"
        -fw_cfg "name=opt/c_root,file=/dev/null"
    )
    python3 "$ROOT/hostfs.py" "$HOSTFS_DIR" "$HOSTFS_SOCKET" &
    HOSTFS_PID=$!
fi

# QEMU does not model ICH7M HDA exactly. Its older `intel-hda` device models
# ICH6 and is the closest available controller; `ich9-intel-hda` is newer.
env "${QEMU_AUDIO_ENV[@]}" qemu-system-i386 \
    -m "${RETROOS_MEMORY_MB:-128}" \
    -drive "file=$IMAGE,format=raw,if=ide,snapshot=on" \
    -boot c \
    -debugcon stdio \
    -vga std \
    -display "${QEMU_DISPLAY:-sdl}" \
    -audiodev "$AUDIO_BACKEND,id=snd0" \
    -device intel-hda \
    -device hda-duplex,audiodev=snd0 \
    -machine pcspk-audiodev=snd0 \
    -no-reboot \
    "${HOSTFS_ARGS[@]}" \
    "$@"
