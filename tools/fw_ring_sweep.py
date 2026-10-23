#!/usr/bin/env python3
"""Engine LZ window-size sweep (Milestone L experiment 1).

Hypothesis family never tested in §14.3 (which refuted *absolute* ring
indexing and table remaps, not the ring *size*): the engine's internal LZ
ring is smaller than the 32 KiB the driver zero-fills. Matches whose RFC
distance exceeds the true ring size W alias to ring[(write - dist) mod W]
— real earlier bytes (locally plausible code/data), exactly the observed
"plausible-but-wrong" corruption signature.

For each candidate W the tool decodes the SP container with a W-byte
pre-zeroed ring (relative live-window copies, literals unchanged) and
scores the output with the calibrated strict MIPS-validity oracle
(§14.2): capstone MIPS32r2 LE word decode against the common-mnemonic
set built from the flash's own known-good code (stage-1 + stage-2).

Usage: python3 tools/fw_ring_sweep.py <flash.bin> [--W 1024,...] [--detail W]
"""
import math
import struct
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import (Huffman, LBASE, DBASE, parse_header, symbol_walk)

STAGE1 = (0x00000, 0x08000)
STAGE2 = (0x08000, 0x10000)
SATDB = (0x3AE1D4, 0x3BFFFF)
CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C


def load_events(flash):
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    assert cont[:2] == b"SP", cont[:2]
    stream = cont[8:]
    bfinal, litlens, distlens, header_bits = parse_header(stream)
    lith = Huffman(litlens)
    disth = Huffman(distlens)
    return symbol_walk(stream, 1024, lith, disth)


def ring_decode(events, W, preset=0):
    """Decode with a W-byte ring (pre-filled with `preset`), relative
    live-window copies. Returns the linear output byte string."""
    ring = bytearray([preset]) * W
    rp = 0  # next write position in the ring
    out = bytearray()
    for e in events:
        k = e["kind"]
        if k == "LIT":
            b = e["sym"]
            ring[rp] = b
            rp = (rp + 1) % W
            out.append(b)
        elif k == "MATCH":
            li = e["sym"] - 257
            ln = LBASE[li] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            for _ in range(ln):
                b = ring[(rp - d) % W]
                ring[rp] = b
                rp = (rp + 1) % W
                out.append(b)
        elif k.startswith("ERR") or k == "EOF":
            break
    return bytes(out)


def rfc_decode(events, preset_len=0x8000, preset=0):
    """Baseline RFC 1951 decode with a preset_len zero window."""
    out = bytearray()
    window = bytearray([preset]) * preset_len
    for e in events:
        k = e["kind"]
        if k == "LIT":
            window.append(e["sym"])
            out.append(e["sym"])
        elif k == "MATCH":
            li = e["sym"] - 257
            ln = LBASE[li] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            n = len(window)
            if d > n:
                break
            for i in range(ln):
                b = window[n - d + i]
                window.append(b)
                out.append(b)
        elif k.startswith("ERR") or k == "EOF":
            break
    return bytes(out)


class Validator:
    """Strict validity oracle: capstone word decode against the common
    mnemonic set of the flash's known-good code regions."""

    def __init__(self, flash):
        import capstone
        self.capstone = capstone
        self.md = capstone.Cs(capstone.CS_ARCH_MIPS,
                              capstone.CS_MODE_MIPS32 |
                              capstone.CS_MODE_LITTLE_ENDIAN)
        hist = {}
        for a, b in (STAGE1, STAGE2):
            for off in range(a, b, 4):
                word = flash[off:off + 4]
                mnem = self._one(word)
                if mnem:
                    hist[mnem] = hist.get(mnem, 0) + 1
        # common = mnemonics covering the code-like vocabulary
        total = sum(hist.values())
        self.common = {m for m, c in hist.items()
                       if c >= 8 and c / total > 0.0005}
        self.cache = {}

    def _one(self, word):
        for ins in self.md.disasm(word, 0):
            return ins.mnemonic
        return None

    def word_valid(self, word):
        c = self.cache.get(word)
        if c is None:
            m = self._one(word)
            c = m in self.common if m else False
            self.cache[word] = c
        return c

    def strict(self, buf, base=0, size=None):
        if size is None:
            size = len(buf) - base
        n = size // 4
        ok = 0
        for i in range(n):
            word = bytes(buf[base + i * 4: base + i * 4 + 4])
            ok += self.word_valid(word)
        return ok / max(n, 1), n

    def lenient(self, buf):
        n = len(buf) // 4
        ok = 0
        for i in range(n):
            word = bytes(buf[i * 4:i * 4 + 4])
            if self._one(word):
                ok += 1
        return ok / max(n, 1)


def byte_stats(buf):
    if not buf:
        return 0.0, 0.0
    zeros = buf.count(0) / len(buf)
    freq = [0] * 256
    for b in buf:
        freq[b] += 1
    ent = -sum((c / len(buf)) * math.log2(c / len(buf))
               for c in freq if c)
    return zeros, ent


def main():
    flash = open(sys.argv[1], "rb").read()
    val = Validator(flash)
    print(f"common mnemonic set: {len(val.common)} of flash code vocabulary")
    cal = []
    for name, (a, b) in (("stage-1 code", STAGE1), ("stage-2 code", STAGE2),
                         ("satellite DB", SATDB)):
        s, n = val.strict(flash, a, b - a)
        cal.append((name, s, n))
        print(f"calib {name:14s}: strict={s * 100:5.1f}%  words={n}")
    rnd = struct.pack("<%dI" % 4096, *struct.unpack_from("<4096I",
                      bytes((i * 2654435761) & 0xFF for i in range(16384))))
    s, _ = val.strict(rnd)
    print(f"calib {'prng bytes':14s}: strict={s * 100:5.1f}%")

    events = load_events(flash)
    print(f"\nevents: {len(events)}; decoding variants...")

    ref = rfc_decode(events, 0x8000)
    s, _ = val.strict(ref)
    z, ent = byte_stats(ref)
    print(f"RFC 32768+zero-preset : len={len(ref):6d} strict={s * 100:5.1f}% "
          f"zeros={z * 100:4.1f}% entropy={ent:.2f}")

    Ws = [512, 1024, 2048, 4096, 8192, 16384, 24576, 32768]
    results = []
    for W in Ws:
        out = ring_decode(events, W)
        s, _ = val.strict(out)
        z, ent = byte_stats(out)
        results.append((W, len(out), s, z, ent))
        print(f"ring W={W:6d}          : len={len(out):6d} "
              f"strict={s * 100:5.1f}% zeros={z * 100:4.1f}% entropy={ent:.2f}")

    best = max(results, key=lambda r: r[2])
    print(f"\nbest W by strict validity: {best[0]} ({best[2] * 100:.1f}%)")
    if len(sys.argv) > 3 and sys.argv[2] == "--detail":
        W = int(sys.argv[3])
        out = ring_decode(events, W)
        n = min(len(out), len(ref))
        div = next((i for i in range(n) if out[i] != ref[i]), None)
        print(f"W={W}: first divergence vs RFC at {div}")
        # per-1KiB-chunk strict validity map
        print("chunk strict-validity map (1 KiB chunks):")
        for c in range(0, min(len(out), 32 * 1024), 1024):
            s, _ = val.strict(out[c:c + 1024])
            bar = "#" * int(s * 40)
            print(f"  chunk {c // 1024:3d} @0x{c:06x}: {s * 100:5.1f}% {bar}")


if __name__ == "__main__":
    main()
