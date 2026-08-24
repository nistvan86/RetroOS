#!/usr/bin/env python3
"""Exercise serial early-console handoff and reboot control frames."""

from __future__ import annotations

import sys

from qemu_console_test import QemuSerialTest


def frame(payload: bytes) -> bytes:
    escaped = payload.replace(b"\x10", b"\x10\x10")
    return b"\x10\x02" + escaped + b"\x10\x03"


def handoff() -> int:
    with QemuSerialTest() as qemu:
        qemu.read_until(b"early> ")
        qemu.send(b"help\r")
        qemu.read_until(b"commands: help info boot reboot")
        qemu.send(b"info\r")
        qemu.read_until(b"early console: paging active")
        qemu.send(b"boot\r")
        qemu.read_until(b"Welcome to RetroOS!")
        qemu.read_until(b"Starting DN...")
        qemu.require(b"help\r\n", b"info\r\n", b"boot\r\n", b"early> boot")
    print("PASS: QEMU early-console serial handoff")
    return 0


def ctrl_alt_delete_sequence() -> bytes:
    events = (
        (0, 0x1D),  # Ctrl down
        (0, 0x38),  # Alt down
        (0, 0xE0),  # Delete extended prefix down
        (0, 0x53),  # Delete down
        (1, 0xE0),  # Delete extended prefix up
        (1, 0x53),  # Delete up
        (1, 0x38),  # Alt up
        (1, 0x1D),  # Ctrl up
    )
    return b"".join(frame(bytes((0x02, action, scancode)))
                   for action, scancode in events)


def key_sequence_early() -> int:
    with QemuSerialTest() as qemu:
        qemu.read_until(b"early> ")
        qemu.send(ctrl_alt_delete_sequence())
        qemu.send(b"help\r")
        qemu.read_until(b"commands: help info boot reboot")
    print("PASS: QEMU serial Ctrl-Alt-Delete make/break sequence")
    return 0


def key_sequence_kernel() -> int:
    with QemuSerialTest(cmdline="serial=com1 console=kernel") as qemu:
        qemu.read_until(b"kernel> ")
        qemu.send(ctrl_alt_delete_sequence())
        qemu.send(b"help\r")
        qemu.read_until(b"commands: help info boot reboot")
        qemu.require(b"kernel> help")
    print("PASS: QEMU serial Ctrl-Alt-Delete make/break sequence in kernel console")
    return 0


def panic_early() -> int:
    with QemuSerialTest() as qemu:
        qemu.read_until(b"early> ")
        qemu.send(b"panic\r")
        qemu.read_until(b"!!! KERNEL PANIC !!!")
        qemu.read_until(b"kernel console panic requested")
    print("PASS: QEMU panic emergency serial output")
    return 0


def panic_dos() -> int:
    with QemuSerialTest() as qemu:
        qemu.read_until(b"early> ")
        qemu.send(b"boot\r")
        qemu.read_until(b"Starting DN...")
        qemu.send(frame(b"\x04"))  # PANIC control command
        qemu.read_until(b"!!! KERNEL PANIC !!!")
        qemu.read_until(b"serial control: panic requested")
    print("PASS: QEMU panic emergency serial output with DOS personality")
    return 0


def panic_kernel() -> int:
    with QemuSerialTest(cmdline="serial=com1 console=kernel") as qemu:
        qemu.read_until(b"kernel> ")
        qemu.send(b"panic\r")
        qemu.read_until(b"!!! KERNEL PANIC !!!")
        qemu.read_until(b"kernel console panic requested")
    print("PASS: QEMU panic emergency serial output from kernel console")
    return 0


def reboot_kernel() -> int:
    with QemuSerialTest(cmdline="serial=com1 console=kernel", no_reboot=False) as qemu:
        qemu.read_until(b"kernel> ")
        qemu.send(frame(b"\x01"))
        qemu.read_until_count(b"RetroOS Rust Kernel", 2)
        qemu.read_until_count(b"kernel> ", 2)
    print("PASS: QEMU serial reboot from kernel console")
    return 0


def reboot_early() -> int:
    with QemuSerialTest(no_reboot=False) as qemu:
        qemu.read_until(b"early> ")
        qemu.send(frame(b"\x01"))  # REBOOT control command
        qemu.read_until_count(b"RetroOS early console", 2)
        qemu.require(b"type help for commands")
    print("PASS: QEMU serial reboot from early console")
    return 0


def reboot_dn() -> int:
    with QemuSerialTest(no_reboot=False) as qemu:
        qemu.read_until(b"early> ")
        qemu.send(b"boot\r")
        qemu.read_until(b"Starting DN...")
        qemu.send(ctrl_alt_delete_sequence())
        qemu.send(frame(b"\x01"))  # REBOOT control command
        qemu.read_until_count(b"RetroOS Rust Kernel", 2)
        # The fw_cfg command line still requests console=early after reset, so
        # the restarted guest stops at the new early prompt again.
        qemu.read_until_count(b"RetroOS early console", 2)
    print("PASS: QEMU serial reboot with DOS personality")
    return 0


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else "handoff"
    if mode == "keys-early":
        return key_sequence_early()
    if mode == "keys-kernel":
        return key_sequence_kernel()
    if mode == "panic-early":
        return panic_early()
    if mode == "panic-dos":
        return panic_dos()
    if mode == "panic-kernel":
        return panic_kernel()
    if mode == "reboot-kernel":
        return reboot_kernel()
    if mode == "reboot-early":
        return reboot_early()
    if mode == "reboot-dos":
        return reboot_dn()
    if mode != "handoff":
        raise SystemExit(f"unknown mode: {mode}")
    return handoff()


if __name__ == "__main__":
    raise SystemExit(main())
