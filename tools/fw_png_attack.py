#!/usr/bin/env python3
"""PNG-structured decode attack (Milestone N).

The "decompression engine" is a PNG decoder (regfile.h: PNG_DEC_*,
0xBFFE3800..0x387C). The firmware programs: IMG_WIDTH=0x400 (1024),
IMG_HEIGHT=78, IMG_TYPE=0x68, PXL output 0x01600000, and the boot JUMPS
into the pixel buffer — so the 1024x78 8-bpp image IS the eCos binary.

A PNG zlib stream decodes to [filter byte][1024 filtered bytes] per row:
78 rows -> 79,950 inflate-output bytes. The RFC walk yields 75,398, so
under a candidate LZ semantics the engine's match lengths L' = L + adj
(on matches selected by a predicate) must satisfy
  sum(L') = 79,950 - 15,761 literals  =>  sum(adj) = 4,552  (RFC sum 59,637)

Oracle chain per candidate (each stage is near-zero-probability for
garbage):
  1. output size == 79,950
  2. bytes at row starts (stride 1025) all in {0,1,2,3,4}  (PNG filters)
  3. unfilter (Sub/Up/Average/Paeth) -> binary; strict MIPS validity

Usage: python3 tools/fw_png_attack.py <flash.bin>
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import Huffman, LBASE, DBASE, parse_header, symbol_walk
from fw_ring_sweep import Validator

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
WIDTH = 1024
HEIGHT = 78
STRIDE = WIDTH + 1           # filter byte + row
TARGET = HEIGHT * STRIDE     # 79,950


def load(flash):
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    assert cont[:2] == b"SP"
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)
    events = symbol_walk(stream, 1024, lith, disth)
    lits = sum(1 for e in events if e["kind"] == "LIT")
    matches = []
    for e in events:
        if e["kind"] == "MATCH":
            ln = LBASE[e["sym"] - 257] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            matches.append((ln, d, e["out"]))
    return lits, matches


def decode(lits, matches, pred, adj):
    """Linear-window decode; matches emit L+adj when pred(len,dist)."""
    win = bytearray(0x8000)
    out = bytearray()
    for ln, d, _ in matches:
        pass
    # replay literals+matches in stream order using events rebuilt below
    return None  # replaced by decode_events


def decode_events(events, pred, adj):
    win = bytearray(0x8000)
    out = bytearray()
    for e in events:
        k = e["kind"]
        if k == "LIT":
            win.append(e["sym"])
            out.append(e["sym"])
        elif k == "MATCH":
            ln = LBASE[e["sym"] - 257] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            n = len(win)
            L = ln + adj if pred(ln, d) else ln
            if d > n:
                # zero-preset window: bytes before output are zeros
                for _ in range(L):
                    win.append(0)
                    out.append(0)
            else:
                for i in range(L):
                    b = win[n - d + i]
                    win.append(b)
                    out.append(b)
            if len(out) > TARGET + 4096:
                return bytes(out), "size_over"
    return bytes(out), "end"


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def unfilter(data, width, height):
    """PNG unfilter; rows of width+1 bytes (leading filter byte)."""
    bpp = 1
    rows = []
    prev = bytearray(width)
    for r in range(height):
        off = r * (width + 1)
        f = data[off]
        row = bytearray(data[off + 1:off + 1 + width])
        if f == 0:
            pass
        elif f == 1:
            for i in range(bpp, width):
                row[i] = (row[i] + row[i - bpp]) & 0xFF
        elif f == 2:
            for i in range(width):
                row[i] = (row[i] + prev[i]) & 0xFF
        elif f == 3:
            for i in range(width):
                a = row[i - bpp] if i >= bpp else 0
                row[i] = (row[i] + (a + prev[i]) // 2) & 0xFF
        elif f == 4:
            for i in range(width):
                a = row[i - bpp] if i >= bpp else 0
                c = prev[i - bpp] if i >= bpp else 0
                row[i] = (row[i] + paeth(a, prev[i], c)) & 0xFF
        else:
            return None, f
        rows.append(bytes(row))
        prev = row
    return b"".join(rows), None


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)
    events = symbol_walk(stream, 1024, lith, disth)
    n_lit = sum(1 for e in events if e["kind"] == "LIT")
    matches = []
    for e in events:
        if e["kind"] == "MATCH":
            ln = LBASE[e["sym"] - 257] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            matches.append((ln, d, e["out"]))
    rfc_sum = sum(m[0] for m in matches)
    delta = TARGET - n_lit - rfc_sum
    print(f"literals={n_lit} matches={len(matches)} rfc_sum={rfc_sum} "
          f"target={TARGET} required_extra={delta}")
    val = Validator(flash)

    import bisect
    divs = [k for k in range(1, delta + 1) if delta % k == 0]
    targets = set(divs)
    lens = sorted({m[0] for m in matches})
    ds_sorted = sorted(m[1] for m in matches)
    dth = [1, 2, 3, 4, 5, 8, 9, 13, 16, 17, 25, 32, 33, 49, 64, 65, 96, 97,
           128, 129, 193, 256, 257, 385, 512, 513, 769, 1024, 1025, 1537,
           2048, 2049, 3072, 3073, 3485, 4096, 4097, 6145, 8192, 8193,
           12289, 16384, 16385, 24577, 24578, 32668, 32768]
    preds = {"true": lambda l, d: True}
    for a in lens:
        preds[f"len>={a}"] = (lambda a: lambda l, d: l >= a)(a)
        preds[f"len<={a}"] = (lambda a: lambda l, d: l <= a)(a)
    for b in sorted(set(ds_sorted)):
        preds[f"dist>={b}"] = (lambda b: lambda l, d: d >= b)(b)
        preds[f"dist<{b}"] = (lambda b: lambda l, d: d < b)(b)
    for a in lens:
        for b in dth:
            preds[f"len>={a}&dist>={b}"] = (
                lambda a, b: lambda l, d: l >= a and d >= b)(a, b)
            preds[f"len>={a}&dist<{b}"] = (
                lambda a, b: lambda l, d: l >= a and d < b)(a, b)

    tested = 0
    results = []
    for name, pred in preds.items():
        c = sum(1 for m in matches if pred(m[0], m[1]))
        if c not in targets:
            continue
        adj = delta // c
        tested += 1
        out, why = decode_events(events, pred, adj)
        if len(out) < TARGET:
            results.append(f"{name} adj={adj}: out={len(out)} SHORT")
            continue
        # filter-byte check
        fbs = [out[r * STRIDE] for r in range(HEIGHT)]
        bad = [(r, v) for r, v in enumerate(fbs) if v > 4]
        if bad:
            results.append(f"{name} adj={adj}: filterfail {len(bad)}/{HEIGHT} "
                           f"(first {bad[:3]})")
            continue
        img, f = unfilter(out, WIDTH, HEIGHT)
        if f is not None:
            results.append(f"{name} adj={adj}: badfilter {f}")
            continue
        s, n = val.strict(img)
        results.append(f"{name} adj={adj}: *** FILTER-OK unfiltered strict="
                       f"{s * 100:.1f}% len={len(img)}")
    print(f"tested {tested} candidate (predicate, adjustment) pairs")
    for r in results[:60]:
        print(" ", r)


if __name__ == "__main__":
    main()
