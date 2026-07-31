#!/usr/bin/env bash
set -euo pipefail

readonly SSH_HOST="${RETROOS_PXE_HOST:-retroos-pi}"
readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SKILL_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
readonly DISCOVERED_REPO="$(cd "${SKILL_DIR}/../../.." && pwd)"
readonly REPO_ROOT="${RETROOS_REPO_ROOT:-$DISCOVERED_REPO}"
readonly LOCAL_GRUB_CFG="${SKILL_DIR}/assets/retroos-grub.cfg"
readonly REMOTE_SEED="${RETROOS_PXE_SEED:-/usr/local/share/pxe-tftp-seed}"
readonly REMOTE_TFTP="${RETROOS_TFTP_ROOT:-/srv/tftp}"
readonly REMOTE_DIR="${REMOTE_TFTP}/boot/retroos"
readonly REMOTE_KERNEL="${REMOTE_DIR}/kernel.elf"
readonly REMOTE_PREVIOUS="${REMOTE_DIR}/kernel.elf.previous"
readonly REMOTE_GRUB_CFG="${REMOTE_TFTP}/grub/grub.cfg"

usage() {
    printf 'Usage: %s --status | [kernel.elf]\n' "${0##*/}" >&2
}

remote_status() {
    ssh "$SSH_HOST" sh -s -- "$REMOTE_SEED" "$REMOTE_TFTP" <<'REMOTE'
set -eu
seed=$1
tftp=$2

printf '%s\n' 'TFTP mount:'
findmnt -T "$tftp" -o TARGET,SOURCE,FSTYPE,OPTIONS

printf '%s\n' 'GRUB configuration:'
if [ -r "$tftp/grub/grub.cfg" ]; then
    sed -n '1,240p' "$tftp/grub/grub.cfg"
else
    printf 'missing: %s\n' "$tftp/grub/grub.cfg"
fi

printf '%s\n' 'PXE artifacts:'
for path in \
    "$tftp/bootx86.pxe" \
    "$tftp/grub/i386-pc/multiboot.mod" \
    "$tftp/boot/retroos/kernel.elf" \
    "$tftp/boot/retroos/kernel.elf.previous"
do
    if [ -e "$path" ]; then
        ls -ld "$path"
        if [ -f "$path" ]; then sha256sum "$path"; fi
    else
        printf 'missing: %s\n' "$path"
    fi
done

printf '%s\n' 'Persistent seed:'
test -r "$seed/bootx86.pxe"
test -r "$seed/grub/grub.cfg"
test -r "$seed/grub/i386-pc/multiboot.mod"
printf '%s\n' 'seed-valid=yes'
sudo -n true
REMOTE
}

if [[ "${1:-}" == "--status" ]]; then
    [[ $# -eq 1 ]] || { usage; exit 2; }
    remote_status
    exit 0
fi

[[ $# -le 1 ]] || { usage; exit 2; }

artifact="${1:-bazel-bin/kernel/kernel.elf}"
if [[ "$artifact" != /* ]]; then artifact="${REPO_ROOT}/${artifact}"; fi
readonly ARTIFACT="$artifact"
[[ -f "$ARTIFACT" && -s "$ARTIFACT" ]] || {
    printf 'Kernel artifact is missing or empty: %s\n' "$ARTIFACT" >&2
    exit 1
}
[[ -f "$LOCAL_GRUB_CFG" && -s "$LOCAL_GRUB_CFG" ]] || {
    printf 'Bundled GRUB configuration is missing: %s\n' "$LOCAL_GRUB_CFG" >&2
    exit 1
}

readonly FILE_DESCRIPTION="$(file -b "$ARTIFACT")"
case "$FILE_DESCRIPTION" in
    *"ELF 32-bit LSB executable"*"Intel 80386"*) ;;
    *)
        printf 'Refusing non-i386 kernel artifact: %s\n' "$FILE_DESCRIPTION" >&2
        exit 1
        ;;
esac

if command -v grub-file >/dev/null 2>&1; then
    grub-file --is-x86-multiboot "$ARTIFACT" || {
        printf 'Artifact has no valid Multiboot 1 header: %s\n' "$ARTIFACT" >&2
        exit 1
    }
fi

readonly KERNEL_SHA="$(sha256sum "$ARTIFACT" | awk '{print $1}')"
readonly KERNEL_SIZE="$(stat -c '%s' "$ARTIFACT")"
readonly GRUB_SHA="$(sha256sum "$LOCAL_GRUB_CFG" | awk '{print $1}')"
readonly GRUB_SIZE="$(stat -c '%s' "$LOCAL_GRUB_CFG")"
readonly UPLOAD_KERNEL="/tmp/retroos-kernel-${KERNEL_SHA}.elf"
readonly UPLOAD_GRUB="/tmp/retroos-grub-${GRUB_SHA}.cfg"
readonly STAGE_KERNEL="${REMOTE_KERNEL}.new-${KERNEL_SHA}"
readonly STAGE_GRUB="${REMOTE_GRUB_CFG}.new-${GRUB_SHA}"

printf 'Local kernel: %s\n' "$ARTIFACT"
printf 'Kernel size: %s\n' "$KERNEL_SIZE"
printf 'Kernel SHA-256: %s\n' "$KERNEL_SHA"
printf 'GRUB SHA-256: %s\n' "$GRUB_SHA"

scp -- "$ARTIFACT" "${SSH_HOST}:${UPLOAD_KERNEL}"
scp -- "$LOCAL_GRUB_CFG" "${SSH_HOST}:${UPLOAD_GRUB}"

ssh "$SSH_HOST" sh -s -- \
    "$REMOTE_SEED" "$REMOTE_TFTP" \
    "$UPLOAD_KERNEL" "$UPLOAD_GRUB" \
    "$STAGE_KERNEL" "$STAGE_GRUB" \
    "$REMOTE_KERNEL" "$REMOTE_PREVIOUS" "$REMOTE_GRUB_CFG" \
    "$KERNEL_SHA" "$KERNEL_SIZE" "$GRUB_SHA" "$GRUB_SIZE" <<'REMOTE'
set -eu

seed=$1
tftp=$2
upload_kernel=$3
upload_grub=$4
stage_kernel=$5
stage_grub=$6
kernel=$7
previous=$8
grub_cfg=$9
shift 9
kernel_sha=$1
kernel_size=$2
grub_sha=$3
grub_size=$4

cleanup() {
    rm -f "$upload_kernel" "$upload_grub"
    sudo rm -f "$stage_kernel" "$stage_grub"
}
trap cleanup EXIT HUP INT TERM

fs_type=$(findmnt -n -o FSTYPE -T "$tftp")
if [ "$fs_type" != tmpfs ]; then
    printf 'Refusing deployment: %s is %s, not tmpfs\n' "$tftp" "$fs_type" >&2
    exit 1
fi

test -r "$seed/bootx86.pxe"
test -r "$seed/grub/grub.cfg"
test -r "$seed/grub/i386-pc/normal.mod"
test -r "$seed/grub/i386-pc/multiboot.mod"

actual_kernel_sha=$(sha256sum "$upload_kernel" | awk '{print $1}')
actual_kernel_size=$(stat -c '%s' "$upload_kernel")
actual_grub_sha=$(sha256sum "$upload_grub" | awk '{print $1}')
actual_grub_size=$(stat -c '%s' "$upload_grub")
test "$actual_kernel_sha" = "$kernel_sha"
test "$actual_kernel_size" = "$kernel_size"
test "$actual_grub_sha" = "$grub_sha"
test "$actual_grub_size" = "$grub_size"

sudo install -d -m 0755 \
    "$tftp/grub" "$tftp/grub/i386-pc" "$tftp/boot/retroos"
sudo install -m 0644 "$seed/bootx86.pxe" "$tftp/bootx86.pxe"
sudo cp -a "$seed/grub/i386-pc/." "$tftp/grub/i386-pc/"
sudo find "$tftp/grub/i386-pc" -type d -exec chmod 0755 {} +
sudo find "$tftp/grub/i386-pc" -type f -exec chmod 0644 {} +

sudo install -m 0644 "$upload_kernel" "$stage_kernel"
sudo install -m 0644 "$upload_grub" "$stage_grub"
test "$(sha256sum "$stage_kernel" | awk '{print $1}')" = "$kernel_sha"
test "$(sha256sum "$stage_grub" | awk '{print $1}')" = "$grub_sha"

if [ -f "$kernel" ]; then sudo cp -p "$kernel" "$previous"; fi

sudo mv -f "$stage_kernel" "$kernel"
test "$(sha256sum "$kernel" | awk '{print $1}')" = "$kernel_sha"
sudo mv -f "$stage_grub" "$grub_cfg"
test "$(sha256sum "$grub_cfg" | awk '{print $1}')" = "$grub_sha"

printf 'Published kernel: %s\n' "$kernel"
printf 'Remote kernel SHA-256: %s\n' "$kernel_sha"
printf 'Activated GRUB config: %s\n' "$grub_cfg"
printf 'Remote GRUB SHA-256: %s\n' "$grub_sha"
REMOTE
