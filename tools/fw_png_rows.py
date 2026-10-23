#!/usr/bin/env python3
"""Row-geometry walk: raw filter bytes between 1024-byte token rows.

Model (PNG hardware decoder semantics): the engine reads each row's
filter byte as 8 RAW bits from the bitstream, then Huffman-decodes
exactly 1024 bytes of tokens for that row. Total inflate output =
78 x 1024 = 79,872 (matches the programmed size exactly — no +4,474
gap, no +78 filter bytes). The mid-stream EOBs of the naive single-
block walk are misalignment artifacts.

Variants tested: start bit 1024 (programmed) / 1008 (header end);
window reset per row vs linear 32 KiB filtered window.

Checks: 78 rows complete, filter bytes in {0..4}, unfiltered image
strict MIPS validity.

Usage: python3 tools/fw_png_rows.py <flash.bin>
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import (Huffman, LBASE, DBASE, LEXTRA, DEXTRA,
                          parse_header, BitReader)
from fw_ring_sweep import Validator

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
WIDTH = 1024
HEIGHT = 78


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def unfilter(rows_filters):
    bpp = 1
    out = bytearray()
    prev = bytearray(WIDTH)
    for f, row in rows_filters:
        row = bytearray(row)
        if f == 1:
            for i in range(bpp, WIDTH):
                row[i] = (row[i] + row[i - bpp]) & 0xFF
        elif f == 2:
            for i in range(WIDTH):
                row[i] = (row[i] + prev[i]) & 0xFF
        elif f == 3:
            for i in range(WIDTH):
                a = row[i - bpp] if i >= bpp else 0
                row[i] = (row[i] + (a + prev[i]) // 2) & 0xFF
        elif f == 4:
            for i in range(WIDTH):
                a = row[i - bpp] if i >= bpp else 0
                c = prev[i - bpp] if i >= bpp else 0
                row[i] = (row[i] + paeth(a, prev[i], c)) & 0xFF
        out += row
        prev = row
    return bytes(out)


def row_walk(stream, start_bit, lith, disth, window_reset):
    """Walk HEIGHT rows; each row = 8 raw bits (filter) + tokens for
    WIDTH bytes. Returns (rows_filters, stop_reason)."""
    br = BitReader(stream, start_bit)
    win = bytearray(0x8000)  # linear filtered window (zero preset)
    rows = []
    for r in range(HEIGHT):
        try:
            f = br.bits(8)
        except EOFError:
            return rows, f"EOF@filter_row{r}"
        row = bytearray()
        while len(row) < WIDTH:
            try:
                s = lith.decode(br)
            except EOFError:
                return rows, f"EOF@row{r}+{len(row)}"
            except ValueError as e:
                return rows, f"badcode@row{r}+{len(row)} bit{br.pos}"
            if s <= 255:
                row.append(s)
                win.append(s)
            elif s == 256:
                continue  # marker (engine decodes on; hardware row count)
            else:
                li = s - 257
                try:
                    eb = br.bits(LEXTRA[li]) if LEXTRA[li] else 0
                    ds = disth.decode(br)
                    deb = br.bits(DEXTRA[ds]) if DEXTRA[ds] else 0
                except EOFError:
                    return rows, f"EOF@match_row{r}"
                ln = LBASE[li] + eb
                d = DBASE[ds] + deb
                n = len(win)
                if window_reset:
                    base = win  # window includes all previous rows anyway
                    src = n - d
                    for i in range(ln):
                        b = win[src + i] if src + i >= 0 and src + i < n else 0
                        row.append(b)
                        win.append(b)
                else:
                    for i in range(ln):
                        b = win[n - d + i] if d <= n else 0
                        row.append(b)
                        win.append(b)
        rows.append((f, bytes(row[:WIDTH])))
    return rows, "complete"


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)
    val = Validator(flash)

    for start in (1024, 1008, 1016, 1032):
        for wr in (True, False):
            rows, why = row_walk(stream, start, lith, disth, wr)
            if len(rows) < HEIGHT:
                print(f"start={start} winreset={wr}: {why} "
                      f"rows={len(rows)}/{HEIGHT}")
                continue
            fbs = [f for f, _ in rows]
            okf = all(0 <= f <= 4 for f in fbs)
            img = unfilter(rows)
            s, n = val.strict(img)
            sraw, _ = val.strict(b"".join(r for _, r in rows))
            print(f"start={start} winreset={wr}: complete! filters_ok={okf} "
                  f"filters={fbs[:20]} unfiltered_strict={s * 100:.1f}% "
                  f"raw_strict={sraw * 100:.1f}%")
            if okf:
                open("/tmp/png_rows.img", "wb").write(img)
                print("  unfiltered image -> /tmp/png_rows.img")


if __name__ == "__main__":
    main()
