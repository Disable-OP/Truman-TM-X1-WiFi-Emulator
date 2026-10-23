#!/usr/bin/env python3
"""Literal-vs-match provenance diagnostic (Milestone N).

The RFC walk's output is ~48% strict-valid. This tool splits the output
by BYTE PROVENANCE (literal vs match copy) and scores strict validity on
  (a) words whose 4 bytes all come from literals,
  (b) words containing at least one match byte,
  (c) everything (baseline).
If (a) is code-like (~90%), literals and the symbol->byte mapping are
right and only match semantics are wrong. If (a) is ~50%, the symbol
mapping itself is suspect (per-length-class permutation).

Also prints the provenance map of the first 128 bytes.

Usage: python3 tools/fw_literal_diagnostic.py <flash.bin>
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import Huffman, LBASE, DBASE, parse_header, symbol_walk
from fw_ring_sweep import Validator

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)
    events = symbol_walk(stream, 1024, lith, disth)

    out = bytearray()
    prov = bytearray()  # 0 = literal, 1 = match
    for e in events:
        if e["kind"] == "LIT":
            out.append(e["sym"])
            prov.append(0)
        elif e["kind"] == "MATCH":
            ln = LBASE[e["sym"] - 257] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            n = len(out)
            for i in range(ln):
                b = out[n - d + i] if d <= n else 0
                out.append(b)
                # provenance propagates from the SOURCE byte
                prov.append(prov[n - d + i] if d <= n else 0)
    print(f"output {len(out)} bytes; literal bytes "
          f"{sum(1 for p in prov if p == 0)} match bytes "
          f"{sum(1 for p in prov if p == 1)}")

    val = Validator(flash)
    groups = {"all-literal": [], "has-match": []}
    for i in range(0, (len(out) // 4) * 4, 4):
        ps = prov[i:i + 4]
        g = "all-literal" if all(p == 0 for p in ps) else "has-match"
        groups[g].append(i)
    for g, idxs in groups.items():
        ok = sum(1 for i in idxs if val.word_valid(bytes(out[i:i + 4])))
        print(f"{g:12s} words={len(idxs):6d} strict={ok / max(len(idxs), 1) * 100:5.1f}%")
    s, n = val.strict(bytes(out))
    print(f"{'overall':12s} words={n:6d} strict={s * 100:5.1f}%")

    print("provenance of first 128 bytes (L=literal M=match):")
    print("  " + "".join("L" if p == 0 else "M" for p in prov[:128]))
    print("first 64 bytes:", bytes(out[:64]).hex(" "))


if __name__ == "__main__":
    main()
