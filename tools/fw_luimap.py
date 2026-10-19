#!/usr/bin/env python3
"""Scan boot code for lui/ori pairs to reconstruct the expected memory map.

A lui GPR, imm16 followed by ori GPR, GPR, imm16 builds a 32-bit constant.
Histogramming those constants across the loader reveals the addresses the
firmware itself dereferences — evidence for the memory map.

Usage: python3 fw_luimap.py <firmware.bin> <limit>
"""
import struct
import sys
from collections import Counter


def main():
    path = sys.argv[1]
    limit = int(sys.argv[2], 0) if len(sys.argv) > 2 else 0x10000
    with open(path, "rb") as f:
        data = f.read()[:limit]

    lui_pairs = Counter()
    lui_only = Counter()
    words = struct.unpack(f"<{len(data) // 4}I", data)

    for i, w in enumerate(words):
        if (w >> 26) == 0x0F:  # lui
            rt = (w >> 16) & 0x1F
            hi = w & 0xFFFF
            # look ahead up to 8 instructions for ori on same reg
            val = None
            for j in range(i + 1, min(i + 9, len(words))):
                w2 = words[j]
                if (w2 >> 26) == 0x0D and ((w2 >> 21) & 0x1F) == rt and ((w2 >> 16) & 0x1F) == rt:
                    val = (hi << 16) | (w2 & 0xFFFF)
                    break
                # if rt is clobbered by another write, stop
                if ((w2 >> 16) & 0x1F) == rt and (w2 >> 26) not in (0x0D,):
                    break
            if val is not None:
                lui_pairs[val] += 1
            else:
                lui_only[hi << 16] += 1

    print("=== lui+ori 32-bit constants (count : value) ===")
    for val, cnt in sorted(lui_pairs.items(), key=lambda kv: -kv[1])[:60]:
        print(f"  {cnt:3d}x  0x{val:08x}")

    print("\n=== lui high halves without matching ori (top 40) ===")
    for val, cnt in sorted(lui_only.items(), key=lambda kv: -kv[1])[:40]:
        print(f"  {cnt:3d}x  0x{val:08x}")


if __name__ == "__main__":
    main()
