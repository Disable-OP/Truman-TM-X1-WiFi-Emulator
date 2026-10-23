#!/usr/bin/env python3
"""Fixed-filter pixel-domain LZ probe (Milestone O).

Hypothesis (§17.4): IMG_TYPE 0x68 encodes a FIXED PNG filter. The engine
  - decodes a literal byte b and unfilters it against the PIXEL domain:
        pixel = (b + predictor(pixel-left, pixel-above, ...)) & 0xFF
  - executes matches as copies from the PIXEL buffer (already-unfiltered
    data) at the coded distance — no filtering inside copies.
The compressor then emitted distances over unfiltered pixels; a plain RFC
replay (copies in the literal domain) produces exactly the observed
"plausible-but-wrong" near-miss content, while row 0 literals (zero
filter history — the driver zero-fills the band) stay certain.

Filters tested: None / Sub / Up / Average / Paeth, bpp=1, row width 1024
(PNG_IMG_WIDTH). Scoring: calibrated strict MIPS validity + the certain
head (bytes 0..1 = 60 81) + zero-ratio/entropy stats.

Usage: python3 tools/fw_pixel_lz.py <flash.bin>
"""
import math
import sys
from collections import Counter

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import Huffman, LBASE, DBASE, parse_header, symbol_walk
from fw_ring_sweep import Validator

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
WIDTH = 1024


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def decode(events, filt):
    """Pixel-domain decode with fixed filter `filt` (0..4)."""
    out = bytearray()          # pixels (linear)
    for e in events:
        k = e["kind"]
        if k == "LIT":
            i = len(out)
            x = e["sym"]
            if filt == 1:      # Sub
                a = out[i - 1] if i % WIDTH else 0
                x = (x + a) & 0xFF
            elif filt == 2:    # Up
                b = out[i - WIDTH] if i >= WIDTH else 0
                x = (x + b) & 0xFF
            elif filt == 3:    # Average
                a = out[i - 1] if i % WIDTH else 0
                b = out[i - WIDTH] if i >= WIDTH else 0
                x = (x + (a + b) // 2) & 0xFF
            elif filt == 4:    # Paeth
                a = out[i - 1] if i % WIDTH else 0
                b = out[i - WIDTH] if i >= WIDTH else 0
                c = out[i - WIDTH - 1] if (i >= WIDTH and i % WIDTH) else 0
                x = (x + paeth(a, b, c)) & 0xFF
            out.append(x)
        elif k == "MATCH":
            ln = LBASE[e["sym"] - 257] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            n = len(out)
            for j in range(ln):
                p = n - d + j
                out.append(out[p] if 0 <= p < n else 0)
        elif k.startswith("ERR") or k == "EOF":
            break
    return bytes(out)


def stats(buf):
    z = buf.count(0) / max(len(buf), 1)
    freq = Counter(buf)
    ent = -sum((c / len(buf)) * math.log2(c / len(buf))
               for c in freq.values())
    return z, ent


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)
    events = symbol_walk(stream, 1024, lith, disth)
    val = Validator(flash)
    names = {0: "None(raw)", 1: "Sub", 2: "Up", 3: "Average", 4: "Paeth"}
    for filt in range(5):
        out = decode(events, filt)
        s, n = val.strict(out)
        z, ent = stats(out)
        print(f"filter {names[filt]:9s}: out={len(out):6d} strict={s * 100:5.1f}% "
              f"zeros={z * 100:4.1f}% ent={ent:.2f} "
              f"head={out[:8].hex(' ')}")


if __name__ == "__main__":
    main()
