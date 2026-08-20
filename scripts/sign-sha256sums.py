#!/usr/bin/env python3
"""Sign SHA256SUMS with the local Ed25519 seed (keys/update-ed25519.seed)."""

from __future__ import annotations

import argparse
import pathlib
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("sums", type=pathlib.Path)
    parser.add_argument(
        "--seed",
        type=pathlib.Path,
        default=pathlib.Path("keys/update-ed25519.seed"),
    )
    parser.add_argument(
        "-o",
        "--output",
        type=pathlib.Path,
        default=None,
    )
    args = parser.parse_args()
    seed_hex = args.seed.read_text().strip()
    seed = bytes.fromhex(seed_hex)
    if len(seed) != 32:
        print("seed must be 32 bytes hex", file=sys.stderr)
        return 1
    key = Ed25519PrivateKey.from_private_bytes(seed)
    data = args.sums.read_bytes()
    signature = key.sign(data)
    out = args.output or args.sums.with_suffix(args.sums.suffix + ".sig")
    out.write_bytes(signature)
    print(out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
