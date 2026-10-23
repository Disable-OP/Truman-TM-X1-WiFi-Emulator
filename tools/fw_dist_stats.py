#!/usr/bin/env python3
"""Distance/length statistics of the eCos SP-container symbol stream.

First step of the Milestone L plan (docs/firmware-analysis.md §14.4):
if the compressor only emitted distances <= 1 KiB, the engine's 0x400
chunk register could be the true LZ window size and the corruption
pattern must follow from a small ring instead of a 32 KiB window.

Usage: python3 tools/fw_dist_stats.py <flash.bin>
"""
import struct
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import (BitReader, Huffman, LBASE, LEXTRA, DBASE, DEXTRA,
                          parse_header, symbol_walk)


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[0x1002C:0x1002C + 0x868C]
    assert cont[:2] == b"SP", cont[:2]
    nchunks = struct.unpack_from(">H", cont, 2)[0]
    slen = struct.unpack_from(">I", cont, 4)[0]
    stream = cont[8:]
    bfinal, litlens, distlens, header_bits = parse_header(stream)
    lith = Huffman(litlens)
    disth = Huffman(distlens)
    events = symbol_walk(stream, 1024, lith, disth)
    matches = [e for e in events if e["kind"] == "MATCH"]
    lits = [e for e in events if e["kind"] == "LIT"]

    dists = [DBASE[e["dsym"]] + e["deb"] for e in matches]
    lens = [LBASE[e["sym"] - 257] + e["eb"] for e in matches]
    out_total = sum(lens) + len(lits)

    print(f"container: chunks={nchunks} stream_len=0x{slen:x} "
          f"literals={len(lits)} matches={len(matches)} out_total={out_total}")
    print(f"dist: min={min(dists)} max={max(dists)}")
    for thr in (256, 512, 1024, 2048, 4096, 8192, 16384, 32768):
        n = sum(1 for d in dists if d > thr)
        print(f"  dist > {thr:5d}: {n:5d} ({100.0 * n / len(dists):.2f}%)")
    print(f"len: min={min(lens)} max={max(lens)} sum={sum(lens)}")

    # histogram of distances in 128-byte buckets
    print("\ndist histogram (128-byte buckets):")
    buckets = {}
    for d in dists:
        b = (d - 1) // 128
        buckets[b] = buckets.get(b, 0) + 1
    for b in sorted(buckets):
        print(f"  [{b * 128:5d},{b * 128 + 128:5d}): {buckets[b]:5d} "
              f"{'#' * min(buckets[b], 60)}")

    # early matches (first 2 KiB of output) — the zero-preset region
    early = [e for e in matches if e["out"] < 2048]
    print(f"\near matches (out < 2048): {len(early)}")
    for e in early[:20]:
        li = e["sym"] - 257
        print(f"  out={e['out']:5d} len={LBASE[li] + e['eb']:3d} "
              f"dist={DBASE[e['dsym']] + e['deb']}")


if __name__ == "__main__":
    main()
