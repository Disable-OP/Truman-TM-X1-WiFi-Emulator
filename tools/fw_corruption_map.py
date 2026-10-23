#!/usr/bin/env python3
"""Structure of the decode corruption (Milestone L experiment 2).

Maps strict MIPS validity per 1 KiB chunk and per 128-byte burst of the
RFC 1951 decode of the eCos SP container, and separately scores the
certain literal bytes, to discriminate the remaining hypothesis classes
(docs/firmware-analysis.md §14.4):
  * chunk/burst-periodic validity  -> write-side block transform
  * uniform ~46% validity          -> per-match value semantics
  * validity decaying with offset  -> accumulating window/drift defect

Usage: python3 tools/fw_corruption_map.py <flash.bin>
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import LBASE, DBASE
from fw_ring_sweep import Validator, load_events, rfc_decode


def main():
    flash = open(sys.argv[1], "rb").read()
    val = Validator(flash)
    events = load_events(flash)
    ref = rfc_decode(events, 0x8000)
    n = len(ref)
    print(f"RFC decode: {n} bytes")

    # ownership map: which event produced each output byte
    owner = []
    for e in events:
        k = e["kind"]
        if k == "LIT":
            owner.append(("L", e["sym"]))
        elif k == "MATCH":
            ln = LBASE[e["sym"] - 257] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            owner.extend([("M", d)] * ln)

    # 1. per-1KiB-chunk strict validity
    print("\nper-1KiB-chunk strict validity:")
    vals = []
    for c in range(0, n // 1024 * 1024, 1024):
        s, _ = val.strict(ref, c, 1024)
        vals.append(s)
        bar = "#" * int(s * 40)
        print(f"  chunk {c // 1024:3d}: {s * 100:5.1f}% {bar}")

    # 2. per-128-byte-burst validity (first 8 KiB, finer)
    print("\nper-128B-burst validity (first 8 KiB):")
    row = []
    for c in range(0, 8192, 128):
        s, _ = val.strict(ref, c, 128)
        row.append(f"{s * 100:3.0f}")
    print("  " + " ".join(row))

    # 3. validity restricted to positions owned by literals vs matches
    lit_pos = [i for i, (k, _) in enumerate(owner) if k == "L"]
    print(f"\nliteral bytes: {len(lit_pos)} ({100 * len(lit_pos) / n:.1f}%)")

    # 4. match-distance class vs position: do high-distance matches dominate
    # late chunks?
    print("\nmatch dist class share per 8 KiB region:")
    for r in range(0, n // 8192):
        seg = owner[r * 8192:(r + 1) * 8192]
        small = sum(1 for k, d in seg if k == "M" and d <= 1024)
        big = sum(1 for k, d in seg if k == "M" and d > 1024)
        lit = sum(1 for k, _ in seg if k == "L")
        tot = len(seg)
        print(f"  region {r:2d} @{r * 8192:6x}: lit={100 * lit / tot:4.1f}% "
              f"match<=1K={100 * small / tot:4.1f}% match>1K={100 * big / tot:4.1f}%")

    # 5. sliding-window validity profile (4 KiB window, 1 KiB step)
    print("\nsliding 4 KiB validity profile (step 4 KiB):")
    prof = []
    for c in range(0, n - 4096, 4096):
        s, _ = val.strict(ref, c, 4096)
        prof.append(s)
    for i in range(0, len(prof), 8):
        print("  " + " ".join(f"{p * 100:4.0f}" for p in prof[i:i + 8]))


if __name__ == "__main__":
    main()
