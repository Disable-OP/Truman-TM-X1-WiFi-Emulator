#!/usr/bin/env python3
"""SP-container DEFLATE symbol walk + engine-output diff (Milestone J analysis).

Parses the "SP" container in the flash dump, decodes the dynamic Huffman
header exactly as the firmware's driver does, then produces the full symbol
sequence and compares two output realizations:

  * reference RFC 1951 decode (standard LBASE/LEXTRA/DBASE/DEXTRA tables)
  * the byte image the hardware-engine model produced (engine_out.bin)

The goal is to pin the engine's length/distance value semantics from the
divergence (docs/firmware-analysis.md §14).

Usage:
  python3 tools/fw_engine_lz.py <flash.bin> <engine_out.bin> [max_events]
"""
import struct
import sys

LBASE = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51,
         59, 67, 83, 99, 115, 131, 163, 195, 227, 258]
LEXTRA = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4,
          4, 4, 5, 5, 5, 5, 0]
DBASE = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385,
         513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577]
DEXTRA = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9,
          10, 10, 11, 11, 12, 12, 13, 13]
CL_ORDER = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15]


class BitReader:
    __slots__ = ("data", "pos")

    def __init__(self, data, pos=0):
        self.data = data
        self.pos = pos

    def bit(self):
        if (self.pos >> 3) >= len(self.data):
            raise EOFError(f"out of input at bit {self.pos}")
        b = (self.data[self.pos >> 3] >> (self.pos & 7)) & 1
        self.pos += 1
        return b

    def bits(self, n):
        v = 0
        for i in range(n):
            v |= self.bit() << i
        return v


class Huffman:
    """Canonical Huffman with a fast table-driven decoder (symbol-ordered)."""

    def __init__(self, lengths):
        self.lengths = lengths
        self.maxbits = max(lengths) if lengths else 0
        counts = [0] * (self.maxbits + 1)
        for l in lengths:
            if l:
                counts[l] += 1
        self.counts = counts
        code = 0
        self.first = [0] * (self.maxbits + 1)
        for b in range(1, self.maxbits + 1):
            code = (code + counts[b - 1]) << 1
            self.first[b] = code
        # per-length symbol list, in code order
        self.by_len = {b: [] for b in range(1, self.maxbits + 1) if counts[b]}
        for s, l in enumerate(lengths):
            if l:
                self.by_len[l].append(s)

    def decode(self, br: BitReader):
        code = 0
        for l in range(1, self.maxbits + 1):
            code = (code << 1) | br.bit()
            c = self.counts[l] if l <= self.maxbits else 0
            if c and self.first[l] <= code < self.first[l] + c:
                return self.by_len[l][code - self.first[l]]
        raise ValueError(f"bad code at bit {br.pos}")


def parse_header(stream):
    br = BitReader(stream)
    bfinal = br.bit()
    btype = br.bits(2)
    if btype != 2:
        raise ValueError(f"btype={btype}")
    hlit = br.bits(5) + 257
    hdist = br.bits(5) + 1
    hclen = br.bits(4) + 4
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
            lens += [lens[-1]] * (br.bits(2) + 3)
        elif s == 17:
            lens += [0] * (br.bits(3) + 3)
        else:
            lens += [0] * (br.bits(7) + 11)
    return bfinal, lens[:hlit], lens[hlit:], br.pos


def symbol_walk(stream, start_bit, lith, disth):
    """Events: dict(kind, sym, dsym, eb, deb, bit, out) — out = RFC1951 pos."""
    br = BitReader(stream, start_bit)
    events = []
    outpos = 0
    while True:
        bit = br.pos
        if (bit >> 3) >= len(stream):
            break
        try:
            s = lith.decode(br)
        except EOFError:
            events.append(dict(kind="EOF", sym=None, dsym=None, eb=None,
                               deb=None, bit=bit, out=outpos))
            break
        except ValueError:
            events.append(dict(kind="ERR_LIT", sym=None, dsym=None, eb=None,
                               deb=None, bit=bit, out=outpos))
            break
        if s <= 255:
            events.append(dict(kind="LIT", sym=s, dsym=None, eb=None, deb=None,
                               bit=bit, out=outpos))
            outpos += 1
        elif s == 256:
            events.append(dict(kind="EOB", sym=256, dsym=None, eb=None, deb=None,
                               bit=bit, out=outpos))
        else:
            li = s - 257
            if li >= len(LEXTRA):
                events.append(dict(kind="ERR_LEN", sym=s, dsym=None, eb=None,
                                   deb=None, bit=bit, out=outpos))
                break
            try:
                eb = br.bits(LEXTRA[li]) if LEXTRA[li] else 0
            except EOFError:
                events.append(dict(kind="EOF", sym=s, dsym=None, eb=None,
                                   deb=None, bit=bit, out=outpos))
                break
            try:
                ds = disth.decode(br)
            except EOFError:
                events.append(dict(kind="EOF", sym=s, dsym=None, eb=eb,
                                   deb=None, bit=bit, out=outpos))
                break
            except ValueError:
                events.append(dict(kind="ERR_DIST", sym=s, dsym=None, eb=eb,
                                   deb=None, bit=bit, out=outpos))
                break
            try:
                deb = br.bits(DEXTRA[ds]) if DEXTRA[ds] else 0
            except EOFError:
                events.append(dict(kind="EOF", sym=s, dsym=ds, eb=eb,
                                   deb=None, bit=bit, out=outpos))
                break
            events.append(dict(kind="MATCH", sym=s, dsym=ds, eb=eb, deb=deb,
                               bit=bit, out=outpos))
            outpos += LBASE[li] + eb
    return events


def rfc_decode(events, preset_len):
    """Reference decode: preset zeros window + standard tables."""
    out = bytearray()
    window = bytearray(preset_len)  # logical window = preset + out
    for e in events:
        if e["kind"] == "LIT":
            window.append(e["sym"])
            out.append(e["sym"])
        elif e["kind"] == "MATCH":
            li = e["sym"] - 257
            ln = LBASE[li] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            n = len(window)
            if d > n:
                e["bad_dist"] = True
                break
            for i in range(ln):
                b = window[n - d + i]
                window.append(b)
                out.append(b)
        elif e["kind"].startswith("ERR"):
            e["decode_stop"] = True
            break
    return out


def main():
    flash = open(sys.argv[1], "rb").read()
    eng = open(sys.argv[2], "rb").read()
    cont = flash[0x1002C:0x1002C + 0x868C]
    stream = cont[8:]
    assert cont[:2] == b"SP"
    nchunks = struct.unpack_from(">H", cont, 2)[0]
    slen = struct.unpack_from(">I", cont, 4)[0]
    print(f"container: chunks={nchunks} stream_len=0x{slen:x} "
          f"expected_out={nchunks * 1024} engine_wrote={len(eng)}")

    bfinal, litlens, distlens, header_bits = parse_header(stream)
    print(f"header: bfinal={bfinal} hlit={len(litlens)} hdist={len(distlens)} "
          f"header_bits={header_bits}")
    pad_ok = all((stream[b >> 3] >> (b & 7)) & 1 == 0
                 for b in range(header_bits, 1024))
    print(f"padding {header_bits}..1024 all zero: {pad_ok}")

    lith = Huffman(litlens)
    disth = Huffman(distlens)
    events = symbol_walk(stream, 1024, lith, disth)
    kinds = {}
    for e in events:
        kinds[e["kind"]] = kinds.get(e["kind"], 0) + 1
    print(f"events: {kinds} last_bit={events[-1]['bit']}")
    ref = rfc_decode(events, 0x8000)
    print(f"RFC1951 reference output: {len(ref)} bytes")

    # First divergence between reference and engine output.
    div = None
    for i in range(min(len(ref), len(eng))):
        if ref[i] != eng[i]:
            div = i
            break
    print(f"first divergence at out offset {div}")
    if div is None:
        print("outputs identical up to min length")
        return

    # Map divergence to the symbol that produced it.
    owner = None
    for k, e in enumerate(events):
        if e["out"] <= div and (k + 1 == len(events) or events[k + 1]["out"] > div):
            if e["kind"] == "LIT" and e["out"] == div:
                owner = (k, e)
            elif e["kind"] == "MATCH":
                owner = (k, e)
    if owner:
        k, e = owner
        print(f"producing event #{k}: {e}")
        if e["kind"] == "MATCH":
            li = e["sym"] - 257
            print(f"  RFC: len={LBASE[li] + e['eb']} dist={DBASE[e['dsym']] + e['deb']}")
        ctx = ref[max(0, div - 16):div + 16]
        print(f"  ref[{div-16}:{div+16}]: {ctx.hex(' ')}")
        ctx = eng[max(0, div - 16):div + 16]
        print(f"  eng[{div-16}:{div+16}]: {ctx.hex(' ')}")

    # Per-match agreement: length of the common prefix between ref and eng
    # around each match (how far until the next divergence).
    print("\n-- match-by-match divergence map (first 40) --")
    mi = 0
    last_agree = 0
    for k, e in enumerate(events):
        if e["kind"] != "MATCH":
            continue
        mi += 1
        if mi > 40:
            break
        o = e["out"]
        # how far do ref and eng agree starting at last_agree?
        a = last_agree
        while a < min(len(ref), len(eng)) and ref[a] == eng[a]:
            a += 1
        li = e["sym"] - 257
        print(f"match#{mi} out={o} bit={e['bit']} len={LBASE[li] + e['eb']} "
              f"dist={DBASE[e['dsym']] + e['deb']} agree_until={a}")
        last_agree = o
    # overall agreement
    a = 0
    while a < min(len(ref), len(eng)) and ref[a] == eng[a]:
        a += 1
    print(f"\ntotal agreement prefix: {a} bytes")


if __name__ == "__main__":
    main()
