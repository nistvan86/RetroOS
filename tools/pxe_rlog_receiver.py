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
    print(f"RLOG listener ready: {args.interface} EtherType=0x88B5 output={output}", flush=True)

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
            text = payload.decode("utf-8", errors="backslashreplace").rstrip("\n")
            print(
                f"{mac(source)} session={session:08x} seq={sequence}{marker} "
                f"flags={flags:02x} {text}",
                flush=True,
            )


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("\nRLOG listener stopped", file=sys.stderr)
