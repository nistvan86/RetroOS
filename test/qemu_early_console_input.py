#!/usr/bin/env python3
"""Exercise serial input, echo, and resume from the early console."""

from __future__ import annotations

import sys

from qemu_console_test import QemuSerialTest


def main() -> int:
    exec_mode = len(sys.argv) > 1 and sys.argv[1] == "exec"
    with QemuSerialTest() as qemu:
        qemu.read_until(b"early> ")

        qemu.send(b"help\r")
        qemu.read_until(b"commands: help info resume reboot")
        qemu.require(b"help\r\n")

        if exec_mode:
            qemu.send(b"exec TESTS/SBTEST.COM\r")
            qemu.read_until(b"BUSY-OK")
            qemu.require(b"Starting TESTS/SBTEST.COM")
            if b"Starting DN" in qemu.output:
                raise AssertionError(f"default DN path was selected: {bytes(qemu.output)!r}")
        else:
            qemu.send(b"resume\r")
            qemu.read_until(b"Block devices initialized")
            qemu.require(b"Ring1 entered")
    print("PASS: QEMU early-console exec" if exec_mode else "PASS: QEMU early-console input")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
