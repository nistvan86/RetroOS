#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
skill_repo="$(cd "$script_dir/../../../../" && pwd)"
repo_root="${RETROOS_REPO_ROOT:-$skill_repo}"
receiver="$repo_root/tools/pxe_rlog_receiver.py"
host="${RETROOS_PXE_HOST:-retroos-pi}"
interface="${RETROOS_PXE_INTERFACE:-eth0}"
output="${RETROOS_RLOG_REMOTE_OUTPUT:-/tmp/retroos-rlog-current.log}"

if [[ ! -f "$receiver" ]]; then
    printf 'RLOG receiver not found: %s\n' "$receiver" >&2
    exit 2
fi

exec ssh "$host" \
    "cd /tmp && sudo -n python3 -u /dev/stdin --interface '$interface' --output '$output'" \
    < "$receiver"
