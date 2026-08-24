#!/bin/bash
# QEMU kernel-console input, default-startup, and early boot smoke tests.
set -euo pipefail
cd "$(dirname "$0")/.."
bazelisk build //:image >/dev/null
python3 test/qemu_early_console_input.py default
python3 test/qemu_early_console_input.py kernel
python3 test/qemu_early_console_input.py
