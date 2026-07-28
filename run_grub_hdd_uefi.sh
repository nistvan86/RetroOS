#!/bin/bash
# Boot the existing MBR/BIOS-GRUB HDD contents through OVMF and a temporary
# x86_64-EFI GRUB system partition. The source HDD is attached read-only via
# QEMU snapshot mode and is not converted or modified.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
IMAGE="${RETROOS_HDD_IMAGE:-$ROOT/retroos-grub-hdd.img}"
KERNEL="${RETROOS_KERNEL:-$ROOT/bazel-bin/kernel/kernel.elf}"
OVMF_CODE="${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}"
OVMF_VARS="${OVMF_VARS:-/usr/share/OVMF/OVMF_VARS_4M.fd}"
WORK=""

if [ "${1:-}" = "--help" ]; then
    echo "Usage: ./run_grub_hdd_uefi.sh [extra QEMU arguments]"
    echo
    echo "The script creates a temporary EFI System Partition and boots:"
    echo "  OVMF -> x86_64 EFI GRUB -> bazel-bin/kernel/kernel.elf"
    echo "The existing retroos-grub-hdd.img remains the root/data disk."
    echo
    echo "Environment overrides:"
    echo "  RETROOS_HDD_IMAGE  Existing raw HDD image"
    echo "  RETROOS_KERNEL     Kernel placed on the temporary ESP"
    echo "  RETROOS_MEMORY_MB  Guest RAM in MiB (default: 512)"
    echo "  QEMU_DISPLAY       QEMU display backend (default: sdl)"
    echo "  OVMF_CODE          Read-only OVMF code image"
    echo "  OVMF_VARS          OVMF variable-store template"
    echo
    echo "Audio is intentionally disabled. Extra arguments are passed to QEMU."
    exit 0
fi

for tool in qemu-system-x86_64 grub-mkstandalone mformat mmd mcopy; do
    command -v "$tool" >/dev/null ||
        { echo "Missing required tool: $tool" >&2; exit 1; }
done

[ -f "$IMAGE" ] || { echo "Missing HDD image: $IMAGE" >&2; exit 1; }
[ -f "$KERNEL" ] || { echo "Missing kernel: $KERNEL" >&2; exit 1; }
[ -f "$OVMF_CODE" ] || { echo "Missing OVMF code image: $OVMF_CODE" >&2; exit 1; }
[ -f "$OVMF_VARS" ] || { echo "Missing OVMF variable template: $OVMF_VARS" >&2; exit 1; }

WORK="$(mktemp -d -t retroos-grub-hdd-uefi.XXXXXX)"
cleanup() {
    [ -z "$WORK" ] || rm -rf "$WORK"
}
trap cleanup EXIT

cat > "$WORK/grub.cfg" <<'EOF'
set timeout=0
insmod efi_gop
set gfxmode=auto
set gfxpayload=keep

menuentry "RetroOS HDD (UEFI framebuffer)" {
    search --no-floppy --file /kernel.elf --set=root
    multiboot /kernel.elf
    boot
}
EOF

grub-mkstandalone -O x86_64-efi -o "$WORK/BOOTX64.EFI" \
    "boot/grub/grub.cfg=$WORK/grub.cfg" >/dev/null

truncate -s 64M "$WORK/esp.img"
mformat -i "$WORK/esp.img" -F ::
mmd -i "$WORK/esp.img" ::/EFI ::/EFI/BOOT
mcopy -i "$WORK/esp.img" "$WORK/BOOTX64.EFI" ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "$WORK/esp.img" "$KERNEL" ::/kernel.elf
cp "$OVMF_VARS" "$WORK/vars.fd"

qemu-system-x86_64 \
    -M q35 \
    -m "${RETROOS_MEMORY_MB:-512}" \
    -cpu max \
    -drive "if=pflash,format=raw,readonly=on,file=$OVMF_CODE" \
    -drive "if=pflash,format=raw,file=$WORK/vars.fd" \
    -nodefaults \
    -device bochs-display \
    -drive "file=$IMAGE,if=none,id=hd,format=raw,snapshot=on" \
    -device nvme,drive=hd,serial=retro1 \
    -drive "file=$WORK/esp.img,if=none,id=esp,format=raw" \
    -device nvme,drive=esp,serial=esp0 \
    -device qemu-xhci \
    -debugcon stdio \
    -display "${QEMU_DISPLAY:-sdl}" \
    -no-reboot \
    "$@"
