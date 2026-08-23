#!/bin/sh
set -eu

bazelisk build //:image
python3 test/qemu_early_console_serial_handoff.py
