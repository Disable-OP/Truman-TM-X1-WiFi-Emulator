#!/usr/bin/env python3
"""Uniform-width extra-bit sweep against the executable-head oracle
(Milestone Q).

The m1..m4 grid (RFC extra-bit widths consumed or not) leaves the eCos head
non-executable in every case — including a reserved instruction at +0x50
(m1/m3) whose bytes decode as LITERALS, i.e. ground truth. The device
boots, so the TRUE accounting must parse the head into executable code.

A simple hardware engine reads FIXED-width extra fields and masks them per
symbol. This sweep tries every (len-extra width, dist-extra width) uniform
combo, decodes the first 0x200 output bytes, and scores the head:

  * first reserved word (lower = worse; none = good),
  * first branch/jump word,
  * branch count in 0x200 (eCos vector pages are branch-heavy).

Value semantics: len = LBASE + (val & (2^LEXTRA-1)), dist = DBASE + ...
(the head is zero-dominated, so the mask choice barely matters there).

Usage: python3 tools/fw_head_sweep.py <flash.bin>
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import (Huffman, LBASE, LEXTRA, DBASE, DEXTRA,
                          parse_header, BitReader)
from fw_first_corruption import realize, head_words

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C


def walk_uniform(stream, start_bit, lith, disth, wl, wd, out_cap):
    """wl/wd = 'r' (RFC widths) or a fixed width in 0..8."""
    br = BitReader(stream, start_bit)
    events = []
    outpos = 0
    while (br.pos >> 3) < len(stream) and outpos < out_cap:
        try:
            s = lith.decode(br)
        except EOFError:
            break
        if s <= 255:
            events.append(dict(kind="LIT", sym=s, out=outpos))
            outpos += 1
            continue
        if s == 256:
            events.append(dict(kind="EOB", out=outpos))
            continue
        li = s - 257
        w = LEXTRA[li] if wl == "r" else wl
        eb = br.bits(w) if w else 0
        try:
            ds = disth.decode(br)
        except (EOFError, ValueError):
            events.append(dict(kind="ERR", out=outpos))
            break
        w2 = DEXTRA[ds] if wd == "r" else wd
        deb = br.bits(w2) if w2 else 0
        mask_l = (1 << LEXTRA[li]) - 1 if LEXTRA[li] else 0
        mask_d = (1 << DEXTRA[ds]) - 1 if DEXTRA[ds] else 0
        ln = LBASE[li] + (eb & mask_l)
        d = DBASE[ds] + (deb & mask_d)
        events.append(dict(kind="MATCH", sym=s, dsym=ds, eb=eb, deb=deb,
                           dist=d, out=outpos, n=ln))
        outpos += ln
    return events


def head_score(out):
    first_res = None
    first_br = None
    n_br = 0
    for off, w, cls in head_words(out, 0x200):
        if cls.startswith("RESERVED") and first_res is None:
            first_res = off
        if cls in ("branch", "jump"):
            n_br += 1
            if first_br is None:
                first_br = off
    return first_res, first_br, n_br


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)

    widths = ["r"] + list(range(0, 9))
    rows = []
    for wl in widths:
        for wd in widths:
            ev = walk_uniform(stream, 1024, lith, disth, wl, wd, 0x400)
            out, _ = realize(ev, 0x8000)
            fr, fb, nb = head_score(out)
            rows.append((wl, wd, fr, fb, nb, len(out)))
    # rank: no reserved word first, then late reserved word, then branches
    def key(r):
        fr = 0x10000 if r[2] is None else r[2]
        return (fr, -r[4])
    rows.sort(key=key)
    print(f"{'wl':>2} {'wd':>2} | {'1st_reserved':>12} {'1st_branch':>10} "
          f"{'br@0x200':>8} | head[0:16]")
    for wl, wd, fr, fb, nb, olen in rows[:25]:
        ev = walk_uniform(stream, 1024, lith, disth, wl, wd, 0x80)
        out, _ = realize(ev, 0x8000)
        print(f"{str(wl):>2} {str(wd):>2} | "
              f"{'none' if fr is None else hex(fr):>12} "
              f"{'-' if fb is None else hex(fb):>10} {nb:>8} | "
              f"{out[:16].hex(' ')}")


if __name__ == "__main__":
    main()
