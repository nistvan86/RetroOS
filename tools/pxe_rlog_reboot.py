#!/usr/bin/env python3
"""Send RetroOS's authenticated-by-physical-link raw reboot command."""

import argparse
import socket


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--interface", default="eth0")
    args = parser.parse_args()

    source = open(f"/sys/class/net/{args.interface}/address", encoding="ascii").read().strip()
    source_bytes = bytes.fromhex(source.replace(":", ""))
    frame = (
        b"\xff" * 6
        + source_bytes
        + b"\x88\xb5"
        + b"RCTL\x01\x01REBOOT"
    )
    frame += b"\x00" * (60 - len(frame))
    with socket.socket(socket.AF_PACKET, socket.SOCK_RAW) as sock:
        sock.bind((args.interface, 0))
        sock.send(frame)
    print(f"RetroOS reboot frame sent on {args.interface} from {source}")


if __name__ == "__main__":
    main()
