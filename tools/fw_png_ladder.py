#!/usr/bin/env python3
"""PNG-oracle length-ladder search (Milestone N).

With the engine identified as a PNG decoder (PNG_DEC_* regs), the inflate
output is 78 rows of [filter byte + 1024 data bytes] = 79,950 bytes; the
pixel output (78 x 1024 = 79,872) is the UNFILTERED image. The emulator's
RFC walk under-decodes (75,398), so the engine's length ladder differs
from RFC's by +4,552 bytes over the match population.

This tool re-runs the group-constant length-base search for the NEW
target and scores every exact-size candidate through the PNG oracle:
  1. decode (custom bases, RFC extra widths)
  2. bytes at 1025-stride row starts must all be in {0..4}  (filters)
  3. strip filter bytes, PNG-unfilter, strict MIPS validity

Usage: python3 tools/fw_png_ladder.py <flash.bin>
"""
import itertools
import sys
from collections import Counter

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import Huffman, LBASE, LEXTRA, DBASE, parse_header, symbol_walk
from fw_ring_sweep import Validator
from fw_png_rows import unfilter

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
WIDTH = 1024
HEIGHT = 78
STRIDE = 1025
TARGET = HEIGHT * STRIDE


def decode_with(events, lb):
    out = bytearray()
    for e in events:
        k = e["kind"]
        if k == "LIT":
            out.append(e["sym"])
        elif k == "MATCH":
            li = e["sym"] - 257
            ln = lb[li] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            q = len(out)
            if q - d + ln > TARGET + 64:
                return None
            for i in range(ln):
                p = q - d + i
                out.append(out[p] if 0 <= p < q else 0)
        elif k.startswith("ERR") or k == "EOF":
            break
    return bytes(out)


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)
    events = symbol_walk(stream, 1024, lith, disth)
    matches = [e for e in events if e["kind"] == "MATCH"]
    lits = [e for e in events if e["kind"] == "LIT"]
    cnt = Counter(e["sym"] - 257 for e in matches)
    group = {li: min(LEXTRA[li], 5) for li in range(29)}
    group[28] = 5

    target_match_bytes = TARGET - len(lits) - sum(e["eb"] for e in matches)
    base_contrib = sum(cnt[li] * LBASE[li] for li in cnt)
    need = target_match_bytes - base_contrib
    gcount = [sum(c for li, c in cnt.items() if group[li] == g)
              for g in range(6)]
    print(f"lits={len(lits)} target_match_bytes={target_match_bytes} "
          f"RFC base_contrib={base_contrib} need=+{need}")
    print(f"group counts: {gcount}")

    sols = []
    for g in itertools.product(range(0, 16), repeat=6):
        if sum(gc * gg for gc, gg in zip(gcount, g)) == need:
            sols.append(g)
    print(f"exact-size solutions: {len(sols)}")

    val = Validator(flash)
    passed = []
    for g in sols:
        lb = [LBASE[li] + g[group[li]] for li in range(29)]
        out = decode_with(events, lb)
        if out is None or len(out) != TARGET:
            continue
        fbs = [out[r * STRIDE] for r in range(HEIGHT)]
        if any(f > 4 for f in fbs):
            continue
        img = unfilter(out, WIDTH, HEIGHT)
        s, n = val.strict(img)
        passed.append((s, g, fbs))
        print(f"  *** FILTER-OK g={g} filters={fbs[:10]}... "
              f"strict={s * 100:.1f}%")
    if not passed:
        print("no candidate passed the filter-byte oracle")


if __name__ == "__main__":
    main()
