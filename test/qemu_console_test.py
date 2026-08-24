"""Shared QEMU serial-console interaction harness."""

from __future__ import annotations

import os
import socket
import subprocess
import tempfile
import time


class QemuSerialTest:
    def __init__(self, cmdline: str = "serial=com1 console=early", no_reboot: bool = True):
        self.cmdline = cmdline
        self.no_reboot = no_reboot
        self.output = bytearray()
        self._directory: tempfile.TemporaryDirectory[str] | None = None
        self._process: subprocess.Popen[bytes] | None = None
        self._connection: socket.socket | None = None

    def __enter__(self) -> "QemuSerialTest":
        self._directory = tempfile.TemporaryDirectory(prefix="retroos-qemu-console-")
        serial_socket = os.path.join(self._directory.name, "serial.sock")
        command = [
            "qemu-system-i386",
            "-cpu", "486",
            "-drive", "file=bazel-bin/image.bin,format=raw,snapshot=on",
            "-m", "64M",
            "-display", "none",
            "-serial", f"unix:{serial_socket},server=on,wait=on",
            "-fw_cfg", f"name=opt/cmdline,string={self.cmdline}",
        ]
        if self.no_reboot:
            command.insert(command.index("-serial"), "-no-reboot")
        self._process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.monotonic() + 15.0
        while time.monotonic() < deadline:
            try:
                self._connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                self._connection.connect(serial_socket)
                self._connection.settimeout(0.25)
                return self
            except OSError:
                if self._connection is not None:
                    self._connection.close()
                    self._connection = None
                time.sleep(0.05)
        raise AssertionError("QEMU serial socket did not become available")

    def __exit__(self, *_: object) -> None:
        if self._connection is not None:
            self._connection.close()
        if self._process is not None:
            self._process.terminate()
            try:
                self._process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self._process.kill()
                self._process.wait(timeout=5)
        if self._directory is not None:
            self._directory.cleanup()

    def read_until(self, marker: bytes) -> None:
        if self._connection is None:
            raise AssertionError("QEMU serial connection is not open")
        deadline = time.monotonic() + 20.0
        while marker not in self.output and time.monotonic() < deadline:
            try:
                chunk = self._connection.recv(4096)
            except socket.timeout:
                continue
            if not chunk:
                break
            self.output.extend(chunk)
        if marker not in self.output:
            raise AssertionError(f"did not receive {marker!r}; got {bytes(self.output)!r}")

    def read_until_count(self, marker: bytes, count: int) -> None:
        if self._connection is None:
            raise AssertionError("QEMU serial connection is not open")
        deadline = time.monotonic() + 30.0
        while self.output.count(marker) < count and time.monotonic() < deadline:
            try:
                chunk = self._connection.recv(4096)
            except socket.timeout:
                continue
            if not chunk:
                break
            self.output.extend(chunk)
        if self.output.count(marker) < count:
            raise AssertionError(
                f"did not receive {count} copies of {marker!r}; got {bytes(self.output)!r}"
            )

    def send(self, data: bytes) -> None:
        if self._connection is None:
            raise AssertionError("QEMU serial connection is not open")
        self._connection.sendall(data)

    def require(self, *markers: bytes) -> None:
        for marker in markers:
            if marker not in self.output:
                raise AssertionError(f"serial output missing {marker!r}: {bytes(self.output)!r}")
