#!/usr/bin/env python3
"""Exercise early-console exec with a stub served from HostFS."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
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



def connect_console(path: str) -> socket.socket:
    connection = None
    deadline = time.monotonic() + 15.0
    while connection is None and time.monotonic() < deadline:
        try:
            connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            connection.connect(path)
            connection.settimeout(0.25)
        except OSError:
            if connection is not None:
                connection.close()
            connection = None
            time.sleep(0.05)
    if connection is None:
        raise AssertionError("QEMU console socket did not become available")
    return connection


def assemble_probe(name: str, output: str) -> None:
    source = Path(__file__).with_name("dos") / "console_hostfs" / f"{name}.asm"
    subprocess.run(["nasm", "-f", "bin", "-o", output, str(source)], check=True)


def run_dos_echo_probe(root: str, directory: str) -> None:
    console_socket = os.path.join(directory, "echo-console.sock")
    hostfs_socket = os.path.join(directory, "echo-hostfs.sock")
    listening = b"DOS-ECHO-LISTENING"
    marker = b"DOS-ECHO-OK"
    assemble_probe("echo", os.path.join(root, "ECHO.COM"))
    hostfs = subprocess.Popen(
        ["python3", "hostfs.py", root, hostfs_socket],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    qemu = subprocess.Popen(
        [
            "qemu-system-i386", "-cpu", "486",
            "-drive", "file=bazel-bin/image.bin,format=raw,snapshot=on",
            "-m", "64M", "-display", "none", "-no-reboot",
            "-serial", f"unix:{console_socket},server=on,wait=on",
            "-chardev", f"socket,id=hostfs,path={hostfs_socket},server=on,wait=on",
            "-device", "isa-serial,chardev=hostfs,index=1",
            "-fw_cfg", "name=opt/cmdline,string=serial=com1 hostfs=com2 console=early",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        with connect_console(console_socket) as connection:
            output = bytearray()
            read_until(connection, b"early> ", output)
            connection.sendall(b"exec --and-halt /host/ECHO.COM\r")
            read_until(connection, b"Starting /host/ECHO.COM", output)
            read_until(connection, listening, output)
            before_key = len(output)
            connection.sendall(b"q")  # raw serial ASCII, translated by DOS adapter
            read_until(connection, marker, output)
            after_key = bytes(output[before_key:])
            if b"q" not in after_key:
                raise AssertionError(f"DOS input was not echoed: {after_key!r}")
            if output.count(marker) != 1:
                raise AssertionError(f"DOS echo marker was duplicated: {bytes(output)!r}")
    finally:
        qemu.terminate()
        hostfs.terminate()
        for process in (qemu, hostfs):
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


def run_personality_probe(
    root: str,
    directory: str,
    source: str,
    guest_name: str,
    listening: bytes,
    marker: bytes,
) -> None:
    console_socket = os.path.join(directory, f"{guest_name}.console.sock")
    hostfs_socket = os.path.join(directory, f"{guest_name}.hostfs.sock")
    shutil.copyfile(source, os.path.join(root, guest_name))
    hostfs = subprocess.Popen(
        ["python3", "hostfs.py", root, hostfs_socket],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    qemu = subprocess.Popen(
        [
            "qemu-system-i386", "-cpu", "486",
            "-drive", "file=bazel-bin/image.bin,format=raw,snapshot=on",
            "-m", "64M", "-display", "none", "-no-reboot",
            "-serial", f"unix:{console_socket},server=on,wait=on",
            "-chardev", f"socket,id={guest_name}hostfs,path={hostfs_socket},server=on,wait=on",
            "-device", f"isa-serial,chardev={guest_name}hostfs,index=1",
            "-fw_cfg", "name=opt/cmdline,string=serial=com1 hostfs=com2 console=early",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        with connect_console(console_socket) as connection:
            output = bytearray()
            read_until(connection, b"early> ", output)
            connection.sendall(f"exec --and-halt /host/{guest_name}\r".encode())
            read_until(connection, b"Starting /host/" + guest_name.encode(), output)
            read_until(connection, listening, output)
            before_key = len(output)
            connection.sendall(b"q")
            read_until(connection, marker, output)
            if b"q" not in bytes(output[before_key:]):
                raise AssertionError(f"{guest_name} input was not echoed: {bytes(output)!r}")
    finally:
        qemu.terminate()
        hostfs.terminate()
        for process in (qemu, hostfs):
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="retroos-qemu-early-hostfs-") as directory:
        root = os.path.join(directory, "root")
        os.mkdir(root)
        console_socket = os.path.join(directory, "console.sock")
        hostfs_socket = os.path.join(directory, "hostfs.sock")

        marker = b"EARLY-EXEC-HOSTFS-OK"
        assemble_probe("early_exec", os.path.join(root, "STUB.COM"))

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
                "-fw_cfg", "name=opt/cmdline,string=serial=com1 hostfs=com2 console=early",
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
                if output.count(marker[:-1]) != 1:
                    raise AssertionError(f"HostFS marker was duplicated: {bytes(output)!r}")
                if b"Starting /host/STUB.COM" not in output:
                    raise AssertionError(f"HostFS executable was not selected: {bytes(output)!r}")
                read_until(connection, b"Starting DN...", output)
                if "All commands done — shutting down.".encode() in output:
                    raise AssertionError(f"default exec halted unexpectedly: {bytes(output)!r}")
        finally:
            qemu.terminate()
            hostfs.terminate()
            for process in (qemu, hostfs):
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)

        run_dos_echo_probe(root, directory)
        run_personality_probe(
            root,
            directory,
            "bazel-out/k8-opt/bin/test/linux/hello/ECHO.ELF",
            "LINUXECHO.ELF",
            b"LINUX-ECHO-LISTENING",
            b"LINUX-ECHO-OK",
        )
        run_personality_probe(
            root,
            directory,
            "bazel-out/k8-opt/bin/test/os2/hello/console_echo.exe",
            "OS2ECHO.EXE",
            b"OS2-ECHO-LISTENING",
            b"OS2-ECHO-OK",
        )
        run_personality_probe(
            root,
            directory,
            "bazel-out/k8-opt/bin/test/windows/hello/console_echo.exe",
            "WINECHO.EXE",
            b"WIN-ECHO-LISTENING",
            b"WIN-ECHO-OK",
        )

    print("PASS: QEMU early-console HostFS exec, DOS, Linux, OS/2, and Win32 echo")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, OSError) as error:
        print(f"FAIL: QEMU early-console HostFS exec: {error}", file=sys.stderr)
        raise SystemExit(1)
