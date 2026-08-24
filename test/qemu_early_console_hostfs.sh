#!/bin/bash
# QEMU early-console HostFS executable override smoke test.
set -euo pipefail
cd "$(dirname "$0")/.."
bazelisk build //:image \
  //test/linux/hello:echo \
  //test/os2/hello:hello_lx \
  //test/os2/hello:console_echo \
  //apps/os2/doscalls:doscalls_dll \
  //test/windows/hello:hello \
  //test/windows/hello:console_echo \
  //apps/windows/kernel32:kernel32_dll \
  //apps/windows/user32:user32_dll >/dev/null
python3 test/qemu_early_console_hostfs.py
