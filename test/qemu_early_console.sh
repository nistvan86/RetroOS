#!/bin/bash
# QEMU early-console stop smoke test.
set -euo pipefail
cd "$(dirname "$0")/.."

stage=$(mktemp -d -t retroos-qemu-early-console.XXXXXX)
serial_log="$stage/serial.log"
debug_log="$stage/debug.log"
cleanup() {
    status=$?
    if [ "$status" -ne 0 ]; then
        echo "--- serial log ---" >&2
        cat "$serial_log" 2>/dev/null || true
        echo "--- debugcon log ---" >&2
        cat "$debug_log" 2>/dev/null || true
    fi
    rm -rf "$stage"
    return "$status"
}
trap cleanup EXIT

bazelisk build //:image >"$stage/build.log" 2>&1

set +e
timeout --kill-after=5s 20s qemu-system-i386 \
    -cpu 486 \
    -drive "file=bazel-bin/image.bin,format=raw,snapshot=on" \
    -m 64M -display none -no-reboot \
    -serial "file:$serial_log" \
    -debugcon "file:$debug_log" \
    -fw_cfg "name=opt/cmdline,string=serial=com1 console=early" \
    >/dev/null 2>&1
status=$?
set -e

case "$status" in
    124|137|143) ;;
    *)
        echo "QEMU did not stop in the early console (status $status)" >&2
        exit "$status"
        ;;
esac

grep -aFq 'RetroOS early console' "$serial_log"
test "$(grep -aFc 'RetroOS early console' "$serial_log")" -eq 1
test "$(grep -aFc 'Block devices initialized' "$serial_log")" -eq 0

# Attached-session output is direct to the serial session; it must not also be
# emitted through the ambient debugcon mirror.
test "$(grep -aFc 'RetroOS early console' "$debug_log")" -eq 0

echo "PASS: QEMU early console"
