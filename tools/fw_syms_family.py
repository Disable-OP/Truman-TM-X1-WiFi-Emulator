#!/usr/bin/env python3
"""Symbol-mapping family generator + dynamic-oracle candidate sets (Milestone R).

Milestone Q left two hypothesis classes for the executable-head paradox
(docs/firmware-analysis.md §19.3/§19.4): the engine's symbol->value mapping
or an unmodeled output transform, both parse-preserving. This tool generates
the candidate decodes for the FIRST class and hands them to the Rust entry
harness (examples/entry_oracle.rs) — the register-aware dynamic oracle.

Structure pinned by tools/fw_syms_probe.py (§20.2): the driver uploads the
litlen symbol table as canonical-rank-ordered BYTES; length symbols 276/285
land at ranks 256/257 as (s-256), literals continue at ranks 258+. The parse
(code->rank, first[]/cum[], extra-bit positions) is verified exact, so the
value-side space is what each literal rank EMITS.

Families generated:
  V1 class-preserving (parse bit-identical to RFC m1; only literal bytes
     change through a map f):
     - identity (the RFC baseline, = the engine-model output)
     - xor k / add k for k = 1..255 (table-RAM scramble sweeps)
     - bitrev8, nibswap (byte-lane conventions)
     - lut-by-value: f(s) = T[s] (engine misreads the rank-ordered table as
       a symbol-value-indexed LUT)
     - rank-shift d in {-2,-1,+1,+2}: f(s) = T[(rank(s)+d) mod ncoded]
       (table-RAM base misalignment)
  V2 structure variants (one representative each, dedicated walks):
     - len-group-rev: symbols reversed within each code-length group
       (per-length symbol permutation, §19.4's family name-sake; permutes
       literal ranks with length ranks -> token classes change)
     - m2 / m4 accountings (§17.5; dynamic confirmation of the §19.2
       static refutations)

Outputs: <outdir>/<name>.bin (raw decode, zero-padded to the 79,872-byte
image) + a summary table with the static head-oracle prefilter.

Usage: python3 tools/fw_syms_family.py <flash.bin> [--outdir DIR] [--sweep]
"""
import os
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import Huffman, BitReader, parse_header
from fw_first_corruption import word_class, head_words

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
IMG = 79_872           # 1024 x 78 (PNG_IMG_WIDTH x PNG_IMG_HEIGHT)
WINDOW = 0x8000        # the driver's zero-filled 32 KiB band (§13.1)
START_BIT = 1024       # stream-relative symbol start (R3818 - 64, §13.4)

LBASE = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51,
         59, 67, 83, 99, 115, 131, 163, 195, 227, 258]
LEXTRA = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4,
          4, 4, 5, 5, 5, 5, 0]
DBASE = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385,
         513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385,
         24577]
DEXTRA = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9,
          10, 10, 11, 11, 12, 12, 13, 13]


def canonical_tables(litlens):
    """Canonical rank machinery: sym per rank, first codes, counts.

    cum_excl[l] = number of coded symbols with length < l (zero-length
    symbols are outside the rank space entirely).
    """
    h = Huffman(litlens)
    cum = [0] * (h.maxbits + 2)
    for b in range(2, h.maxbits + 2):
        cum[b] = cum[b - 1] + h.counts[b - 1]
    sym_of_rank = [None] * cum[h.maxbits + 1]
    for l in sorted(h.by_len):
        for k, s in enumerate(h.by_len[l]):
            sym_of_rank[cum[l] + k] = s
    return h, sym_of_rank


def walk_ranks(stream, start_bit, lith, disth, lit_class, len_extra=True,
               dist_extra=True, cap=IMG + 64):
    """One bit-level walk; token classes/values come from lit_class(rank).

    lit_class(rank) -> (kind, value): kind in "L" (literal byte), "E" (EOB),
    "M" (RFC length symbol value 257..285). Bit consumption = RFC m1 unless
    len_extra/dist_extra are disabled (m2/m4 accountings, §17.5).
    """
    br = BitReader(stream, start_bit)
    win = bytearray(WINDOW)
    out = bytearray()
    ranks = []
    why = "input_end"
    try:
        while (br.pos >> 3) < len(stream):
            # litlen code -> canonical rank
            code = 0
            rank = None
            for l in range(1, lith.maxbits + 1):
                code = (code << 1) | br.bit()
                c = lith.counts[l]
                if c and lith.first[l] <= code < lith.first[l] + c:
                    rank = cum_excl[l] + (code - lith.first[l])
                    break
            if rank is None:
                why = f"bad_code@{br.pos}"
                break
            kind, val = lit_class(rank)
            ranks.append(rank)
            if kind == "L":
                win.append(val)
                out.append(val)
                if len(out) >= cap:
                    why = "image_full"
                    break
                continue
            if kind == "E":
                continue
            # match: val = RFC length symbol 257..285
            li = val - 257
            ln = LBASE[li]
            if len_extra and LEXTRA[li]:
                ln += br.bits(LEXTRA[li])
            # distance code -> canonical dist rank
            dcode = 0
            drank = None
            for l in range(1, disth.maxbits + 1):
                dcode = (dcode << 1) | br.bit()
                c = disth.counts[l]
                if c and disth.first[l] <= dcode < disth.first[l] + c:
                    drank = dist_cum_excl[l] + (dcode - disth.first[l])
                    break
            if drank is None:
                why = f"bad_dist_code@{br.pos}"
                break
            ds = dist_sym_of_rank[drank]
            d = DBASE[ds]
            if dist_extra and DEXTRA[ds]:
                d += br.bits(DEXTRA[ds])
            if d > len(win):
                why = f"bad_distance@{br.pos}"
                break
            for _ in range(min(ln, cap - len(out))):
                b = win[len(win) - d]
                win.append(b)
                out.append(b)
            if len(out) >= cap:
                why = "image_full"
                break
    except EOFError:
        why = f"eof@{br.pos}"
    return bytes(out[:cap]), ranks, why


def render(events, f, cap=IMG):
    """Render recorded m1 events with literal map f (live-window copy).

    f: callable byte value -> byte value (the hypothesised engine table
    interpretation applied to literal symbols).
    """
    win = bytearray(WINDOW)
    out = bytearray()
    for ev in events:
        k = ev[0]
        if k == "L":
            b = f(ev[1]) & 0xFF
            win.append(b)
            out.append(b)
            if len(out) >= cap:
                break
        elif k == "M":
            ln, d = ev[1], ev[2]
            for _ in range(min(ln, cap - len(out))):
                b = win[len(win) - d]
                win.append(b)
                out.append(b)
            if len(out) >= cap:
                break
        # "E" (EOB): marker, keep walking (§13.4)
    return bytes(out[:cap])


def record_m1_events(stream, start_bit, lith, disth, cap=IMG + 64):
    """Record the canonical m1 event list once: ("L", sym) / ("M", ln, d) / "E"."""
    br = BitReader(stream, start_bit)
    events = []
    out_len = 0
    why = "input_end"
    while (br.pos >> 3) < len(stream):
        try:
            s = lith.decode(br)
        except (ValueError, EOFError):
            why = f"bad_code@{br.pos}"
            break
        if s <= 255:
            events.append(("L", s))
            out_len += 1
        elif s == 256:
            events.append(("E",))
            continue
        else:
            li = s - 257
            ln = LBASE[li]
            if LEXTRA[li]:
                ln += br.bits(LEXTRA[li])
            try:
                ds = disth.decode(br)
            except (ValueError, EOFError):
                why = f"bad_dist_code@{br.pos}"
                break
            d = DBASE[ds]
            if DEXTRA[ds]:
                d += br.bits(DEXTRA[ds])
            if d > WINDOW + out_len:
                why = f"bad_distance@{br.pos}"
                break
            events.append(("M", ln, d))
            out_len += ln
        if out_len >= cap:
            why = "image_full"
            break
    return events, why


def head_verdict(out):
    """Static executable-head oracle: first fatal word in the first 0x100."""
    for off, w, cls in head_words(out, 0x100):
        if cls.startswith("RESERVED") or cls == "trap":
            return f"FATAL +0x{off:x} ({cls})"
        if cls == "jump":
            return f"jump +0x{off:x}"
        if cls == "branch":
            return f"branch +0x{off:x}"
    return "clean-0x100"


def bitrev8(b):
    return int(f"{b:08b}"[::-1], 2)


def nibswap(b):
    return ((b & 0x0F) << 4) | (b >> 4)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    opts = set(a for a in sys.argv[1:] if a.startswith("--"))
    outdir = "tools/out/syms_family"
    for i, a in enumerate(sys.argv):
        if a == "--outdir" and i + 1 < len(sys.argv):
            outdir = sys.argv[i + 1]
    flash = open(args[0], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)

    global cum_excl, dist_cum_excl, dist_sym_of_rank
    _, sym_of_rank = canonical_tables(litlens)
    ncoded = len(sym_of_rank)
    cum_excl = [0] * (lith.maxbits + 2)
    for b in range(2, lith.maxbits + 2):
        cum_excl[b] = cum_excl[b - 1] + lith.counts[b - 1]
    _, dist_sym_of_rank = canonical_tables(distlens)
    dist_cum_excl = [0] * (disth.maxbits + 2)
    for b in range(2, disth.maxbits + 2):
        dist_cum_excl[b] = dist_cum_excl[b - 1] + disth.counts[b - 1]
    rank_of_sym = {s: r for r, s in enumerate(sym_of_rank)}
    canon_T = bytes(s & 0xFF for s in sym_of_rank)  # the uploaded byte table

    os.makedirs(outdir, exist_ok=True)
    cands = []  # (name, bytes)

    # ---- V1: class-preserving literal-value maps (one m1 parse) ----
    events, why = record_m1_events(stream, START_BIT, lith, disth)
    print(f"# m1 parse: {len(events)} events, stop={why}")

    def add(name, data):
        b = bytearray(data[:IMG])
        if len(b) < IMG:
            b.extend(bytes(IMG - len(b)))
        cands.append((name, bytes(b)))

    add("v1-identity", render(events, lambda s: s))
    add("v1-bitrev8", render(events, bitrev8))
    add("v1-nibswap", render(events, nibswap))
    add("v1-lut-by-value", render(events, lambda s: canon_T[s]))
    for d in (-2, -1, 1, 2):
        tbl = {s: canon_T[(rank_of_sym[s] + d) % ncoded]
               for s in range(256)}
        add(f"v1-rankshift{d:+d}", render(events, lambda s, t=tbl: t[s]))
    if "--sweep" in opts:
        for k in range(1, 256):
            add(f"v1-xor{k:02x}", render(events, lambda s, k=k: s ^ k))
        for k in range(1, 256):
            add(f"v1-add{k:02x}", render(events, lambda s, k=k: (s + k) & 0xFF))
    else:
        for k in (0x20, 0x60, 0x80, 0xA0, 0xFF):
            add(f"v1-xor{k:02x}", render(events, lambda s, k=k: s ^ k))

    # ---- V2: structure variants (dedicated walks) ----
    ident = lambda r: (("L", sym_of_rank[r]) if sym_of_rank[r] <= 255
                       else ("E", 0) if sym_of_rank[r] == 256
                       else ("M", sym_of_rank[r]))
    out, _, w = walk_ranks(stream, START_BIT, lith, disth, ident)
    add("v2-m1-recheck", out)

    # per-length reversal: symbols reversed within each code-length group
    perm = list(sym_of_rank)
    for l in sorted(lith.by_len):
        grp = [cum_excl[l] + k for k in range(len(lith.by_len[l]))]
        for k, r in enumerate(grp):
            perm[r] = sym_of_rank[grp[-1 - k]]
    perm_class = lambda r: (("L", perm[r] & 0xFF) if perm[r] <= 255
                            else ("E", 0) if perm[r] == 256
                            else ("M", perm[r]))
    out, _, w = walk_ranks(stream, START_BIT, lith, disth, perm_class)
    add("v2-lengroup-rev", out)

    # m2 / m4 accountings (identity maps; §17.5, §19.2)
    for name, le, de in (("v2-m2-lenonly", True, False),
                         ("v2-m4-neither", False, False)):
        out, _, w = walk_ranks(stream, START_BIT, lith, disth, ident,
                               len_extra=le, dist_extra=de)
        add(name, out)

    # ---- report + write ----
    print(f"{'candidate':<22} {'bytes':>6}  {'static head oracle':<26} head[0..8]")
    for name, data in cands:
        print(f"{name:<22} {len(data):>6}  {head_verdict(data):<26} {data[:8].hex(' ')}")
        with open(os.path.join(outdir, f"{name}.bin"), "wb") as fh:
            fh.write(data)
    base = cands[0][1]
    recheck = next((d for n, d in cands if n == "v2-m1-recheck"), None)
    match = "byte-identical" if recheck == base else "DIVERGES (tool bug!)"
    diverge = next((i for i in range(IMG) if base[i] != cands[1][1][i]), None)
    print(f"\n# wrote {len(cands)} candidates to {outdir}/")
    print(f"# v2-m1-recheck vs v1-identity: {match}")
    print(f"# first divergence identity vs {cands[1][0]}: byte {diverge}")
    print(f"# evaluate: cargo run --release --example entry_oracle "
          f"<flash.bin> --budget 65536 {outdir}/*.bin")


if __name__ == "__main__":
    main()
