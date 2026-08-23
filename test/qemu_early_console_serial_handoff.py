#!/usr/bin/env python3
"""Verify serial early-console output and the return to ambient logging."""

from __future__ import annotations

from qemu_console_test import QemuSerialTest


def main() -> int:
    with QemuSerialTest() as qemu:
        qemu.read_until(b"early> ")

        qemu.send(b"help\r")
        qemu.read_until(b"commands: help info resume reboot")
        qemu.send(b"info\r")
        qemu.read_until(b"early console: paging active")
        qemu.send(b"resume\r")
        qemu.read_until(b"Welcome to RetroOS!")
        qemu.read_until(b"Starting DN...")

        qemu.require(b"help\r\n", b"info\r\n", b"resume\r\n", b"early> resume")
    print("PASS: QEMU early-console serial handoff")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
