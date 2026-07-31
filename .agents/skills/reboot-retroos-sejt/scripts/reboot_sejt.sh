#!/usr/bin/env bash
set -euo pipefail

skill_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
host="${RETROOS_PXE_HOST:-retroos-pi}"
interface="${RETROOS_PXE_INTERFACE:-eth0}"

exec ssh "$host" \
    "sudo -n python3 - --interface '$interface'" \
    < "$skill_dir/scripts/send_reboot.py"
