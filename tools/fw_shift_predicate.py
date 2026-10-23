#!/usr/bin/env python3
"""Length-accounting predicate search for the +4,474 output delta (§16.3).

The programmed output is 78x1024 = 79,872 bytes; the RFC decode of the
verified symbol stream is 75,398 (delta 4,474). Every prior length search
(§15.6 item 5) varied per-extra-bit-group base offsets WITH RFC copy
accounting — i.e. positions of later tokens never moved. But if the
engine's matches emit more bytes than RFC accounting, all subsequent
positions shift; the delta then constrains a PREDICATE on (len, dist, out)
selecting exactly the matches that emit extra bytes:
  count(predicate) * adjustment == 4,474  (4,474 = 2 x 2,237, 2,237 prime)

This tool enumerates single/double predicates (len thresholds, dist
thresholds, conjunctions, singletons, modulo classes, output-chunk
thresholds) and reports exact-count hits. Current hits (adjustment +2):
  dist < 96 | dist >= 3485 | len >= 5 & dist < 3073  (2,237 matches each)
All three are arbitrary hardware-wise; the search continues to favor
combined length+distance semantics rather than any uniform table remap.

Usage: python3 tools/fw_shift_predicate.py <flash.bin>
"""
import bisect
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import Huffman, LBASE, DBASE, parse_header, symbol_walk

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
TARGET_TOTAL = 79_872


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    assert cont[:2] == b"SP"
    stream = cont[8:]
    bfinal, litlens, distlens, hb = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)
    events = symbol_walk(stream, 1024, lith, disth)
    matches = []
    for e in events:
        if e["kind"] == "MATCH":
            ln = LBASE[e["sym"] - 257] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            matches.append((ln, d, e["out"]))
    n_lit = sum(1 for e in events if e["kind"] == "LIT")
    rfc = n_lit + sum(m[0] for m in matches)
    delta = TARGET_TOTAL - rfc
    print(f"matches={len(matches)} literals={n_lit} rfc={rfc} delta={delta}")
    # divisor analysis
    divs = [k for k in range(1, delta + 1) if delta % k == 0]
    print(f"exact-count targets (delta=adjustment*count): {divs}")
    targets = set(divs)
    hits = []

    def add(name, c):
        if c in targets:
            hits.append((name, c))

    lens = sorted({m[0] for m in matches})
    ds_sorted = sorted(m[1] for m in matches)
    for a in lens:
        add(f"len>={a}", sum(1 for m in matches if m[0] >= a))
        add(f"len<={a}", sum(1 for m in matches if m[0] <= a))
        add(f"len=={a}", sum(1 for m in matches if m[0] == a))
    for b in sorted(set(ds_sorted)):
        i = bisect.bisect_left(ds_sorted, b)
        add(f"dist>={b}", len(ds_sorted) - i)
        add(f"dist<{b}", i)
    dth = [1, 2, 3, 4, 5, 8, 9, 13, 16, 17, 25, 32, 33, 49, 64, 65, 97, 128,
           129, 193, 256, 257, 385, 512, 513, 769, 1024, 1025, 1537, 2048,
           2049, 3073, 4096, 4097, 6145, 8192, 8193, 12289, 16384, 16385,
           24577, 24578, 32768]
    for a in lens:
        for b in dth:
            add(f"len>={a}&dist>={b}",
                sum(1 for m in matches if m[0] >= a and m[1] >= b))
            add(f"len>={a}&dist<{b}",
                sum(1 for m in matches if m[0] >= a and m[1] < b))
    for mod in (2, 4, 8):
        for r in range(mod):
            add(f"len%{mod}=={r}", sum(1 for m in matches if m[0] % mod == r))
            add(f"dist%{mod}=={r}", sum(1 for m in matches if m[1] % mod == r))
    for nb in range(0, 79):
        add(f"out>={nb}*1024", sum(1 for m in matches if m[2] >= nb * 1024))
    for name, c in sorted(set(hits)):
        print(f"  EXACT: {name} -> {c} matches "
              f"(adjustment {delta}/{c} = {delta / c:g})")


if __name__ == "__main__":
    main()
