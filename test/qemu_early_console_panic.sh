#!/bin/sh
set -eu

bazelisk build //:image
python3 test/qemu_early_console_serial_handoff.py panic-early
python3 test/qemu_early_console_serial_handoff.py panic-dos
