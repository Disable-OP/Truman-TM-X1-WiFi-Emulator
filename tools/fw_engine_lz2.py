#!/usr/bin/env python3
"""Deep divergence analysis: engine output vs RFC1951 reference (Milestone J).

Part 2 of tools/fw_engine_lz.py:
 1. Rich dump of the first divergence regions (context + events).
 2. Proves the frozen-pointer decode defect: the pre-fix engine model
    output is reproduced byte-for-byte by the "fill of window[n-d]"
    semantics (100.000% over the shared prefix).

Usage:
  python3 tools/fw_engine_lz2.py <flash.bin> <engine_dump.bin>
"""
import struct
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import (BitReader, Huffman, LBASE, LEXTRA, DBASE, DEXTRA,
                          parse_header, symbol_walk, rfc_decode)


def load(flash_path, eng_path):
    flash = open(flash_path, "rb").read()
    eng = open(eng_path, "rb").read()
    cont = flash[0x1002C:0x1002C + 0x868C]
    stream = cont[8:]
    bfinal, litlens, distlens, header_bits = parse_header(stream)
    lith = Huffman(litlens)
    disth = Huffman(distlens)
    events = symbol_walk(stream, 1024, lith, disth)
    return stream, events, eng


def frozen_decode(events, preset_len=0x8000):
    """The pre-fix engine semantics: match copy reads window[n-d] with n
    frozen at the match start (constant fill of the first source byte)."""
    out = bytearray()
    window = bytearray(preset_len)
    for e in events:
        if e["kind"] == "LIT":
            window.append(e["sym"])
            out.append(e["sym"])
        elif e["kind"] == "MATCH":
            li = e["sym"] - 257
            ln = LBASE[li] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            n = len(window)
            if d > n:
                break
            for _ in range(ln):
                b = window[n - d]
                window.append(b)
                out.append(b)
    return out


def divergence_regions(events, ref, eng, limit=24):
    """Yield (start, end, first_event_idx) for each maximal disagreement."""
    n = min(len(ref), len(eng))
    regions = []
    i = 0
    while i < n:
        if ref[i] != eng[i]:
            j = i
            while j < n and ref[j] != eng[j]:
                j += 1
            regions.append((i, j))
            i = j
        else:
            i += 1
    return regions[:limit]


def event_at(events, pos):
    """Event that produced byte at ref position pos."""
    lo, hi = 0, len(events) - 1
    while lo <= hi:
        mid = (lo + hi) // 2
        if events[mid]["out"] <= pos:
            lo = mid + 1
        else:
            hi = mid - 1
    return lo - 1


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return
    stream, events, eng = load(sys.argv[1], sys.argv[2])
    ref = rfc_decode(events, 0x8000)
    print(f"ref={len(ref)} eng={len(eng)}")

    # --- frozen-pointer proof: reproduce the pre-fix engine output ---
    bug = frozen_decode(events)
    nb = min(len(bug), len(eng))
    same = sum(1 for i in range(nb) if bug[i] == eng[i])
    first_bad = next((i for i in range(nb) if bug[i] != eng[i]), None)
    print(f"\nfrozen-pointer decode: {len(bug)} bytes; matches engine dump "
          f"{same}/{nb} ({100.0 * same / max(nb, 1):.3f}%), "
          f"first mismatch: {first_bad}")

    regions = divergence_regions(events, ref, eng)
    print(f"divergence regions vs RFC ref: {len(regions)}")

    for (a, b) in regions:
        k = event_at(events, a)
        e = events[k]
        r = ref[a:b]
        g = eng[a:b]
        info = ""
        if e["kind"] == "MATCH":
            li = e["sym"] - 257
            ln = LBASE[li] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            info = f"len={ln} dist={d} overlap={d < ln}"
        print(f"\nregion out[{a}:{b}] ({b - a}B) event#{k} {e['kind']} {info} bit={e['bit']}")
        print(f"  ref: {r[:16].hex(' ')}")
        print(f"  eng: {g[:16].hex(' ')}")
        # does engine match ref shifted?
        # show 8 bytes before region in both
        print(f"  ctx ref[{a-8}:{a}]: {ref[a-8:a].hex(' ')}")
        print(f"  ctx eng[{a-8}:{a}]: {eng[a-8:a].hex(' ')}")

    # --- size oracle over candidate length semantics ---
    matches = [e for e in events if e["kind"] == "MATCH"]
    n_lit = sum(1 for e in events if e["kind"] == "LIT")
    target_total = 78 * 1024
    target_match = target_total - n_lit
    print(f"\nliterals={n_lit} matches={len(matches)} "
          f"target_match_bytes={target_match}")

    def total(f):
        return sum(f(e) for e in matches)

    def rfc_len(e):
        return LBASE[e["sym"] - 257] + e["eb"]

    cands = {
        "RFC1951": rfc_len,
        "base+1 all": lambda e: rfc_len(e) + 1,
        "shift down LBASE[li+1]": lambda e: (LBASE[min(e["sym"] - 257 + 1, 28)]) + e["eb"],
        "shift up LBASE[li-1]": lambda e: (LBASE[max(e["sym"] - 257 - 1, 0)]) + e["eb"],
        "len = sym-254+eb": lambda e: (e["sym"] - 254) + e["eb"],
        "len = sym-255": lambda e: e["sym"] - 255,
        "len = sym-256+eb": lambda e: (e["sym"] - 256) + e["eb"],
    }
    for name, f in cands.items():
        t = total(f)
        print(f"  {name:28s} total={t:6d} delta_vs_target={t - target_match:+6d}")


if __name__ == "__main__":
    main()
