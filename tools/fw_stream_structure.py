#!/usr/bin/env python3
"""SP-container stream-structure probe (Milestone M).

Prior milestones parsed the "SP" container as ONE deflate block. The walk
finds symbol 256 (EOB) TWICE at mid-stream (RFC-out 28,260 and 45,137).
This tool settles the framing question with three probes:

  chain    decode blocks honoring byte-aligned block headers after each
           EOB (the only candidate header position found by probe_dyn);
           reports every block, its output range and strict validity.
  find-b2  exhaustive per-bit search for a terminating dynamic/fixed block
           header in a window after the first EOB (the real filter is not
           "header looks plausible" — complete tables decode anything —
           but "the subsequent decode terminates with EOB inside the
           stream").
  kraft    Kraft sums of the first block's litlen/dist tables. Both are
           1 (complete), which proves "zero bad codes" never protected
           the single-block parse of the tail.

Findings (docs/firmware-analysis.md §16.1): block 1 is a well-formed
FINAL dynamic block (bfinal=1) ending at bit 106,259 with 28,260 bytes;
no standard continuation (dynamic/fixed/stored) exists after it.

Usage: python3 tools/fw_stream_structure.py <flash.bin> [chain|find-b2|kraft]
"""
import bisect
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import (Huffman, LBASE, DBASE, LEXTRA, DEXTRA, CL_ORDER,
                         parse_header, symbol_walk)
from fw_ring_sweep import Validator

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
EOB1_END = 106_259  # end bit of block 1's EOB code (verified by chain)

LIT_FIXED = [8] * 144 + [9] * 112 + [7] * 24 + [8] * 8
DIST_FIXED = [5] * 30


class BR:
    __slots__ = ("data", "pos")

    def __init__(self, data, pos=0):
        self.data, self.pos = data, pos

    def bit(self):
        if (self.pos >> 3) >= len(self.data):
            raise EOFError(f"bit {self.pos}")
        b = (self.data[self.pos >> 3] >> (self.pos & 7)) & 1
        self.pos += 1
        return b

    def bits(self, n):
        v = 0
        for i in range(n):
            v |= self.bit() << i
        return v

    def align(self):
        self.pos = (self.pos + 7) & ~7


def parse_dyn_header(br):
    """Dynamic header; rejects out-of-range hlit/hdist (RFC bounds)."""
    hlit = br.bits(5) + 257
    hdist = br.bits(5) + 1
    hclen = br.bits(4) + 4
    if hlit > 286 or hdist > 30:
        raise ValueError(f"hlit={hlit} hdist={hdist} out of range")
    cl = [0] * 19
    for i in range(hclen):
        cl[CL_ORDER[i]] = br.bits(3)
    clh = Huffman(cl)
    lens = []
    while len(lens) < hlit + hdist:
        s = clh.decode(br)
        if s <= 15:
            lens.append(s)
        elif s == 16:
            if not lens:
                raise ValueError("repeat code 16 with empty history")
            lens += [lens[-1]] * (br.bits(2) + 3)
        elif s == 17:
            lens += [0] * (br.bits(3) + 3)
        else:
            lens += [0] * (br.bits(7) + 11)
    return lens[:hlit], lens[hlit:hlit + hdist]


def decode_body(stream, br, litlens, distlens, out):
    """Decode one block body (symbols to EOB) appending to out."""
    lith, disth = Huffman(litlens), Huffman(distlens)
    b0, o0, nsym = br.pos, len(out), 0
    while True:
        s = lith.decode(br)
        nsym += 1
        if s <= 255:
            out.append(s)
        elif s == 256:
            break
        else:
            li = s - 257
            if li >= len(LEXTRA):
                raise ValueError(f"length symbol {s} at bit {br.pos}")
            eb = br.bits(LEXTRA[li]) if LEXTRA[li] else 0
            ds = disth.decode(br)
            deb = br.bits(DEXTRA[ds]) if DEXTRA[ds] else 0
            ln = LBASE[li] + eb
            d = DBASE[ds] + deb
            n = len(out)
            for i in range(ln):
                out.append(out[n - d + i] if d <= n else 0)
    return dict(kind="dyn", bit0=b0, endbit=br.pos, syms=nsym,
                out0=o0, out1=len(out))


def load_events(flash):
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    assert cont[:2] == b"SP", cont[:2]
    stream = cont[8:]
    bfinal, litlens, distlens, hb = parse_header(stream)
    return stream, litlens, distlens


def cmd_chain(flash):
    stream, litlens, distlens = load_events(flash)
    v = Validator(flash)
    out = bytearray()
    br = BR(stream, 1024)
    rec = decode_body(stream, br, litlens, distlens, out)
    sc, n = v.strict(out, rec["out0"], rec["out1"] - rec["out0"])
    print(f"block 1 (dynamic, bfinal=1): {rec}")
    print(f"  strict validity: {sc:.3f} over {n} words")
    tail_bits = len(stream) * 8 - rec["endbit"]
    print(f"  stream bits remaining after block 1: {tail_bits}")
    # try the standard continuations at the only plausible positions
    for name, pos in (("immediate", rec["endbit"]),
                      ("byte-aligned", (rec["endbit"] + 7) & ~7)):
        b = BR(stream, pos)
        try:
            bfinal, btype = b.bit(), b.bits(2)
            print(f"  {name} continuation at bit {pos}: "
                  f"bfinal={bfinal} btype={btype} -> ", end="")
            if btype == 2:
                t = parse_dyn_header(b)
                r = decode_body(stream, BR(stream, b.pos), *t, bytearray())
                print(f"decodes {r['syms']} symbols, EOB at {r['endbit']}"
                      f" (bit {r['endbit']} < stream end:"
                      f" {r['endbit'] < len(stream) * 8})")
            elif btype == 1:
                r = decode_body(stream, BR(stream, b.pos), LIT_FIXED,
                                DIST_FIXED, bytearray())
                print(f"decodes {r['syms']} symbols")
            elif btype == 0:
                b.align()
                ln, nlen = b.bits(16), b.bits(16)
                print(f"stored LEN={ln} NLEN={nlen:04x} "
                      f"complement_ok={ln ^ 0xFFFF == nlen}")
            else:
                print("invalid btype 3")
        except (ValueError, EOFError) as e:
            print(f"REFUTED ({type(e).__name__}: {e})")


def cmd_find_b2(flash, w0=EOB1_END, w1=EOB1_END + 900):
    """Exhaustive terminating-header search after the first EOB."""
    stream, _, _ = load_events(flash)
    hits = 0
    for p in range(w0, w1):
        b = BR(stream, p)
        try:
            bfinal, btype = b.bit(), b.bits(2)
        except EOFError:
            break
        if btype not in (1, 2):
            continue
        try:
            if btype == 2:
                t = parse_dyn_header(BR(stream, b.pos))
                bb = BR(stream, b.pos)
            else:
                t = (LIT_FIXED, DIST_FIXED)
                bb = BR(stream, b.pos)
            r = decode_body(stream, bb, *t, bytearray())
        except (ValueError, EOFError):
            continue
        if r["endbit"] < len(stream) * 8 and r["syms"] > 32:
            hits += 1
            print(f"terminating block: hdr at bit {p} bfinal={bfinal} "
                  f"btype={btype} endbit={r['endbit']} syms={r['syms']} "
                  f"out={r['out1'] - r['out0']}B")
    if not hits:
        print(f"no terminating dynamic/fixed block header in bits "
              f"{w0}..{w1} (all candidates either fail to decode or run "
              f"past the stream end)")


def cmd_kraft(flash):
    from fractions import Fraction
    _, litlens, distlens = load_events(flash)
    for name, lens in (("litlen", litlens), ("dist", distlens)):
        used = [l for l in lens if l]
        kraft = sum(Fraction(1, 2 ** l) for l in used)
        print(f"{name}: {len(used)} symbols, Kraft sum = {kraft} "
              f"(complete: {kraft == 1})")


def main():
    flash = open(sys.argv[1], "rb").read()
    cmd = sys.argv[2] if len(sys.argv) > 2 else "chain"
    if cmd == "chain":
        cmd_chain(flash)
    elif cmd == "find-b2":
        cmd_find_b2(flash)
    elif cmd == "kraft":
        cmd_kraft(flash)
    else:
        print(__doc__)
        sys.exit(1)


if __name__ == "__main__":
    main()
