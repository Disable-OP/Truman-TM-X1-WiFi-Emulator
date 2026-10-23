#!/usr/bin/env python3
"""Word-aligned copy-source hypothesis (Milestone L experiment 3).

A burst/DMA copy engine fetches source words aligned to its bus width.
If the engine's match-copy rounds the source address DOWN to a word (or
halfword) boundary instead of byte-exact, every match copies from
(q - d) & ~3 — a per-match variable shift. This produces exactly the
observed "locally correctable with mutually inconsistent deltas"
signature (§14.2/§14.4), and was NOT covered by the refuted families
(all of which used byte-exact sources or fixed table remaps).

Variants: align down/up, halfword, word-granular length (extra bytes),
and the same with the copy clamped to the true length.

Usage: python3 tools/fw_align_copy.py <flash.bin>
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import LBASE, DBASE
from fw_ring_sweep import Validator, load_events, rfc_decode, byte_stats


def decode(events, src_mode="down", align=4, len_mode="exact"):
    """src_mode: down|up|exact; align: 4|2; len_mode: exact|word|half"""
    out = bytearray()
    for e in events:
        k = e["kind"]
        if k == "LIT":
            out.append(e["sym"])
        elif k == "MATCH":
            ln = LBASE[e["sym"] - 257] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            q = len(out)
            s = q - d
            if src_mode == "down":
                s = s & ~(align - 1)
            elif src_mode == "up":
                s = (s + align - 1) & ~(align - 1)
            if len_mode == "word":
                ln2 = (ln + 3) & ~3
            elif len_mode == "half":
                ln2 = (ln + 1) & ~1
            else:
                ln2 = ln
            for i in range(ln2):
                p = s + i
                b = out[p] if 0 <= p < len(out) else 0
                out.append(b)
        elif k.startswith("ERR") or k == "EOF":
            break
    return bytes(out)


def main():
    flash = open(sys.argv[1], "rb").read()
    val = Validator(flash)
    events = load_events(flash)
    ref = rfc_decode(events, 0x8000)
    s, _ = val.strict(ref)
    z, ent = byte_stats(ref)
    print(f"RFC baseline           : len={len(ref):6d} strict={s * 100:5.1f}% "
          f"zeros={z * 100:4.1f}% entropy={ent:.2f}")

    variants = [
        ("src down  word, exact len", dict(src_mode="down", align=4, len_mode="exact")),
        ("src up    word, exact len", dict(src_mode="up", align=4, len_mode="exact")),
        ("src down  half, exact len", dict(src_mode="down", align=2, len_mode="exact")),
        ("src exact      , word  len", dict(src_mode="exact", align=4, len_mode="word")),
        ("src down  word, word  len", dict(src_mode="down", align=4, len_mode="word")),
        ("src exact      , half  len", dict(src_mode="exact", align=4, len_mode="half")),
    ]
    for name, kw in variants:
        out = decode(events, **kw)
        s, _ = val.strict(out)
        z, ent = byte_stats(out)
        print(f"{name:22s}: len={len(out):6d} strict={s * 100:5.1f}% "
              f"zeros={z * 100:4.1f}% entropy={ent:.2f}")

    # first divergence analysis for the most plausible variant (down/exact)
    out = decode(events, src_mode="down", align=4, len_mode="exact")
    n = min(len(out), len(ref))
    div = next((i for i in range(n) if out[i] != ref[i]), None)
    print(f"\nsrc-down/exact-len: first divergence vs RFC at {div}")
    if div is not None:
        print(f"  ref[{div - 8}:{div + 8}]: {ref[div - 8:div + 8].hex(' ')}")
        print(f"  new[{div - 8}:{div + 8}]: {out[div - 8:div + 8].hex(' ')}")


if __name__ == "__main__":
    main()
