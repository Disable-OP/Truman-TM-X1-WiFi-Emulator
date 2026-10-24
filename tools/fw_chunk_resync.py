#!/usr/bin/env python3
"""SP-container tail framing probe (Milestone W): the chunk-resync dialect.

Implements the three open §16.5 probes as one decisive experiment:

  (a) per-chunk framing motivated by the driver's 128-byte rounding rule
      (PNG_BS_END_ADDR = round128(base + len); PNG_CHUNK_SIZE regs;
      the copied container is consumed by a DMA that moves 128-byte units);
  (b) the dialect meaning of the two mid-stream EOB symbols (the RFC walk
      finds symbol 256 at out 28,260 and 45,137);
  (c) tail alignment to the engine's grid: PNG_BS_OFFSET = 1088 container
      bits = stream bit 1024 — the dynamic header ends at 1008 and the
      body start is aligned/padded up to the 128-bit boundary.

Hypothesis H-chunk: the engine does NOT read the tail as one continuous
bit stream. After the first EOB (bit 106,259, out 28,260) it resyncs to
the next grid position (origin + k*period) and keeps decoding with the
SAME uploaded tables; symbol 256 ends a chunk, not the stream.

Variants swept:

  A eob-grid  : EOB -> jump to the first grid position >= EOB end; codes
                may otherwise span grid lines freely.
  B hard-grid : the decoder also resets at every grid line (a code that
                crosses the line is aborted, overshoot bits discarded).
  C eob-byte  : control — EOB -> align to the next byte boundary.
  D continuous: control — the RFC walk itself (EOB recorded, no jump).

Scoring per variant: fault (kind/bit), output bytes, EOB count, last
consumed bit, tail coverage, head bytes, strict-validity % (capstone
word oracle, same as §15), ASCII runs (>= 6) and known eCos markers.

Usage: python3 tools/fw_chunk_resync.py <flash.bin> [sweep|chain|gap]
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import (Huffman, LBASE, DBASE, LEXTRA, DEXTRA,
                          parse_header, BitReader)
from fw_ring_sweep import Validator

CONTAINER_OFF = 0x1002C          # "SP" magic
STREAM_OFF = CONTAINER_OFF + 8   # deflate stream: byte 0xED = bfinal|btype<<1 = 5
STREAM_LEN = 0x8684              # container u32@4 big-endian
BODY = 1024                      # stream-relative body start (BS_OFFSET 1088 - 64)
IMAGE_BYTES = 79_872             # 78 rows x 1024 (PNG_IMG_WIDTH x HEIGHT)

MARKERS = [b"eCos", b"IntCtrl", b"Zero BSS", b"1ST BOOT", b"LD eCos",
           b"zeroBoot", b"SPHE", b"Sunplus"]


def next_grid_ge(pos, origin, period):
    """First grid position >= pos."""
    if pos <= origin:
        return origin
    k = (pos - origin + period - 1) // period
    return origin + k * period


def walk_chunked(stream, lith, disth, body, origin, period, mode,
                 out_cap=300_000):
    """Chunked decode; returns a result dict."""
    br = BitReader(stream, body)
    out = bytearray()
    eobs = 0
    fault = None
    chunks = []
    stream_bits = len(stream) * 8
    # mode B chunk bookkeeping: the chunk that contains `body`
    chunk_start = origin + ((body - origin) // period) * period \
        if body >= origin else origin
    nsym = 0
    while True:
        if mode == "B":
            chunk_end = chunk_start + period
            if br.pos > chunk_end:
                # a code crossed the grid line: hardware reset discards
                # the overshoot; the next chunk starts at the boundary
                br.pos = chunk_end
            if br.pos >= stream_bits:
                fault = ("EOF", br.pos)
                break
            if br.pos == chunk_end and chunk_end < stream_bits:
                chunk_start = chunk_end
                continue
        else:
            if br.pos >= stream_bits:
                fault = ("EOF", br.pos)
                break
        bit0 = br.pos
        try:
            s = lith.decode(br)
        except EOFError:
            fault = ("EOF", bit0)
            break
        nsym += 1
        if s <= 255:
            out.append(s)
            continue
        if s == 256:
            eobs += 1
            chunks.append((bit0, br.pos, len(out)))
            if len(out) >= out_cap:
                fault = ("OUTCAP", br.pos)
                break
            if mode == "A":
                br.pos = next_grid_ge(br.pos, origin, period)
            elif mode == "B":
                br.pos = chunk_start + period
                chunk_start = br.pos
            elif mode == "C":  # byte align
                br.pos = (br.pos + 7) & ~7
            # mode D: pure continuous — EOB recorded, no jump (RFC walk)
            continue
        li = s - 257
        if li >= len(LEXTRA):
            fault = (f"lensym{s}", bit0)
            break
        eb = br.bits(LEXTRA[li]) if LEXTRA[li] else 0
        ds = disth.decode(br)
        if ds >= len(DBASE):
            fault = (f"distsym{ds}", bit0)
            break
        deb = br.bits(DEXTRA[ds]) if DEXTRA[ds] else 0
        ln = LBASE[li] + eb
        d = DBASE[ds] + deb
        n = len(out)
        for i in range(ln):
            out.append(out[n - d + i] if d <= n else 0)
        if len(out) >= out_cap:
            fault = ("OUTCAP", br.pos)
            break
    return dict(out=bytes(out), eobs=eobs, fault=fault, last=br.pos,
                chunks=chunks, syms=nsym)


def ascii_runs(out, minlen=6):
    runs, cur = 0, 0
    for b in out:
        if 32 <= b < 127:
            cur += 1
        else:
            if cur >= minlen:
                runs += 1
            cur = 0
    if cur >= minlen:
        runs += 1
    return runs


def found_markers(out):
    return [m.decode() for m in MARKERS if m in out]


def score(res, val, stream_bits):
    out = res["out"]
    validity = None
    if val is not None and len(out) >= 16:
        words = [out[i:i + 4] for i in range(0, len(out) - 3, 4)]
        validity = 100.0 * sum(val.word_valid(w) for w in words) / len(words)
    return dict(
        out=len(out), eobs=res["eobs"], fault=res["fault"],
        last=res["last"], cover=round(100.0 * res["last"] / stream_bits, 1),
        head=out[:8].hex(), valid=round(validity, 1) if validity else None,
        ascii=ascii_runs(out), marks=found_markers(out),
    )


def baseline(stream, lith, disth):
    """Continuous RFC walks from both candidate body starts."""
    stream_bits = len(stream) * 8
    rows = []
    for body in (1008, 1024):
        for mode in ("C", "D"):
            r = walk_chunked(stream, lith, disth, body, 0, 1 << 30, mode)
            eob_bits = [c for c in r["chunks"]]
            rows.append((body, mode, r, eob_bits[:3]))
    return rows


def main():
    import argparse
    from pathlib import Path
    ap = argparse.ArgumentParser()
    ap.add_argument("flash")
    ap.add_argument("cmd", nargs="?", default="sweep",
                    choices=["sweep", "chain", "gap"])
    args = ap.parse_args()
    flash = Path(args.flash).read_bytes()
    stream = flash[STREAM_OFF:STREAM_OFF + STREAM_LEN]
    stream_bits = len(stream) * 8
    bfinal, litlens, distlens, hdr_end = parse_header(stream)
    print(f"stream: flash 0x{STREAM_OFF:x} len {len(stream)} = "
          f"{stream_bits} bits; bfinal={bfinal}; header ends at bit "
          f"{hdr_end} (body padding {1024 - hdr_end} bits)")
    lith, disth = Huffman(litlens), Huffman(distlens)

    if args.cmd == "gap":
        bits = [(stream[i >> 3] >> (i & 7)) & 1 for i in range(1008, 1024)]
        val = 0
        for i, b in enumerate(bits):
            val |= b << i
        print(f"body gap bits 1008..1023 (LSB-first u16) = 0x{val:04x} "
              f"raw {[hex(b) for b in bits]}")
        return

    # ---- baseline: continuous walks (must reproduce §16.1/§16.3) ----
    print("\n== baseline walks ==")
    for body, mode, r, eobs in baseline(stream, lith, disth):
        sc = score(r, None, stream_bits)
        print(f" body={body} mode={mode}: out={sc['out']} eobs={sc['eobs']} "
              f"fault={sc['fault']} last={sc['last']}/{stream_bits} "
              f"head={sc['head']} first_eobs={[(c[0], c[1], c[2]) for c in eobs]}")

    if args.cmd == "chain":
        val = Validator(flash)
        r = walk_chunked(stream, lith, disth, BODY, 0, 1024, "A")
        sc = score(r, val, stream_bits)
        print(f"\n== chain mode A origin=0 period=1024 ==\n{sc}")
        prev = 0
        for i, (b0, b1, o1) in enumerate(r["chunks"][:20]):
            print(f"  chunk {i}: bits {b0}..{b1} -> out {prev}..{o1}")
            prev = o1
        print(f"  ... {len(r['chunks'])} EOB chunks total")
        return

    # ---- sweep ----
    print("\n== chunk-resync sweep ==")
    val = Validator(flash)
    results = []
    for mode in ("A", "B", "C", "D"):
        for origin in (0, 64, 512, 1008):
            for period in (512, 1024, 2048, 4096):
                if mode in ("C", "D") and (origin, period) != (0, 1024):
                    continue  # controls: no grid params
                r = walk_chunked(stream, lith, disth, BODY, origin, period, mode)
                sc = score(r, val, stream_bits)
                sc.update(mode=mode, origin=origin, period=period)
                results.append(sc)
    results.sort(key=lambda s: (s["fault"] is not None,
                                -(s["out"] or 0)))
    hdr = ("mode orgn period  out   eobs fault          last/cover%   "
           "valid% asc head")
    print(hdr)
    for s in results:
        print(f"{s['mode']}    {s['origin']:4d} {s['period']:5d} "
              f"{s['out']:6d} {s['eobs']:5d} "
              f"{str(s['fault']):13.13s} {s['last']:8d}/{s['cover']:5.1f} "
              f"{str(s['valid']):>6s} {s['ascii']:3d} {s['head']}")

    ok = [s for s in results
          if s["fault"] is None
          and abs(s["out"] - IMAGE_BYTES) <= 1024
          and s["valid"] and s["valid"] >= 90.0]
    print("\nverdict:", "SUCCESS" if ok else "no variant meets the "
          "success criteria (fault-free, out within 1024 of 79872, "
          "validity >= 90%)")
    for s in ok:
        print("  survivor:", s)


if __name__ == "__main__":
    main()
