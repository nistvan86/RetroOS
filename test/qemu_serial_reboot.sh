#!/bin/sh
set -eu

bazelisk build //:image
python3 test/qemu_early_console_serial_handoff.py keys-early
python3 test/qemu_early_console_serial_handoff.py reboot-early
python3 test/qemu_early_console_serial_handoff.py reboot-dos
