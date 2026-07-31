#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SKILL_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
readonly DISCOVERED_REPO="$(cd "${SKILL_DIR}/../../.." && pwd)"
readonly REPO_ROOT="${RETROOS_REPO_ROOT:-$DISCOVERED_REPO}"
readonly HOST="${RETROOS_PXE_HOST:-retroos-pi}"
readonly INTERFACE="${RETROOS_PXE_INTERFACE:-eth0}"
readonly REMOTE_TFTP="${RETROOS_TFTP_ROOT:-/srv/tftp}"
readonly REMOTE_GRUB_CFG="${REMOTE_TFTP}/grub/grub.cfg"
readonly REMOTE_RLOG_OUTPUT="${RETROOS_RLOG_REMOTE_OUTPUT:-/tmp/retroos-rlog-current.log}"
readonly DEPLOY="${REPO_ROOT}/.agents/skills/deploy-retroos-kernel/scripts/deploy_kernel.sh"
readonly REBOOT="${REPO_ROOT}/.agents/skills/reboot-retroos-sejt/scripts/reboot_sejt.sh"
readonly RECEIVER="${REPO_ROOT}/tools/pxe_rlog_receiver.py"
artifact="${REPO_ROOT}/bazel-bin/kernel/kernel_pxe_netlog.elf"
build_target="//kernel:kernel_elf_pxe_netlog"

usage() {
    printf 'Usage: %s [--exec embedded-vfs-path] [--target bazel-label --artifact path]\n' "${0##*/}" >&2
}

exec_path=
while [[ $# -gt 0 ]]; do
    case "$1" in
        --exec)
            [[ $# -ge 2 && -n "${2:-}" ]] || { usage; exit 2; }
            exec_path=$2
            shift 2
            ;;
        --target)
            [[ $# -ge 2 && -n "${2:-}" ]] || { usage; exit 2; }
            build_target=$2
            shift 2
            ;;
        --artifact)
            [[ $# -ge 2 && -n "${2:-}" ]] || { usage; exit 2; }
            artifact=$2
            [[ "$artifact" = /* ]] || artifact="${REPO_ROOT}/${artifact}"
            shift 2
            ;;
        *) usage; exit 2 ;;
    esac
done

if [[ "$build_target" != //* || "$build_target" == *[!A-Za-z0-9_./:+@-]* ]]; then
    printf 'Unsafe Bazel target: %s\n' "$build_target" >&2
    exit 2
fi

case "$artifact" in
    "${REPO_ROOT}"/*) ;;
    *)
        printf 'Artifact must be inside the repository: %s\n' "$artifact" >&2
        exit 2
        ;;
esac

if [[ -n "$exec_path" && ! "$exec_path" =~ ^[A-Za-z0-9_./:\\-]+$ ]]; then
    printf 'Unsafe retroos.exec path: %s\n' "$exec_path" >&2
    exit 2
fi

for helper in "$DEPLOY" "$REBOOT"; do
    [[ -x "$helper" ]] || {
        printf 'Required helper is missing or not executable: %s\n' "$helper" >&2
        exit 2
    }
done
[[ -f "$RECEIVER" ]] || {
    printf 'RLOG receiver not found: %s\n' "$RECEIVER" >&2
    exit 2
}

tmp_dir=
listener_pid=
cleanup() {
    if [[ -n "$listener_pid" ]] && kill -0 "$listener_pid" 2>/dev/null; then
        kill "$listener_pid" 2>/dev/null || true
        wait "$listener_pid" 2>/dev/null || true
    fi
    if [[ -n "$tmp_dir" && -d "$tmp_dir" ]]; then
        rm -rf -- "$tmp_dir"
    fi
}
trap cleanup EXIT HUP INT TERM

cd "$REPO_ROOT"

printf '==> Building ordinary kernel and %s\n' "$build_target"
bazelisk build //kernel:kernel_elf "$build_target"

[[ -f "$artifact" ]] || {
    printf 'Built artifact not found: %s\n' "$artifact" >&2
    exit 1
}

printf '%s\n' '==> Inspecting PXE runtime'
"$DEPLOY" --status

printf '%s\n' '==> Deploying PXE/RLOG kernel with the normal GRUB entry'
"$DEPLOY" "$artifact"

if [[ -n "$exec_path" ]]; then
    printf '==> Installing temporary retroos.exec GRUB entry: %s\n' "$exec_path"
    tmp_dir=$(mktemp -d)
    grub_cfg="${tmp_dir}/grub.cfg"
    {
        printf '%s\n' 'set timeout=0' 'set default=0' ''
        printf 'menuentry "RetroOS diagnostic: %s" {\n' "$exec_path"
        printf '%s\n' \
            '    terminal_output console' \
            '    insmod multiboot' \
            '' \
            "    multiboot /boot/retroos/kernel.elf retroos.exec=${exec_path}" \
            '    boot' \
            '}'
    } > "$grub_cfg"
    scp -- "$grub_cfg" "${HOST}:/tmp/retroos-grub-exec.cfg"
    ssh "$HOST" sudo -n install -m 0644 \
        /tmp/retroos-grub-exec.cfg "$REMOTE_GRUB_CFG"
fi

printf '%s\n' '==> Starting RLOG listener'
coproc RLOG_LISTENER {
    ssh "$HOST" \
        "cd /tmp && sudo -n python3 -u /dev/stdin --interface '$INTERFACE' --output '$REMOTE_RLOG_OUTPUT'" \
        < "$RECEIVER" 2>&1
}
listener_pid=$RLOG_LISTENER_PID
listener_fd=${RLOG_LISTENER[0]}

ready=false
while IFS= read -r line <&"$listener_fd"; do
    printf '%s\n' "$line"
    if [[ "$line" == *"RLOG listener ready"* ]]; then
        ready=true
        break
    fi
done
if ! $ready; then
    wait "$listener_pid" || true
    printf '%s\n' 'RLOG listener exited before becoming ready.' >&2
    exit 1
fi

printf '%s\n' '==> Sending one RCTL reboot request'
"$REBOOT"

printf '%s\n' '==> Listening for the new boot (Ctrl-C stops the listener)'
while IFS= read -r line <&"$listener_fd"; do
    printf '%s\n' "$line"
done
wait "$listener_pid"
