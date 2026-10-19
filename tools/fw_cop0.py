#!/usr/bin/env python3
"""Enumerate COP0 accesses (mfc0/mtc0) in the boot code.

Usage: python3 fw_cop0.py <firmware.bin> <limit>
"""
import struct
import sys
from collections import Counter

COP0_NAMES = {
    0: "Index", 1: "MPLegacy/Random", 2: "EntryLo0", 3: "EntryLo1", 4: "Context",
    5: "PageMask", 6: "Wired", 7: "HWrena", 8: "BadVAddr", 9: "Count", 10: "EntryHi",
    11: "Compare", 12: "Status", 13: "Cause", 14: "EPC", 15: "PRId/EBase", 16: "Config",
    17: "LLAddr", 18: "WatchLo", 19: "WatchHi", 20: "XContext", 21: "*", 22: "Debug/DEVID?",
    23: "DEPC/DEVID?", 24: "PerfCnt0", 25: "PerfCnt1", 26: "ErrCtl", 27: "CacheErr",
    28: "TagLo", 29: "TagHi", 30: "ErrorEPC", 31: "DESAVE",
}


def main():
    path = sys.argv[1]
    limit = int(sys.argv[2], 0) if len(sys.argv) > 2 else 0x10000
    with open(path, "rb") as f:
        data = f.read()[:limit]

    words = struct.unpack(f"<{len(data) // 4}I", data)
    reads = Counter()
    writes = Counter()

    for w in words:
        if (w >> 26) == 0x10:  # COP0
            rs = (w >> 21) & 0x1F
            rd = (w >> 11) & 0x1F
            sel = w & 0x7
            if rs == 0x00:  # mfc0
                reads[(rd, sel)] += 1
            elif rs == 0x04:  # mtc0
                writes[(rd, sel)] += 1

    print("=== mfc0 (reads) rd/sel : count ===")
    for (rd, sel), c in sorted(reads.items(), key=lambda kv: -kv[1]):
        print(f"  {c:3d}x  rd={rd:2d} sel={sel}  ({COP0_NAMES.get(rd, '?')})")
    print("=== mtc0 (writes) rd/sel : count ===")
    for (rd, sel), c in sorted(writes.items(), key=lambda kv: -kv[1]):
        print(f"  {c:3d}x  rd={rd:2d} sel={sel}  ({COP0_NAMES.get(rd, '?')})")


if __name__ == "__main__":
    main()
