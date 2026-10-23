#!/usr/bin/env python3
"""Write-transform key detection (Milestone L experiment 5).

If the engine applies a position-dependent XOR/ADD transform on its
output write path (or the SDK tool pre-transformed the plaintext), then
truth(x) = decode(x) OP key(x). At decode positions where the byte is
ZERO, truth(x) = key(x) — so the value distribution at decode-zero
positions IS the key (up to the zero-ratio of truth).

This tool:
 1. histograms byte values at decode-zero positions (per offset class
    mod P for P = 16..32768) and reports the max-class concentration
    vs the uniform expectation;
 2. directly scores strict validity of decode ^ / + key(x) for
    position-derived key families.

Usage: python3 tools/fw_keydetect.py <flash.bin>
"""
import sys
from collections import Counter

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_ring_sweep import Validator, load_events, rfc_decode, byte_stats


def main():
    flash = open(sys.argv[1], "rb").read()
    val = Validator(flash)
    events = load_events(flash)
    ref = rfc_decode(events, 0x8000)
    n = len(ref)
    zpos = [i for i, b in enumerate(ref) if b == 0]
    print(f"decode: {n} bytes, zero positions: {len(zpos)} "
          f"({100 * len(zpos) / n:.1f}%)")

    # 1. concentration of values at zero positions, by offset class
    print("\nvalue concentration at decode-zero positions:")
    for P in (16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192):
        best = 0
        bestinfo = None
        for m in range(P):
            cnt = Counter(ref[i] for i in zpos if i % P == m)
            if not cnt:
                continue
            v, c = cnt.most_common(1)[0]
            tot = sum(cnt.values())
            if c / tot > best:
                best = c / tot
                bestinfo = (m, v, c, tot)
        print(f"  P={P:5d}: best class {bestinfo[0]:4d} -> value 0x{bestinfo[1]:02x} "
              f"concentration {best * 100:.1f}% of {bestinfo[3]}")

    # overall value histogram at zero positions (any offset)
    cnt = Counter(ref[i] for i in zpos)
    top = cnt.most_common(8)
    tot = sum(cnt.values())
    print("  overall top values at zero positions:",
          ", ".join(f"0x{v:02x}:{100 * c / tot:.1f}%" for v, c in top))

    # 2. direct transform scoring
    print("\ntransform scoring (strict validity of transformed decode):")
    base, _ = val.strict(ref)
    print(f"  (baseline {base * 100:.1f}%)")
    best = (base, "identity")
    for P in (16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 32768):
        for op in ("xor", "add", "sub"):
            buf = bytearray(ref)
            for i in range(n):
                k = (i % P) & 0xFF
                if op == "xor":
                    buf[i] ^= k
                elif op == "add":
                    buf[i] = (buf[i] + k) & 0xFF
                else:
                    buf[i] = (buf[i] - k) & 0xFF
            s, _ = val.strict(bytes(buf))
            tag = f"{op} (x mod {P})"
            if s > best[0]:
                best = (s, tag)
            if s > base + 0.02:
                print(f"  {tag:20s}: {s * 100:5.1f}%  <<<<")
    print(f"  best: {best[1]} {best[0] * 100:.1f}%")


if __name__ == "__main__":
    main()
