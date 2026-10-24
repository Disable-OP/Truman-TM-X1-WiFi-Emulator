#!/usr/bin/env python3
"""Rank-boundary class rule + output-transform families (Milestone S).

Milestone R (docs/firmware-analysis.md §20.4) left three hypothesis classes
for the executable-head paradox:

  1. the class assignment is RANK-driven (§20.2): the engine's litlen table
     is a byte table indexed by canonical rank, so an engine that classifies
     tokens from the rank alone reads rank < 256 as a literal byte T[rank],
     rank 256 as EOB and rank > 256 as a length selector r-257 (which
     indexes a 29-entry hardware length ladder exactly). The token STREAM
     changes (classes move), so every §19/§20 static refutation is void.
  2. an unmodeled output transform between the LZ output and the pixel
     buffer (byte-lane permutations et al.), and
  3. the container plaintext pre-transformed by the SDK tool — oracle-wise
     the same object as (2): execute T(identity decode).

This tool generates both families and pre-screens them with the static
executable-head oracle (fw_first_corruption.word_class, §19.2). Survivors
go to examples/entry_oracle.rs (the register-aware dynamic oracle).

Self-checks (run before any verdict is reported):
  * class_rule="sym" + RFC ladder + m1/m2/m4 extras must reproduce the
    recorded §19.2/§20.3 static verdicts byte-for-byte (m1 FATAL +0x50
    0000ffff, m2 FATAL +0x18 6200003b, m4 branch at +0x18);
  * the m1 identity walk must start 60 81 ... (§19.1);
  * the m1 identity output must byte-match fw_syms_family.render()
    (imported and compared directly).

Usage: python3 tools/fw_rank_transform.py <flash.bin> [--outdir DIR]
"""
import itertools
import os
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import Huffman, BitReader, parse_header
from fw_first_corruption import word_class, head_words
from fw_syms_family import (CONTAINER_OFF, CONTAINER_LEN, IMG, WINDOW,
                            START_BIT, LBASE, LEXTRA, DBASE, DEXTRA,
                            canonical_tables, render, record_m1_events,
                            head_verdict, bitrev8, nibswap)

RANK_EOB = 256          # rank boundary: rank 256 = EOB, rank 257+ = length
SEL0 = 257              # first length rank; selector = rank - SEL0 (0..28)


# --------------------------------------------------------------------------
# parameterised walker
# --------------------------------------------------------------------------

class Ranks:
    """Canonical rank machinery for one table: sym_of_rank + cum_excl."""

    def __init__(self, lengths):
        h = Huffman(lengths)
        cum = [0] * (h.maxbits + 2)
        for b in range(2, h.maxbits + 2):
            cum[b] = cum[b - 1] + h.counts[b - 1]
        sym_of_rank = [None] * cum[h.maxbits + 1]
        for l in sorted(h.by_len):
            for k, s in enumerate(h.by_len[l]):
                sym_of_rank[cum[l] + k] = s
        self.sym_of_rank = sym_of_rank
        self.cum = cum


def walk(stream, start_bit, lith, disth, lit_ranks, dist_ranks,
         class_rule="rank", len_ladder="rfc", len_extra=True,
         dist_extra=True, cap=IMG + 64):
    """One bit-level walk with a pluggable class rule and length ladder.

    class_rule: "sym" (RFC value classes) or "rank" (rank < 256 literal,
        rank 256 EOB, rank 257+ length selector r-257).
    len_ladder: "rfc" (LBASE/LEXTRA indexed by the selector) or "u<k>"
        (uniform base k + selector, no length extra bits).
    Returns (out_bytes, stop_reason, stats).
    """
    br = BitReader(stream, start_bit)
    win = bytearray(WINDOW)
    out = bytearray()
    n_lit = n_eob = n_match = 0
    why = "input_end"
    try:
        while (br.pos >> 3) < len(stream):
            # litlen code -> canonical rank
            code, rank = 0, None
            for l in range(1, lith.maxbits + 1):
                code = (code << 1) | br.bit()
                c = lith.counts[l]
                if c and lith.first[l] <= code < lith.first[l] + c:
                    rank = lit_ranks.cum[l] + (code - lith.first[l])
                    break
            if rank is None:
                why = f"bad_code@{br.pos}"
                break
            # class + value
            if class_rule == "rank":
                kind = ("L" if rank < RANK_EOB
                        else "E" if rank == RANK_EOB else "M")
                sel = rank - SEL0 if kind == "M" else None
            elif class_rule == "t0":
                # the last single-table reading (§21): class from the
                # uploaded BYTE itself -- T=0 is EOB (symbol 256 uploads
                # as 0x00), T=1..29 is a length selector (the length
                # symbols upload as s-256), anything else is a literal.
                t = lit_ranks.sym_of_rank[rank] & 0xFF
                kind = "E" if t == 0 else "M" if t <= 29 else "L"
                sel = t - 1 if kind == "M" else None
            else:
                s = lit_ranks.sym_of_rank[rank]
                kind = "L" if s <= 255 else "E" if s == 256 else "M"
                sel = s - 257 if kind == "M" else None
            # sel = r-257 for the rank rule indexes a 29-entry ladder exactly
            # (286 coded symbols -> ranks 257..285 -> selectors 0..28, §20.2).
            if kind == "L":
                b = lit_ranks.sym_of_rank[rank] & 0xFF
                win.append(b)
                out.append(b)
                n_lit += 1
                if len(out) >= cap:
                    why = "image_full"
                    break
                continue
            if kind == "E":
                n_eob += 1
                continue
            # match: length from the ladder
            if len_ladder == "rfc":
                ln = LBASE[sel]
                if len_extra and LEXTRA[sel]:
                    ln += br.bits(LEXTRA[sel])
            else:
                ln = int(len_ladder[1:]) + sel
            # distance code -> canonical dist rank
            dcode, drank = 0, None
            for l in range(1, disth.maxbits + 1):
                dcode = (dcode << 1) | br.bit()
                c = disth.counts[l]
                if c and disth.first[l] <= dcode < disth.first[l] + c:
                    drank = dist_ranks.cum[l] + (dcode - disth.first[l])
                    break
            if drank is None:
                why = f"bad_dist_code@{br.pos}"
                break
            ds = dist_ranks.sym_of_rank[drank]
            d = DBASE[ds]
            if dist_extra and DEXTRA[ds]:
                d += br.bits(DEXTRA[ds])
            if d > len(win):
                why = f"bad_distance@{br.pos}"
                break
            n_match += 1
            for _ in range(min(ln, cap - len(out))):
                b = win[len(win) - d]
                win.append(b)
                out.append(b)
            if len(out) >= cap:
                why = "image_full"
                break
    except EOFError:
        why = f"eof@{br.pos}"
    stats = {"literals": n_lit, "eobs": n_eob, "matches": n_match,
             "emitted": len(out)}
    return bytes(out[:cap]), why, stats




# --------------------------------------------------------------------------
# output transforms (family 2 / §20.4 item 2+3)
# --------------------------------------------------------------------------

def perm4(p):
    """Byte-lane permutation of period 4: out[4k+i] = raw[4k+p[i]]."""
    def f(raw):
        out = bytearray(raw)
        for k in range(len(raw) // 4):
            base = 4 * k
            for i in range(4):
                out[base + i] = raw[base + p[i]]
        return bytes(out)
    return f


def rot8(k):
    """Byte rotation of period 8: out[8i+j] = raw[8i+(j+k)%8]."""
    def f(raw):
        out = bytearray(raw)
        for i in range(len(raw) // 8):
            base = 8 * i
            for j in range(8):
                out[base + j] = raw[base + ((j + k) % 8)]
        return bytes(out)
    return f


def swap16(raw):
    out = bytearray(raw)
    for k in range(len(raw) // 2):
        out[2 * k] = raw[2 * k + 1]
        out[2 * k + 1] = raw[2 * k]
    return bytes(out)


def rowrev(raw, row=1024):
    """Mirror each 1024-byte row (the PNG band row pitch)."""
    out = bytearray(raw)
    for r in range(len(raw) // row):
        base = r * row
        out[base:base + row] = raw[base:base + row][::-1]
    return bytes(out)


def transpose(rows, cols):
    """Column-major write: out[(i % rows) * cols + (i // rows)] = raw[i].

    A strided/planar output write is NOT excluded by the hardware evidence
    (the byte order of the pixel buffer is model-implied, not observed;
    only the byte COUNT is pinned by the completion accounting). The
    transposed head executes the decode's bytes 0, rows, 2*rows, ..."""
    def f(raw):
        out = bytearray(len(raw))
        for i in range(len(raw)):
            j = (i % rows) * cols + (i // rows)
            if j < len(out):
                out[j] = raw[i]
        return bytes(out)
    return f


def transforms():
    """Named output transforms: (name, callable)."""
    t = [("t-ident", lambda b: b), ("t-swap16", swap16),
         ("t-bitrev8", lambda b: bytes(bitrev8(x) for x in b)),
         ("t-nibswap", lambda b: bytes(nibswap(x) for x in b)),
         ("t-rowrev", rowrev),
         ("t-transp78", transpose(78, 1024)),
         ("t-transp1024", transpose(1024, 78))]
    for p in itertools.permutations(range(4)):
        if p == (0, 1, 2, 3):
            continue
        t.append((f"t-perm4{''.join(map(str, p))}", perm4(p)))
    for k in range(1, 8):
        t.append((f"t-rot8-{k}", rot8(k)))
    return t


# --------------------------------------------------------------------------
# main
# --------------------------------------------------------------------------

def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    outdir = "tools/out/rank_transform"
    for i, a in enumerate(sys.argv):
        if a == "--outdir" and i + 1 < len(sys.argv):
            outdir = sys.argv[i + 1]
    flash = open(args[0], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)

    lit_ranks = Ranks(litlens)
    dist_ranks = Ranks(distlens)
    lit_sym_of_rank = lit_ranks.sym_of_rank

    ncoded = len(lit_sym_of_rank)
    T = bytes(s & 0xFF for s in lit_sym_of_rank)  # the uploaded byte table
    print(f"# stream {len(stream)} bytes; litlen coded {ncoded}; "
          f"T[256..258] = {T[256]:02x} {T[257]:02x} {T[258]:02x}")

    os.makedirs(outdir, exist_ok=True)
    cands = []

    def add(name, data):
        b = bytearray(data[:IMG])
        if len(b) < IMG:
            b.extend(bytes(IMG - len(b)))
        cands.append((name, bytes(b)))

    # ---- self-checks: my walker vs the recorded §19.2/§20.3 verdicts ----
    checks = []
    for name, rule, ld, le, de, want in (
            ("m1", "sym", "rfc", True, True, "FATAL +0x50"),
            ("m2", "sym", "rfc", True, False, "FATAL +0x18"),
            ("m4", "sym", "rfc", False, False, "branch +0x18")):
        out, why, st = walk(stream, START_BIT, lith, disth, lit_ranks,
                            dist_ranks, class_rule=rule,
                            len_ladder=ld, len_extra=le, dist_extra=de)
        v = head_verdict(out)
        ok = v.startswith(want)
        checks.append((name, why, v, ok))
        if name == "m1":
            add("s0-baseline-m1", out)
            m1_out = out
    for name, why, v, ok in checks:
        print(f"# self-check {name}: stop={why} verdict={v} "
              f"{'OK' if ok else 'MISMATCH (tool bug!)'}")
    # m1 identity must byte-match fw_syms_family.render (independent render)
    events, _ = record_m1_events(stream, START_BIT, lith, disth)
    ref = render(events, lambda s: s)
    same = ref[:len(m1_out)] == m1_out[:len(ref)]
    print(f"# self-check render-vs-walk: "
          f"{'byte-identical' if same else 'DIVERGES (tool bug!)'}")

    # ---- family 1: the rank-boundary class rule ----
    rank_variants = [
        ("s1-rank-rfc-m1", "rank", "rfc", True, True),
        ("s1-rank-rfc-m2", "rank", "rfc", True, False),
        ("s1-rank-rfc-m3", "rank", "rfc", False, True),
        ("s1-rank-rfc-m4", "rank", "rfc", False, False),
        ("s1-rank-u0-m2", "rank", "u0", False, False),
        ("s1-rank-u1-m2", "rank", "u1", False, False),
        ("s1-rank-u3-m1", "rank", "u3", False, True),
        ("s1-rank-u3-m2", "rank", "u3", False, False),
        ("s1-rank-u4-m2", "rank", "u4", False, False),
        ("s1-rank-u8-m2", "rank", "u8", False, False),
    ]
    print(f"\n{'candidate':<18} {'stop':<12} {'lit':>5} {'eob':>4} {'mat':>5} "
          f"{'emitted':>7}  static head oracle")
    for name, rule, ld, le, de in rank_variants:
        out, why, st = walk(stream, START_BIT, lith, disth, lit_ranks,
                            dist_ranks, class_rule=rule, len_ladder=ld,
                            len_extra=le, dist_extra=de)
        print(f"{name:<18} {why:<12} {st['literals']:>5} {st['eobs']:>4} "
              f"{st['matches']:>5} {st['emitted']:>7}  {head_verdict(out)}")
        add(name, out)

    # ---- family 1b: T-byte boundary class rule (§21) + start-bit sweep ----
    extra_variants = [
        ("s1-t0-rfc-m1", "t0", "rfc", True, True),
        ("s1-t0-rfc-m2", "t0", "rfc", True, False),
        ("s1-t0-u3-m2", "t0", "u3", False, False),
    ]
    # start-bit variants under the rank rule (§19.2's sweep was value-class
    # only; the rank rule changes the token stream, so it does not transfer)
    for sb in (1008, 1072, 1088, 1152, 1216):
        extra_variants.append((f"s1-rank-rfc-m1-sb{sb}", "rank", "rfc",
                               True, True))
        extra_variants.append((f"s1-rank-rfc-m2-sb{sb}", "rank", "rfc",
                               True, False))
    print(f"\n{'candidate':<24} {'stop':<12} {'lit':>5} {'eob':>4} {'mat':>5} "
          f"{'emitted':>7}  static head oracle")
    for name, rule, ld, le, de in extra_variants:
        sb = START_BIT
        if "-sb" in name:
            sb = int(name.rsplit("-sb", 1)[1])
        out, why, st = walk(stream, sb, lith, disth, lit_ranks,
                            dist_ranks, class_rule=rule, len_ladder=ld,
                            len_extra=le, dist_extra=de)
        print(f"{name:<24} {why:<12} {st['literals']:>5} {st['eobs']:>4} "
              f"{st['matches']:>5} {st['emitted']:>7}  {head_verdict(out)}")
        add(name, out)

    # ---- family 2: output transforms over the m1 and m2 bases ----
    bases = {}
    out, _, _ = walk(stream, START_BIT, lith, disth, lit_ranks,
                     dist_ranks, class_rule="sym", len_ladder="rfc",
                     len_extra=True, dist_extra=False)
    bases["m2"] = out
    bases["m1"] = m1_out
    print()
    for bname, raw in bases.items():
        padded = bytearray(raw[:IMG])
        if len(padded) < IMG:
            padded.extend(bytes(IMG - len(padded)))
        padded = bytes(padded)
        for tname, f in transforms():
            if tname == "t-ident":
                continue  # identity == s0-baseline / m2 walk
            add(f"{tname}-{bname}", f(padded))

    # ---- report + write ----
    print(f"\n{'candidate':<24} {'static head oracle':<26} head[0..8]")
    survivors = []
    for name, data in cands:
        v = head_verdict(data)
        keep = v.startswith(("clean", "branch", "jump"))
        if keep:
            survivors.append(name)
        print(f"{name:<24} {v:<26} {data[:8].hex(' ')}"
              + ("  <== SURVIVOR" if keep else ""))
        with open(os.path.join(outdir, f"{name}.bin"), "wb") as fh:
            fh.write(data)
    print(f"\n# wrote {len(cands)} candidates to {outdir}/")
    print(f"# static survivors: {survivors or 'none'}")
    if survivors:
        print("# evaluate: cargo run --release --example entry_oracle "
              "<flash.bin> --budget 65536 "
              + " ".join(f"{outdir}/{s}.bin" for s in survivors))


if __name__ == "__main__":
    main()
