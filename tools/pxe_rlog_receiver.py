#!/usr/bin/env python3
"""Receive RetroOS RLOG frames from a directly connected PXE client."""

import argparse
import datetime
import socket
import struct
import sys
from pathlib import Path

ETH_P_ALL = 0x0003
ETHERTYPE_RLOG = b"\x88\xb5"
PACKET_OUTGOING = 4
HEADER = struct.Struct("!4sBBIIH")


def timestamp() -> str:
    """Local wall-clock timestamp with millisecond precision."""
    return datetime.datetime.now().astimezone().isoformat(timespec="milliseconds")


def mac(data: bytes) -> str:
    return ":".join(f"{byte:02x}" for byte in data)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--interface", default="eth0")
    parser.add_argument("--output", type=Path, help="append payload bytes to this file")
    args = parser.parse_args()

    output = args.output
    if output is None:
        stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
        output = Path(f"retroos-rlog-{stamp}.log")

    sock = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(ETH_P_ALL))
    sock.bind((args.interface, 0))
    print(
        f"{timestamp()} RLOG listener ready: {args.interface} "
        f"EtherType=0x88B5 output={output}",
        flush=True,
    )

    expected: dict[tuple[bytes, int], int] = {}
    with output.open("ab", buffering=0) as log:
        while True:
            frame, address = sock.recvfrom(2048)
            if address[2] == PACKET_OUTGOING or len(frame) < 14 + HEADER.size:
                continue
            if frame[12:14] != ETHERTYPE_RLOG:
                continue
            magic, version, flags, session, sequence, length = HEADER.unpack_from(frame, 14)
            if magic != b"RLOG" or version != 1 or length > len(frame) - 14 - HEADER.size:
                continue
            source = frame[6:12]
            key = (source, session)
            want = expected.get(key, sequence)
            marker = "" if sequence == want else f" [expected {want}]"
            expected[key] = (sequence + 1) & 0xFFFFFFFF
            payload = frame[14 + HEADER.size : 14 + HEADER.size + length]
            log.write(payload)
            if flags == 2 and len(payload) == 16 and payload[:4] == b"ISRB":
                values = struct.unpack("!6H", payload[4:])
                text = (
                    f"ISR1 A={values[0]:04X} S={values[1]:04X} F={values[2]:04X}; "
                    f"ISR2 A={values[3]:04X} S={values[4]:04X} F={values[5]:04X}"
                )
            else:
                text = payload.decode("utf-8", errors="backslashreplace").rstrip("\n")
            print(
                f"{timestamp()} {mac(source)} session={session:08x} "
                f"seq={sequence}{marker} "
                f"flags={flags:02x} {text}",
                flush=True,
            )


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print(f"\n{timestamp()} RLOG listener stopped", file=sys.stderr)
