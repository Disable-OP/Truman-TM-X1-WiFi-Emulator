#!/usr/bin/env python3
"""Beam search over per-match length adjustments (Milestone O).

§14.4 observed that per-match greedy length corrections recover 97.5%
LOCAL validity with mutually inconsistent deltas. This tool searches the
per-match adjustment space systematically: at every match, branch over
delta in {0,+1,+2} extra emitted bytes, keep the top-B partial decodes by
EXACT incremental strict validity (every completed word is checked once;
append-only output makes this O(1) amortized), and at the end report the
winner's delta sequence annotated by (len, dist, out, out%4, row) so any
consistent rule becomes visible.

The PNG row framing (§17) removed the exact-size constraint: the engine
pads the image from the zeroed band buffer, so the beam optimizes
validity alone; the stream is always consumed to input end.

Usage: python3 tools/fw_beam_lz.py <flash.bin> [--beam B] [--deltas 0,1,2]
"""
import sys
from collections import Counter

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import Huffman, LBASE, DBASE, parse_header, symbol_walk
from fw_ring_sweep import Validator

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
WIDTH = 1024


def main():
    flash = open(sys.argv[1], "rb").read()
    beam_n = 16
    deltas = (0, 1, 2)
    for i, a in enumerate(sys.argv[2:]):
        if a == "--beam":
            beam_n = int(sys.argv[3 + i])
        if a == "--deltas":
            deltas = tuple(int(x) for x in sys.argv[3 + i].split(","))

    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)
    events = symbol_walk(stream, 1024, lith, disth)
    val = Validator(flash)
    wv = val.word_valid

    # beam items: [out, extra, valid_words, total_words, log]
    beam = [[bytearray(), 0, 0, 0, []]]

    def feed_literal(item, b):
        item[0].append(b)
        if len(item[0]) % 4 == 0:
            o = item[0]
            item[3] += 1
            if wv(bytes(o[-4:])):
                item[2] += 1

    def feed_match(item, ln, d, log):
        o = item[0]
        n = len(o)
        for j in range(ln):
            o.append(o[n - d + j] if 0 <= n - d + j < n else 0)
            if len(o) % 4 == 0:
                item[3] += 1
                if wv(bytes(o[-4:])):
                    item[2] += 1
        item[1] += ln - log[0]
        item[4].append((log[0], log[1], log[2], log[3], ln))

    n_matches = sum(1 for e in events if e["kind"] == "MATCH")
    done = 0
    for e in events:
        k = e["kind"]
        if k == "LIT":
            for it in beam:
                feed_literal(it, e["sym"])
        elif k == "MATCH":
            ln0 = LBASE[e["sym"] - 257] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            p = e["out"]
            new = []
            for it in beam:
                for dl in deltas:
                    nit = [bytearray(it[0]), it[1], it[2], it[3],
                           list(it[4])]
                    feed_match(nit, ln0 + dl, d, (ln0, d, p, dl))
                    new.append(nit)
            new.sort(key=lambda it: -(it[2] / max(it[3], 1)))
            beam = new[:beam_n]
            done += 1
            if done % 2000 == 0:
                best = beam[0]
                print(f"  match {done}/{n_matches}: score="
                      f"{best[2] / max(best[3], 1) * 100:.1f}% "
                      f"out={len(best[0])} extra={best[1]}", flush=True)
        # EOB/EOF: markers, no output

    best = beam[0]
    o = best[0]
    s, n = val.strict(bytes(o))
    print(f"\nBEST: out={len(o)} extra={best[1]} "
          f"incr_score={best[2] / max(best[3], 1) * 100:.2f}% "
          f"final_strict={s * 100:.2f}% head={bytes(o[:8]).hex(' ')}")

    # delta-pattern analysis: what distinguishes delta>0 matches?
    dl = best[4]
    by = {0: Counter(), 1: Counter(), 2: Counter()}
    for ln0, d, p, delta, ln in dl:
        by[delta][f"len={ln0}"] += 1
        by[delta][f"dist_bucket={min(d // 128, 8)}"] += 1
        by[delta][f"rowmod={p % WIDTH}"] += 1
        by[delta][f"align={p % 4}"] += 1
    for delta in sorted(by):
        c = by[delta]
        tot = sum(v for k2, v in c.items() if k2.startswith("len="))
        print(f"delta={delta}: n={tot}")
        for pref in ("len=", "dist_bucket=", "align="):
            top = sorted(((k2, v) for k2, v in c.items()
                          if k2.startswith(pref)), key=lambda t: -t[1])[:8]
            print(f"   {top}")
    open("/tmp/beam_best.bin", "wb").write(bytes(o))
    with open("/tmp/beam_deltas.txt", "w") as f:
        for ln0, d, p, delta, ln in dl:
            f.write(f"{p}\t{ln0}\t{d}\t{delta}\n")


if __name__ == "__main__":
    main()
