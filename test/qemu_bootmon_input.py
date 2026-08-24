#!/usr/bin/env python3
"""Exercise serial input, echo, and boot from the early console."""

from __future__ import annotations

import sys

from qemu_console_test import QemuSerialTest



def default_mode() -> int:
    with QemuSerialTest(cmdline="serial=com1") as qemu:
        qemu.read_until(b"Welcome to RetroOS!")
        qemu.read_until(b"Starting DN...")
        if b"kernel> " in qemu.output or b"bootmon> " in qemu.output:
            raise AssertionError(f"default startup entered the boot monitor: {bytes(qemu.output)!r}")
    print("PASS: QEMU default startup")
    return 0


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else "early"

    if mode == "default":
        return default_mode()
    exec_mode = mode == "exec"
    with QemuSerialTest() as qemu:
        qemu.read_until(b"bootmon> ")

        qemu.send(b"help\r")
        qemu.read_until(b"commands: boot, init")
        qemu.require(b"help\r\n")

        if exec_mode:
            qemu.send(b"exec TESTS/SBTEST.COM\r")
            qemu.read_until(b"BUSY-OK")
            qemu.require(b"Starting TESTS/SBTEST.COM")
            if b"Starting DN" in qemu.output:
                raise AssertionError(f"default DN path was selected: {bytes(qemu.output)!r}")
        else:
            qemu.send(b"boot\r")
            qemu.read_until(b"Block devices initialized")
            qemu.require(b"Ring1 entered")
    print("PASS: QEMU early-console exec" if exec_mode else "PASS: QEMU early-console input")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
