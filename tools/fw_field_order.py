#!/usr/bin/env python3
"""Match-token field-order probe (Milestone N).

§14.1's parse verification shows the RFC-order walk decodes the whole
declared stream with the uploaded tables — but the litlen/dist tables are
Kraft-COMPLETE, so any bit consumption order is equally "valid". The
hardware's actual token layout was never tested beyond RFC. This tool
decodes the stream under every plausible match-token field order and
scores output size (vs the hardware-reported 79,872) + strict validity.

  o1  [len_sym][len_eb][dist_sym][dist_eb]   RFC 1951
  o2  [len_sym][dist_sym][len_eb][dist_eb]
  o3  [len_sym][dist_sym][dist_eb][len_eb]
  o4  [dist_sym][dist_eb][len_sym][len_eb]

Usage: python3 tools/fw_field_order.py <flash.bin>
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import (Huffman, LBASE, DBASE, LEXTRA, DEXTRA,
                          parse_header, BitReader)
from fw_ring_sweep import Validator

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
TARGET = 79_872
WINDOW = 0x8000


def decode_order(stream, start_bit, lith, disth, order):
    """order: sequence like ('len','leneb','dist','disteb') — extra-bit
    groups consume LEXTRA/DEXTRA of the symbol decoded before them."""
    br = BitReader(stream, start_bit)
    out = bytearray()
    win = bytearray(WINDOW)
    n_sym = 0
    why = "stream_end"
    while (br.pos >> 3) < len(stream):
        try:
            sym = lith.decode(br)
            n_sym += 1
            if sym <= 255:
                win.append(sym)
                out.append(sym)
                continue
            if sym == 256:
                continue  # marker semantics (engine model)
            fields = {}
            for f in order:
                if f == "len":
                    fields["len"] = sym
                elif f == "leneb":
                    fields["leneb"] = br.bits(LEXTRA[sym - 257]) if LEXTRA[sym - 257] else 0
                elif f == "dist":
                    fields["dist"] = disth.decode(br)
                elif f == "disteb":
                    ds = fields.get("dist")
                    fields["disteb"] = br.bits(DEXTRA[ds]) if DEXTRA[ds] else 0
        except EOFError:
            why = "input_end"
            break
        except ValueError as e:
            return out, n_sym, f"bad_code@{br.pos}"
        ln = LBASE[fields["len"] - 257] + fields.get("leneb", 0)
        d = DBASE[fields["dist"]] + fields.get("disteb", 0)
        n = len(win)
        if d > n:
            return out, n_sym, "dist_overrun"
        for i in range(ln):
            b = win[n - d + i]
            win.append(b)
            out.append(b)
        if len(out) > TARGET + 4096:
            return out, n_sym, "size_over"
    return out, n_sym, why


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)
    val = Validator(flash)
    orders = {
        "o1 RFC len leneb dist disteb":
            ("len", "leneb", "dist", "disteb"),
        "o2 len dist leneb disteb":
            ("len", "dist", "leneb", "disteb"),
        "o3 len dist disteb leneb":
            ("len", "dist", "disteb", "leneb"),
        "o4 dist disteb len leneb":
            ("dist", "disteb", "len", "leneb"),
    }
    for name, order in orders.items():
        try:
            out, n, why = decode_order(stream, 1024, lith, disth, order)
        except (ValueError, EOFError) as e:
            print(f"{name:32s} FAIL {e}")
            continue
        s, n4 = val.strict(bytes(out[:TARGET]))
        print(f"{name:32s} out={len(out):6d} syms={n} stop={why:13s} "
              f"strict={s * 100:5.1f}% head={bytes(out[:8]).hex(' ')}")


if __name__ == "__main__":
    main()
