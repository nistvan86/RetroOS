#!/usr/bin/env python3
"""Exercise early-console exec with a stub served from HostFS."""

from __future__ import annotations

import os
import socket
import subprocess
import sys
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
    with tempfile.TemporaryDirectory(prefix="retroos-qemu-early-hostfs-") as directory:
        root = os.path.join(directory, "root")
        os.mkdir(root)
        console_socket = os.path.join(directory, "console.sock")
        hostfs_socket = os.path.join(directory, "hostfs.sock")

        # DOS COM stub: print EARLY-EXEC-HOSTFS-OK through INT 21h/AH=09,
        # then terminate through INT 21h/AH=4C.
        marker = b"EARLY-EXEC-HOSTFS-OK$"
        stub = bytes((0xB4, 0x09, 0xBA, 0x0C, 0x01, 0xCD, 0x21,
                      0xB8, 0x00, 0x4C, 0xCD, 0x21)) + marker
        with open(os.path.join(root, "STUB.COM"), "wb") as output:
            output.write(stub)

        hostfs = subprocess.Popen(
            ["python3", "hostfs.py", root, hostfs_socket],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        qemu = subprocess.Popen(
            [
                "qemu-system-i386",
                "-cpu", "486",
                "-drive", "file=bazel-bin/image.bin,format=raw,snapshot=on",
                "-m", "64M",
                "-display", "none",
                "-no-reboot",
                "-serial", f"unix:{console_socket},server=on,wait=on",
                "-chardev", f"socket,id=hostfs,path={hostfs_socket},server=on,wait=on",
                "-device", "isa-serial,chardev=hostfs,index=1",
                "-fw_cfg", "name=opt/cmdline,string=serial=com1 hostfs=com2 earlyconsole",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        try:
            connection = None
            deadline = time.monotonic() + 15.0
            while connection is None and time.monotonic() < deadline:
                try:
                    connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                    connection.connect(console_socket)
                    connection.settimeout(0.25)
                except OSError:
                    if connection is not None:
                        connection.close()
                    connection = None
                    time.sleep(0.05)
            if connection is None:
                raise AssertionError("QEMU console socket did not become available")

            with connection:
                output = bytearray()
                read_until(connection, b"early> ", output)
                connection.sendall(b"exec /host/STUB.COM\r")
                read_until(connection, marker[:-1], output)
                if b"Starting /host/STUB.COM" not in output:
                    raise AssertionError(f"HostFS executable was not selected: {bytes(output)!r}")
                if b"Starting DN" in output:
                    raise AssertionError(f"DN was selected instead: {bytes(output)!r}")
        finally:
            qemu.terminate()
            hostfs.terminate()
            for process in (qemu, hostfs):
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)

    print("PASS: QEMU early-console HostFS exec")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, OSError) as error:
        print(f"FAIL: QEMU early-console HostFS exec: {error}", file=sys.stderr)
        raise SystemExit(1)
