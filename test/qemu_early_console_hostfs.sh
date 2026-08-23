#!/bin/bash
# QEMU early-console HostFS executable override smoke test.
set -euo pipefail
cd "$(dirname "$0")/.."
bazelisk build //:image >/dev/null
python3 test/qemu_early_console_hostfs.py
