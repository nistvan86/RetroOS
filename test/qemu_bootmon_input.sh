#!/bin/bash
# QEMU boot-monitor input and default-startup smoke tests.
set -euo pipefail
cd "$(dirname "$0")/.."
bazelisk build //:image >/dev/null
python3 test/qemu_bootmon_input.py default

python3 test/qemu_bootmon_input.py
