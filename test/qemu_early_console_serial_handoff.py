#!/usr/bin/env python3
"""Verify serial early-console output and the return to ambient logging."""

from __future__ import annotations

import os
import socket
import subprocess
import tempfile
import time


def read_until(sock: socket.socket, marker: bytes, data: bytearray) -> None:
    deadline = time.monotonic() + 20.0
    while marker not in data and time.monotonic() < deadline:
        try:
            chunk = sock.recv(4096)
        except socket.timeout:
            continue
        if not chunk:
            break
        data.extend(chunk)
    if marker not in data:
        raise AssertionError(f"did not receive {marker!r}; got {bytes(data)!r}")


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="retroos-qemu-serial-handoff-") as directory:
        serial_socket = os.path.join(directory, "serial.sock")
        debug_log = os.path.join(directory, "debug.log")
        command = [
            "qemu-system-i386",
            "-cpu", "486",
            "-drive", "file=bazel-bin/image.bin,format=raw,snapshot=on",
            "-m", "64M",
            "-display", "none",
            "-no-reboot",
            "-serial", f"unix:{serial_socket},server=on,wait=on",
            "-debugcon", f"file:{debug_log}",
            "-fw_cfg", "name=opt/cmdline,string=serial=com1 earlyconsole",
        ]
        process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            connection = None
            deadline = time.monotonic() + 15.0
            while connection is None and time.monotonic() < deadline:
                try:
                    connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                    connection.connect(serial_socket)
                    connection.settimeout(0.25)
                except OSError:
                    if connection is not None:
                        connection.close()
                    connection = None
                    time.sleep(0.05)
            if connection is None:
                raise AssertionError("QEMU serial socket did not become available")

            with connection:
                output = bytearray()
                read_until(connection, b"early> ", output)

                connection.sendall(b"help\r")
                read_until(connection, b"commands: help info resume reboot", output)
                connection.sendall(b"info\r")
                read_until(connection, b"early console: paging active", output)
                connection.sendall(b"resume\r")
                read_until(connection, b"Welcome to RetroOS!", output)
                read_until(connection, b"Starting DN...", output)

                required = [b"help\r\n", b"info\r\n", b"resume\r\n"]
                for item in required:
                    if item not in output:
                        raise AssertionError(f"serial echo missing {item!r}: {bytes(output)!r}")
                if b"early> resume" not in output:
                    raise AssertionError(f"resume was not echoed by the early session: {bytes(output)!r}")
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)

    print("PASS: QEMU early-console serial handoff")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
