#!/usr/bin/env python3
"""Capstone cross-check for the Milestone P length-ladder search (§18.2).

The Rust search (examples/ladder_search.rs) optimizes against the crate's
own MIPS decoder oracle. This tool re-scores the reported finalists with
the PUBLISHED capstone oracle (§14.2 Validator) so the conclusions in
docs/firmware-analysis.md stand on the same oracle as Milestones N/O:

  1. RFC ladder under m2 accounting (walk to the image cap) — the
     Milestone O baseline behavior;
  2. the best in-band ladder found by the Rust search;
  3. its monotone projection.

Also verifies the size anchor independently: the full-parse decode length
(no image cap) per ladder, computed with the tools' own walk.

Usage: python3 tools/fw_ladder_verify.py <flash.bin> [ladder.json]
"""
import json
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
import fw_fixed_ladder as F
from fw_engine_lz import Huffman, parse_header
from fw_ring_sweep import Validator

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
IMG = 79_872

# examples/ladder_search.rs winner + monotone projection (this milestone)
RUST_WINNER = [1, 1, 4, 6, 7, 7, 9, 10, 11, 1, 11, 17, 19, 23, 27, 31, 35,
               43, 51, 59, 67, 83, 105, 115, 131, 163, 194, 227, 256]
RUST_MONO = [1, 1, 4, 6, 7, 7, 9, 10, 11, 11, 11, 17, 19, 23, 27, 31, 35,
             43, 51, 59, 67, 83, 105, 115, 131, 163, 194, 227, 256]


def walk_uncapped(stream, start_bit, lith, disth, ladder):
    """m2 walk with NO image cap: full-parse decode length S(ladder)."""
    orig = F.IMG
    try:
        F.IMG = 1 << 30  # neutralize the cap inside walk's stop checks
        out, n, why = F.walk(stream, start_bit, lith, disth, True, False,
                             1 << 30)
    finally:
        F.IMG = orig
    return out, n, why


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(disth_len := distlens)
    val = Validator(flash)

    if len(sys.argv) > 2:
        extra = json.load(open(sys.argv[2]))
    else:
        extra = {}

    ladders = [
        ("RFC m2 (cap walk, Milestone O baseline)", None),
        ("search winner (in-band)", RUST_WINNER),
        ("winner monotone projection", RUST_MONO),
    ] + [(f"extra: {k}", v) for k, v in extra.items()]

    for name, ladder in ladders:
        if ladder is None:
            out, n, why = F.walk(stream, 1024, lith, disth, True, False,
                                 IMG + 64)
            full = None
        else:
            F.LBASE = list(ladder)
            out, n, why = F.walk(stream, 1024, lith, disth, True, False,
                                 IMG + 64)
            full_out, _, _ = walk_uncapped(stream, 1024, lith, disth, ladder)
            full = len(full_out)
            F.LBASE = None
        s, nw = val.strict(out[:IMG])
        print(f"{name:38s} out={len(out):6d} syms={n:6d} stop={why:11s} "
              f"S_full={full if full is not None else '-':>7} "
              f"strict={s * 100:5.1f}% head={out[:8].hex(' ')}")


if __name__ == "__main__":
    main()
