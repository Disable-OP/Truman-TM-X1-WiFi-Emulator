#!/usr/bin/env python3
"""Length-base-table search (Milestone L experiment 6).

Reading (a) of §14.1: the engine decodes exactly 79,872 bytes (the
container's own chunk claim 78 x 1024, corroborated by the accounting
624 x 128). RFC lengths give 75,398 — 4,474 short. The extra-bit COUNTS
are pinned by the exact bit-stream parse, so only the LENGTH BASE TABLE
can differ. A vendor engine in a closed ecosystem may legitimately use
a non-RFC base table.

Search space: LBASE'[li] = LBASE[li] + g_{extra_group(li)}, with the
size equation  sum(count[li] * LBASE'[li]) = 62,639  exactly, g in 0..12.
Candidates are ranked by cheap byte statistics, and the top ones are
scored with the calibrated strict-validity oracle.

Usage: python3 tools/fw_length_search.py <flash.bin> [--top N]
"""
import itertools
import math
import sys
from collections import Counter

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import LBASE, LEXTRA, DBASE
from fw_ring_sweep import Validator, load_events

TARGET_TOTAL = 78 * 1024


def decode_with(events, lb):
    """Decode with custom length base table lb (list of 29 bases)."""
    out = bytearray()
    n_full = 0
    for e in events:
        k = e["kind"]
        if k == "LIT":
            out.append(e["sym"])
        elif k == "MATCH":
            li = e["sym"] - 257
            ln = lb[li] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            q = len(out)
            for i in range(ln):
                p = q - d + i
                out.append(out[p] if 0 <= p < q else 0)
        elif k.startswith("ERR") or k == "EOF":
            break
    return bytes(out)


def byte_stats(buf):
    if not buf:
        return 0.0, 0.0, 0.0
    zeros = buf.count(0) / len(buf)
    freq = [0] * 256
    for b in buf:
        freq[b] += 1
    ent = -sum((c / len(buf)) * math.log2(c / len(buf)) for c in freq if c)
    # mean zero-run length over runs >= 4 (code-like structure proxy)
    runs = []
    cur = 0
    for b in buf:
        if b == 0:
            cur += 1
        else:
            if cur >= 4:
                runs.append(cur)
            cur = 0
    mrun = (sum(runs) / len(runs)) if runs else 0.0
    return zeros, ent, mrun


def main():
    flash = open(sys.argv[1], "rb").read()
    events = load_events(flash)
    matches = [e for e in events if e["kind"] == "MATCH"]
    lits = [e for e in events if e["kind"] == "LIT"]
    cnt = Counter(e["sym"] - 257 for e in matches)
    # extra-bit group per length symbol (cap LEXTRA at 5; sym 285 has 0)
    group = {li: min(LEXTRA[li], 5) for li in range(29)}
    group[28] = 5  # fold the 258 symbol into the 5-extra group

    # target: sum(count[li] * (LBASE[li] + g_g)) = 62639
    target = TARGET_TOTAL - len(lits) - sum(e["eb"] for e in matches)
    base_contrib = sum(cnt[li] * LBASE[li] for li in cnt)
    need = target - base_contrib
    gcount = [sum(c for li, c in cnt.items() if group[li] == g) for g in range(6)]
    print(f"target base-table contribution: {target} (need +{need} over RFC "
          f"{base_contrib})")
    print(f"group counts: {gcount}")

    sols = []
    for g in itertools.product(range(0, 13), repeat=6):
        s = sum(gc * gg for gc, gg in zip(gcount, g))
        if s == need:
            sols.append(g)
    print(f"exact-size solutions: {len(sols)}")
    if not sols:
        return

    # rank by cheap stats
    scored = []
    for g in sols:
        lb = [LBASE[li] + g[group[li]] for li in range(29)]
        out = decode_with(events, lb)
        z, ent, mrun = byte_stats(out)
        # plausibility: real code zeros 40-57%, entropy <= 5.15, long runs
        score = (max(0, 1 - abs(z - 0.48) / 0.25) +
                 max(0, 1 - abs(ent - 4.8) / 1.5) +
                 min(mrun / 40.0, 1.0))
        scored.append((score, g, len(out), z, ent, mrun, out))
    scored.sort(key=lambda t: -t[0])

    val = Validator(flash)
    print("\ntop candidates by byte statistics:")
    for score, g, ln, z, ent, mrun, out in scored[:12]:
        print(f"  g={g} len={ln} zeros={z * 100:.1f}% ent={ent:.2f} "
              f"mrun={mrun:.1f} score={score:.3f}")

    print("\nstrict validity of the top 8:")
    results = []
    for score, g, ln, z, ent, mrun, out in scored[:8]:
        s, _ = val.strict(out)
        results.append((s, g, ln))
        print(f"  g={g} len={ln}: strict={s * 100:.1f}%")
    best = max(results)
    print(f"\nbest: g={best[1]} len={best[2]} strict={best[0] * 100:.1f}% "
          f"(RFC baseline 48.3%)")


if __name__ == "__main__":
    main()
