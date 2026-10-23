#!/usr/bin/env python3
"""Fixed-ladder walk: engine variants that consume NO extra bits (Milestone O).

A simple hardware LZ implements length/distance as FIXED ladders (one
value per symbol, ROM constants) — the RFC "extra bits" are then NOT
consumed and re-enter the bitstream as the following symbols' bits. The
RFC-order "verified parse" does not exclude this (Kraft-complete tables
decode any bit pattern), and the size direction is right: freeing the
extra bits yields MORE symbols and MORE output from the same stream.

Four models:
  m1  RFC (len extra + dist extra consumed)  — baseline 75,398
  m2  len extra consumed, dist extra NOT
  m3  len extra NOT, dist extra consumed
  m4  neither consumed (pure fixed ladders)

Distances/lengths = base tables (RFC values as hypothesis H1). Scoring:
output size vs the 79,872 image, strict validity, head.

Usage: python3 tools/fw_fixed_ladder.py <flash.bin>
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import (Huffman, LBASE, LEXTRA, DBASE, DEXTRA,
                          parse_header, BitReader)
from fw_ring_sweep import Validator

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
IMG = 79_872
WINDOW = 0x8000


def walk(stream, start_bit, lith, disth, len_extra, dist_extra, cap):
    br = BitReader(stream, start_bit)
    win = bytearray(WINDOW)
    out = bytearray()
    n_sym = 0
    why = "input_end"
    while (br.pos >> 3) < len(stream):
        try:
            s = lith.decode(br)
            n_sym += 1
            if s <= 255:
                win.append(s)
                out.append(s)
                if len(out) >= cap:
                    return bytes(out[:cap]), n_sym, "image_full"
                continue
            if s == 256:
                continue
            li = s - 257
            ln = LBASE[li]
            if len_extra:
                ln += br.bits(LEXTRA[li]) if LEXTRA[li] else 0
            ds = disth.decode(br)
            d = DBASE[ds]
            if dist_extra:
                d += br.bits(DEXTRA[ds]) if DEXTRA[ds] else 0
        except EOFError:
            break
        except ValueError:
            return bytes(out), n_sym, f"bad_code@{br.pos}"
        n = len(win)
        for j in range(ln):
            b = win[n - d + j] if d <= n else 0
            win.append(b)
            out.append(b)
        if len(out) >= cap:
            return bytes(out[:cap]), n_sym, "image_full"
    return bytes(out), n_sym, why


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)
    val = Validator(flash)
    models = [
        ("m1 RFC (len+dist extra)", True, True),
        ("m2 len extra only", True, False),
        ("m3 dist extra only", False, True),
        ("m4 fixed ladders (none)", False, False),
    ]
    for name, le, de in models:
        out, n, why = walk(stream, 1024, lith, disth, le, de, IMG + 64)
        s, nw = val.strict(out[:IMG])
        print(f"{name:26s} out={len(out):6d} syms={n:6d} stop={why:11s} "
              f"strict={s * 100:5.1f}% head={out[:8].hex(' ')}")


if __name__ == "__main__":
    main()
