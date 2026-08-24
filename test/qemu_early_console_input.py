#!/usr/bin/env python3
"""Exercise serial input, echo, and boot from the early console."""

from __future__ import annotations

import sys

from qemu_console_test import QemuSerialTest


def kernel_mode() -> int:
    with QemuSerialTest(cmdline="serial=com1 console=kernel") as qemu:
        qemu.read_until(b"kernel> ")
        qemu.require(b"Ring1 entered")
        if b"Starting DN..." in qemu.output:
            raise AssertionError(f"kernel console launched DN prematurely: {bytes(qemu.output)!r}")
        qemu.send(b"boot\r")
        qemu.read_until(b"boot is only available in the early boot console")
        qemu.require(b"kernel> boot")
        qemu.send(b"exec TESTS/SBTEST.COM\r")
        qemu.read_until(b"BUSY-OK")
        if b"Starting DN..." in qemu.output:
            raise AssertionError(f"kernel console exec selected DN: {bytes(qemu.output)!r}")
    print("PASS: QEMU kernel-ready console")
    return 0


def default_mode() -> int:
    with QemuSerialTest(cmdline="serial=com1") as qemu:
        qemu.read_until(b"Welcome to RetroOS!")
        qemu.read_until(b"Starting DN...")
        if b"kernel> " in qemu.output or b"early> " in qemu.output:
            raise AssertionError(f"default startup entered a kernel console: {bytes(qemu.output)!r}")
    print("PASS: QEMU default startup")
    return 0


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else "early"
    if mode == "kernel":
        return kernel_mode()
    if mode == "default":
        return default_mode()
    exec_mode = mode == "exec"
    with QemuSerialTest() as qemu:
        qemu.read_until(b"early> ")

        qemu.send(b"help\r")
        qemu.read_until(b"commands: help info boot reboot")
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
