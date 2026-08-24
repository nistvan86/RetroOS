#!/usr/bin/env python3
"""Exercise serial boot-monitor handoff and reboot control frames."""

from __future__ import annotations

import sys

from qemu_console_test import QemuSerialTest


def frame(payload: bytes) -> bytes:
    escaped = payload.replace(b"\x10", b"\x10\x10")
    return b"\x10\x02" + escaped + b"\x10\x03"


def handoff() -> int:
    with QemuSerialTest() as qemu:
        qemu.read_until(b"bootmon> ")
        qemu.send(b"help\r")
        qemu.read_until(b"commands: boot, init")
        qemu.send(b"info\r")
        qemu.read_until(b"boot monitor: paging active")
        qemu.send(b"boot\r")
        qemu.read_until(b"Welcome to RetroOS!")
        qemu.read_until(b"Starting DN...")
        qemu.require(b"help\r\n", b"info\r\n", b"boot\r\n", b"bootmon> boot")
    print("PASS: QEMU boot-monitor serial handoff")
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
        qemu.read_until(b"bootmon> ")
        qemu.send(ctrl_alt_delete_sequence())
        qemu.send(b"help\r")
        qemu.read_until(b"commands: boot, init")
    print("PASS: QEMU serial Ctrl-Alt-Delete make/break sequence")
    return 0



def panic_early() -> int:
    with QemuSerialTest() as qemu:
        qemu.read_until(b"bootmon> ")
        qemu.send(b"panic\r")
        qemu.read_until(b"!!! KERNEL PANIC !!!")
        qemu.read_until(b"kernel console panic requested")
    print("PASS: QEMU panic emergency serial output")
    return 0


def panic_dos() -> int:
    with QemuSerialTest() as qemu:
        qemu.read_until(b"bootmon> ")
        qemu.send(b"boot\r")
        qemu.read_until(b"Starting DN...")
        qemu.send(frame(b"\x04"))  # PANIC control command
        qemu.read_until(b"!!! KERNEL PANIC !!!")
        qemu.read_until(b"serial control: panic requested")
    print("PASS: QEMU panic emergency serial output with DOS personality")
    return 0



def reboot_early() -> int:
    with QemuSerialTest(no_reboot=False) as qemu:
        qemu.read_until(b"bootmon> ")
        qemu.send(frame(b"\x01"))  # REBOOT control command
        qemu.read_until_count(b"RetroOS boot monitor", 2)
        qemu.require(b"type help for commands")
    print("PASS: QEMU serial reboot from boot monitor")
    return 0


def reboot_dn() -> int:
    with QemuSerialTest(no_reboot=False) as qemu:
        qemu.read_until(b"bootmon> ")
        qemu.send(b"boot\r")
        qemu.read_until(b"Starting DN...")
        qemu.send(ctrl_alt_delete_sequence())
        qemu.send(frame(b"\x01"))  # REBOOT control command
        qemu.read_until_count(b"RetroOS Rust Kernel", 2)
        # The fw_cfg command line still requests bootmon after reset, so
        # the restarted guest stops at the new boot monitor prompt again.
        qemu.read_until_count(b"RetroOS boot monitor", 2)
    print("PASS: QEMU serial reboot with DOS personality")
    return 0


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else "handoff"
    if mode == "keys-early":
        return key_sequence_early()

    if mode == "panic-early":
        return panic_early()
    if mode == "panic-dos":
        return panic_dos()


    if mode == "reboot-early":
        return reboot_early()
    if mode == "reboot-dos":
        return reboot_dn()
    if mode != "handoff":
        raise SystemExit(f"unknown mode: {mode}")
    return handoff()


if __name__ == "__main__":
    raise SystemExit(main())
