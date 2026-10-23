#!/usr/bin/env python3
"""Tail-structure probe with a Kraft-completeness filter (Milestone N).

§16.1's find-b2 searched bits 106,259..107,159 for a TERMINATING block
decode; complete (Kraft=1) tables made every bit pattern decodable and
only degenerate fixed-table artifacts surfaced. This probe changes the
oracle: a REAL new block header parses to a code-length sequence whose
litlen+dist tables are KRAFT-COMPLETE (sum of 2^-len == 1). Random bits
essentially never satisfy that, so Kraft=1 hits are structural.

Modes:
  kraft   every bit offset in a window after EOB#1 where a dynamic header
          (extended bounds: hlit<=512, hdist<=32) yields Kraft==1 tables;
          also reports the 1024-bit-grid offsets EOB1+k*1024.
  fixed   per-bit 3-bit header parse selecting btype=1 (fixed) — reports
          which offsets select it (structure check).
  profile per-1024B strict-validity profile of the RFC decode (padding /
          corruption-structure view of the tail output).

Usage: python3 tools/fw_tail_probe.py <flash.bin> [kraft|fixed|profile]
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import Huffman, LBASE, DBASE, parse_header, symbol_walk
from fw_ring_sweep import Validator
from fw_stream_structure import BR

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
EOB1_END = 106_259


def cl_tables(br, extended):
    """Parse a candidate dynamic header; return (litlens, distlens) or None."""
    hlit = br.bits(5) + 257
    hdist = br.bits(5) + 1
    hclen = br.bits(4) + 4
    lim_l, lim_d = (512, 32) if extended else (286, 30)
    if hlit > lim_l or hdist > lim_d:
        return None
    cl = [0] * 19
    from fw_engine_lz import CL_ORDER
    for i in range(hclen):
        cl[CL_ORDER[i]] = br.bits(3)
    # code-length table must itself be decodable
    try:
        clh = Huffman(cl)
    except Exception:
        return None
    if not clh.maxbits:
        return None
    lens = []
    try:
        while len(lens) < hlit + hdist:
            s = clh.decode(br)
            if s <= 15:
                lens.append(s)
            elif s == 16:
                if not lens:
                    return None
                lens += [lens[-1]] * (br.bits(2) + 3)
            elif s == 17:
                lens += [0] * (br.bits(3) + 3)
            else:
                lens += [0] * (br.bits(7) + 11)
    except (ValueError, EOFError):
        return None
    return lens[:hlit], lens[hlit:]


def kraft(lens):
    tot = 0
    for l in lens:
        if l:
            tot += 2.0 ** (-l)
    return tot


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    bfinal, litlens, distlens, hb = parse_header(stream)
    mode = sys.argv[2] if len(sys.argv) > 2 else "kraft"

    if mode == "profile":
        lith, disth = Huffman(litlens), Huffman(distlens)
        events = symbol_walk(stream, 1024, lith, disth)
        out = bytearray()
        pos_at = []  # event index -> out position
        for e in events:
            if e["kind"] == "LIT":
                out.append(e["sym"])
            elif e["kind"] == "MATCH":
                ln = LBASE[e["sym"] - 257] + e["eb"]
                d = DBASE[e["dsym"]] + e["deb"]
                n = len(out)
                for i in range(ln):
                    out.append(out[n - d + i] if d <= n else 0)
        val = Validator(flash)
        print(f"RFC output {len(out)} bytes; per-1024B strict validity:")
        row = []
        for c in range(0, len(out) // 1024 + 1):
            seg = bytes(out[c * 1024:(c + 1) * 1024])
            if not seg:
                break
            s, n = val.strict(seg)
            row.append(f"{c * 1024 // 1024:3d}:{s * 100:4.0f}")
        for i in range(0, len(row), 13):
            print("  " + " ".join(row[i:i + 13]))
        # tail of the output — is there zero padding?
        print("tail[79_000:79_872?] n/a; last 64 bytes:",
              bytes(out[-64:]).hex(" "))
        nz = sum(1 for b in out if b)
        print(f"nonzero ratio {nz / len(out):.3f}")
        return

    if mode == "fixed":
        hits = []
        for t in range(EOB1_END, EOB1_END + 2048):
            br = BR(stream, t)
            bfinal = br.bit()
            btype = br.bits(2)
            if btype == 1:
                hits.append((t, bfinal))
        print(f"btype=1 selections in [{EOB1_END},{EOB1_END + 2048}): "
              f"{len(hits)} (expected ~{2048 // 8})")
        return

    # kraft mode
    print(f"scanning bit offsets {EOB1_END}..{EOB1_END + 4096} "
          f"for Kraft-complete dynamic headers (extended bounds)")
    grid = {EOB1_END + k * 1024 for k in range(0, 5)}
    hits = []
    for t in range(EOB1_END, EOB1_END + 4096):
        for extended in (False, True):
            br = BR(stream, t)
            try:
                bfinal = br.bit()
                btype = br.bits(2)
            except EOFError:
                break
            if btype != 2:
                continue
            r = cl_tables(br, extended)
            if r is None:
                continue
            ll, dl = r
            kl, kd = kraft(ll), kraft(dl)
            if abs(kl - 1.0) < 1e-9 and abs(kd - 1.0) < 1e-9:
                mark = " GRID" if t in grid else ""
                hits.append(t)
                print(f"  bit {t}: bfinal={bfinal} hlit={len(ll)} "
                      f"hdist={len(dl)} Kraft=1/1 header_bits="
                      f"{br.pos - t}{mark}")
    if not hits:
        print("  no Kraft-complete dynamic header found in window")


if __name__ == "__main__":
    main()
