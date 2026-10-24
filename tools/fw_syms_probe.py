#!/usr/bin/env python3
"""Probe the litlen alphabet structure of the SP container (Milestone R).

Question (docs/firmware-analysis.md §20): the driver uploads the litlen
symbol table as BYTES (285 entries at engine table-RAM 0x000, §13.6), so a
table entry cannot hold a symbol >= 256. For the canonical reading to work
on a byte table, symbol VALUE must equal canonical RANK for every symbol
the engine distinguishes. Probe: which symbols have zero code length, does
rank(s) == s hold for all literals, and what do the uploaded table bytes
look like around the literal/length boundary?

Usage: python3 tools/fw_syms_probe.py <flash.bin>
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import Huffman, BitReader, parse_header

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
# The driver's start bit (§13.6): 1024 stream bits into the container.
START_BIT = 1024


def main():
    flash = open(sys.argv[1], "rb").read()
    stream = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    # DEFLATE starts after the 8-byte "SP" container header (bit 64).
    bfinal, lens_lit, lens_dist, hdr_bits = parse_header(stream[8:])
    print(f"header bits: {hdr_bits} (bfinal={bfinal})")
    print(f"litlen lengths: {len(lens_lit)} entries, dist: {len(lens_dist)} entries")

    nz = [s for s, l in enumerate(lens_lit) if l]
    zero = [s for s, l in enumerate(lens_lit) if not l]
    print(f"nonzero-length litlen symbols: {len(nz)}; zero-length: {zero}")
    nz_dist = [s for s, l in enumerate(lens_dist) if l]
    zero_dist = [s for s, l in enumerate(lens_dist) if not l]
    print(f"dist: nonzero {len(nz_dist)}, zero {zero_dist}")

    # Canonical rank of each symbol.
    h = Huffman(lens_lit)
    rank = {}
    for l in sorted(h.by_len):
        for k, s in enumerate(h.by_len[l]):
            rank[s] = len(rank)
    ident = all(rank.get(s) == s for s in nz if s <= 255)
    print(f"rank(s) == s for ALL coded literals: {ident}")
    if not ident:
        first_bad = [s for s in nz if s <= 255 and rank[s] != s][:8]
        print(f"  first mismatches: {[(s, rank[s]) for s in first_bad]}")
    ident_all = all(rank.get(s) == s for s in nz)
    print(f"rank(s) == s for ALL coded symbols (incl. lengths): {ident_all}")

    # The uploaded byte table (285 bytes in canonical order).
    table = bytes(s & 0xFF for s in (t[0] for t in sorted(rank.items(), key=lambda kv: kv[1])))
    print(f"table[0..16]  = {list(table[:16])}  (as bytes)")
    print(f"table[250..260] = {list(table[250:260])}")
    # Which canonical positions hold what symbol values:
    for i in range(250, 262):
        s_at = next((s for s, r in rank.items() if r == i), None)
        print(f"  rank {i}: symbol {s_at}")

    # Decode the first symbols to see the head's literal sequence.
    # The engine's symbol start is stream bit 1024 (R3818 = 1088 container
    # bits, minus the 64 header bits; §13.4) — the 16 bits between the
    # 1008-bit header end and 1024 are zero padding.
    lith = Huffman(lens_lit)
    disth = Huffman(lens_dist)
    br = BitReader(stream[8:], START_BIT)
    lits = []
    nsym = 0
    try:
        while nsym < 64:
            s = lith.decode(br)
            nsym += 1
            if s <= 255:
                lits.append(s)
            elif s == 256:
                pass
            else:
                li = s - 257
                if li < 8:
                    pass
                break
    except (ValueError, EOFError) as e:
        print(f"walk stopped: {e}")
    print(f"first {len(lits)} literal bytes: {[hex(x) for x in lits[:24]]}")


if __name__ == "__main__":
    main()
