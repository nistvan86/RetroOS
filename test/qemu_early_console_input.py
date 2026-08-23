#!/usr/bin/env python3
"""Exercise serial input, echo, and resume from the early console."""

from __future__ import annotations

import os
import socket
import subprocess
import sys
import tempfile
import time


def read_until(sock: socket.socket, marker: bytes, data: bytearray) -> None:
    deadline = time.monotonic() + 15.0
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
    exec_mode = len(sys.argv) > 1 and sys.argv[1] == "exec"
    with tempfile.TemporaryDirectory(prefix="retroos-qemu-early-input-") as directory:
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
            deadline = time.monotonic() + 10.0
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
                if b"help\r\n" not in output:
                    raise AssertionError(f"input was not echoed: {bytes(output)!r}")

                if exec_mode:
                    connection.sendall(b"exec TESTS/SBTEST.COM\r")
                    read_until(connection, b"BUSY-OK", output)
                    if b"Starting TESTS/SBTEST.COM" not in output:
                        raise AssertionError(f"selected executable did not start: {bytes(output)!r}")
                    if b"Starting DN" in output:
                        raise AssertionError(f"default DN path was selected: {bytes(output)!r}")
                else:
                    connection.sendall(b"resume\r")
                    read_until(connection, b"Block devices initialized", output)
                    if b"Ring1 entered" not in output:
                        raise AssertionError(f"normal boot did not resume: {bytes(output)!r}")
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)

    print("PASS: QEMU early-console exec" if exec_mode else "PASS: QEMU early-console input")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, OSError) as error:
        print(f"FAIL: QEMU early-console input: {error}", file=sys.stderr)
        raise SystemExit(1)
