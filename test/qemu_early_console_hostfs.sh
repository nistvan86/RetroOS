#!/bin/bash
# QEMU early-console HostFS executable override smoke test.
set -euo pipefail
cd "$(dirname "$0")/.."
bazelisk build //:image \
  //test/os2/hello:hello_lx \
  //apps/os2/doscalls:doscalls_dll \
  //test/windows/hello:hello \
  //apps/windows/kernel32:kernel32_dll \
  //apps/windows/user32:user32_dll >/dev/null
python3 test/qemu_early_console_hostfs.py
