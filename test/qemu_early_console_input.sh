#!/bin/bash
# QEMU early-console interactive input and resume smoke test.
set -euo pipefail
cd "$(dirname "$0")/.."
bazelisk build //:image >/dev/null
python3 test/qemu_early_console_input.py
