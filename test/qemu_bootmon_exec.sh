#!/bin/bash
# QEMU boot-monitor executable override smoke test.
set -euo pipefail
cd "$(dirname "$0")/.."
bazelisk build //:image >/dev/null
python3 test/qemu_bootmon_input.py exec
