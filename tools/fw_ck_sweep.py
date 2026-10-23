#!/usr/bin/env python3
"""Checksum-algorithm sweep for the two known anchors (Q8).

Training pair:
  data = flash[0x2F6C : 0x2F6C+0x3ADC]   (driver payload, plaintext)
  expected = 0xB56FB05D (descriptor word @ flash 0x1224)
Confirmation pair:
  data = flash[0x1002C : 0x1002C+0x868C] (SP container)
  expected = 0xB5EBF9FF (desc[6] @ 0x10028)  [coverage hypothesis]

Sweeps CRC-32 catalog (poly x init x refin/refout x xorout), CRC-16
families, Adler/Fletcher/FNV/DJB2/sdbm/JS/one-at-a-time, sums, rotated
accumulators, weighted sums.
"""
import struct
import sys

P1_DATA = (0x2F6C, 0x3ADC)
P1_EXP = 0xB56FB05D
P2_DATA = (0x1002C, 0x868C)
P2_EXP = 0xB5EBF9FF

CRC32_POLYS = [
    0x04C11DB7,  # ISO-HDLC / BZIP2 / JAMCRC
    0x1EDC6F41,  # Castagnoli
    0x741B8CD7,  # Koopman
    0x814141AB,  # CRC-32Q
    0xF4ACFB13,  # AUTOSAR
    0xA833982B,  # BASE91-D
    0x000000AF,  # XFER
    0x8001801B,  # CD-ROM ED
    0x8F6E37A0,  # Koopman2
    0x6C45E9A8,  # extra candidate
]

CRC16 = [
    (0x1021, 0x0000, False, 0x0000),  # XMODEM
    (0x1021, 0xFFFF, False, 0x0000),  # CCITT-FALSE
    (0x1021, 0x1D0F, False, 0x0000),  # AUG-CCITT
    (0x1021, 0xB2AA, False, 0x0000),  # GENIBUS
    (0x1021, 0x89EC, False, 0x0000),  # KERMIT (non-ref impl of 8408)
    (0x8408, 0x0000, True, 0x0000),   # KERMIT reflected
    (0xA001, 0x0000, True, 0x0000),   # IBM/ARC (Modbus init 0xFFFF below)
    (0xA001, 0xFFFF, True, 0x0000),   # Modbus
    (0xA001, 0x800D, True, 0x0000),   # DNP? (approx)
    (0x3D65, 0x0000, False, 0xFFFF),  # DNP-style
    (0xC867, 0xFFFF, False, 0x0000),  # CDMA2000
    (0x0589, 0x0000, False, 0x0001),  # DECT
    (0x8005, 0x0000, False, 0x0000),  # IBM normal
    (0x1021, 0xC6C6, False, 0x0000),  # CMS/Buypass style
]


def crc32_table(poly, refin):
    if refin:
        rp = int(f"{poly:032b}"[::-1], 2)
        table = []
        for b in range(256):
            c = b
            for _ in range(8):
                c = (rp ^ (c >> 1)) if (c & 1) else (c >> 1)
            table.append(c)
        return table, rp
    table = []
    for b in range(256):
        c = b << 24
        for _ in range(8):
            c = ((c << 1) ^ poly) & 0xFFFFFFFF if (c & 0x80000000) else (c << 1) & 0xFFFFFFFF
        table.append(c)
    return table, poly


def crc32(data, poly, init, refin, refout, xorout):
    table, _ = crc32_table(poly, refin)
    c = init
    if refin:
        c = int(f"{c:032b}"[::-1], 2)
        for b in data:
            c = table[(c ^ b) & 0xFF] ^ (c >> 8)
        if not refout:
            c = int(f"{c:032b}"[::-1], 2)
    else:
        for b in data:
            c = table[((c >> 24) ^ b) & 0xFF] ^ ((c << 8) & 0xFFFFFFFF)
        if refout:
            c = int(f"{c:032b}"[::-1], 2)
    return (c ^ xorout) & 0xFFFFFFFF


def reflect16(v):
    return int(f"{v:016b}"[::-1], 2)


def crc16(data, poly, init, ref, xorout):
    c = init
    for b in data:
        if ref:
            c ^= b
            for _ in range(8):
                c = (poly ^ (c >> 1)) if (c & 1) else (c >> 1)
        else:
            c ^= b << 8
            for _ in range(8):
                c = ((poly ^ (c << 1)) & 0xFFFF) if (c & 0x8000) else ((c << 1) & 0xFFFF)
    return (c ^ xorout) & 0xFFFF


def rol32(v, n):
    n &= 31
    return ((v << n) | (v >> (32 - n))) & 0xFFFFFFFF


def ror32(v, n):
    n &= 31
    return ((v >> n) | (v << (32 - n))) & 0xFFFFFFFF


def non_crc_variants(data):
    """Yield (name, value) for the non-CRC families."""
    out = []
    # Adler-32 variants
    for mod in (65521, 65535, 0xFFFFFFFF):
        a, b = 1, 0
        for x in data:
            a = (a + x) % mod
            b = (b + a) % mod
        out.append((f"adler{mod}", ((b << 16) | a) & 0xFFFFFFFF))
        out.append((f"adler{mod}-swap", ((a << 16) | b) & 0xFFFFFFFF))
    # Fletcher-32 (16-bit lanes) / Fletcher-16
    s1 = s2 = 0
    for i, x in enumerate(data):
        s1 = (s1 + x) & 0xFF
        s2 = (s2 + s1) & 0xFF
    out.append(("fletcher16", (s2 << 8) | s1))
    s1 = s2 = 0
    for x in data:
        s1 = (s1 + x) & 0xFFFF
        s2 = (s2 + s1) & 0xFFFF
    out.append(("fletcher32", (s2 << 16) | s1))
    # FNV / DJB2 / sdbm / JS / one-at-a-time
    h = 0x811C9DC5
    for x in data:
        h = ((h * 0x01000193) ^ x) & 0xFFFFFFFF
    out.append(("fnv1a", h))
    h = 0x811C9DC5
    for x in data:
        h = ((h * 0x01000193) & 0xFFFFFFFF) ^ 0  # fnv1: mul after xor
    h = 0x811C9DC5
    for x in data:
        h = ((h ^ x) * 0x01000193) & 0xFFFFFFFF
    out.append(("fnv1", h))
    h = 5381
    for x in data:
        h = ((h << 5) + h + x) & 0xFFFFFFFF
    out.append(("djb2", h))
    h = 5381
    for x in data:
        h = ((h << 5) + h) ^ x & 0xFFFFFFFF
    out.append(("djb2x", h & 0xFFFFFFFF))
    h = 0
    for x in data:
        h = (h + (h << 6) - h + x) & 0xFFFFFFFF  # sdbm
    out.append(("sdbm", h))
    h = 1315423911
    for x in data:
        h = ((h << 5) ^ x + (h >> 2)) & 0xFFFFFFFF
    out.append(("js", h))
    h = 0xFFFFFFFF
    for x in data:
        h = (h + x) & 0xFFFFFFFF
        h = (h + (h << 10)) & 0xFFFFFFFF
        h = (h ^ (h >> 6)) & 0xFFFFFFFF
    h = (h + (h << 3)) & 0xFFFFFFFF
    h = (h ^ (h >> 11)) & 0xFFFFFFFF
    h = (h + (h << 15)) & 0xFFFFFFFF
    out.append(("one_at_a_time", h & 0xFFFFFFFF))
    # plain sums / xors
    out.append(("sum32", sum(data) & 0xFFFFFFFF))
    out.append(("negsum32", (-sum(data)) & 0xFFFFFFFF))
    x = 0
    for b in data:
        x ^= b
    out.append(("xor8", x))
    x = 0
    for i in range(0, len(data) - 1, 2):
        x ^= data[i] | (data[i + 1] << 8)
    out.append(("xor16le", x & 0xFFFF))
    x = 0
    for i in range(0, len(data) - 3, 4):
        x ^= struct.unpack_from("<I", data, i)[0]
    out.append(("xor32le", x))
    x = 0
    for i in range(0, len(data) - 3, 4):
        x ^= struct.unpack_from(">I", data, i)[0]
    out.append(("xor32be", x))
    # BSD / SysV (u16)
    c = 0
    for b in data:
        c = ((c >> 1) | ((c & 1) << 15)) & 0xFFFF
        c = (c + b) & 0xFFFF
    out.append(("bsd16", c))
    s = 0
    for b in data:
        s += b
    out.append(("sysv16", ((s & 0xFFFF) + (s >> 16)) & 0xFFFF))
    # rotated accumulators
    for n in range(1, 16):
        c = 0
        for b in data:
            c = ((ror32(c, n) + b) & 0xFFFFFFFF)
        out.append((f"ror{n}+sum", c))
        c = 0
        for b in data:
            c = ((rol32(c, n) + b) & 0xFFFFFFFF)
        out.append((f"rol{n}+sum", c))
        c = 0
        for b in data:
            c = (ror32(c, n) ^ b) & 0xFFFFFFFF
        out.append((f"ror{n}^xor", c))
        c = 0
        for b in data:
            c = (rol32(c, n) ^ b) & 0xFFFFFFFF
        out.append((f"rol{n}^xor", c))
    # weighted sums
    w = 0
    for i, b in enumerate(data):
        w = (w + (i + 1) * b) & 0xFFFFFFFF
    out.append(("weighted_sum", w))
    w = 0
    for i, b in enumerate(data):
        w = (w + i * b) & 0xFFFFFFFF
    out.append(("weighted_sum0", w))
    # word-wise sums LE/BE
    s = 0
    for i in range(0, len(data) - 3, 4):
        s = (s + struct.unpack_from("<I", data, i)[0]) & 0xFFFFFFFF
    out.append(("wordsum_le", s))
    s = 0
    for i in range(0, len(data) - 3, 4):
        s = (s + struct.unpack_from(">I", data, i)[0]) & 0xFFFFFFFF
    out.append(("wordsum_be", s))
    # byte-swapped lane sums
    s = 0
    for i in range(0, len(data) - 3, 4):
        s = (s + struct.unpack_from("<I", data, i)[0]) & 0xFFFFFFFF
    out.append(("wordsum_le_bswap", int(f"{s:032b}"[::-1], 2) if s else 0))
    return out


def main():
    flash = open(sys.argv[1], "rb").read()
    d1 = flash[P1_DATA[0]:P1_DATA[0] + P1_DATA[1]]
    d2 = flash[P2_DATA[0]:P2_DATA[0] + P2_DATA[1]]
    targets1 = {P1_EXP, int(f"{P1_EXP:032b}"[::-1], 2)}
    targets2 = {P2_EXP, int(f"{P2_EXP:032b}"[::-1], 2)}
    hits = []

    # CRC-32 catalog
    for poly in CRC32_POLYS:
        for init in (0x00000000, 0xFFFFFFFF):
            for refin in (False, True):
                for refout in (False, True):
                    for xorout in (0x00000000, 0xFFFFFFFF):
                        v1 = crc32(d1, poly, init, refin, refout, xorout)
                        if v1 in targets1:
                            v2 = crc32(d2, poly, init, refin, refout, xorout)
                            hits.append(("crc32", poly, init, refin, refout,
                                         xorout, v1, v2 in targets2))
                            print("CRC32 HIT:", hex(poly), hex(init),
                                  f"refin={refin} refout={refout}",
                                  f"xorout={hex(xorout)}", hex(v1),
                                  "p2:", hex(v2), v2 in targets2)

    # CRC-16 families (compare low 16 bits of the target)
    for poly, init, ref, xorout in CRC16:
        v1 = crc16(d1, poly, init, ref, xorout)
        if v1 in (P1_EXP & 0xFFFF, (P1_EXP >> 16) & 0xFFFF):
            v2 = crc16(d2, poly, init, ref, xorout)
            print("CRC16 HIT:", hex(poly), hex(init), f"ref={ref}",
                  f"xorout={hex(xorout)}", hex(v1), "p2:", hex(v2))

    # non-CRC families
    for name, v in non_crc_variants(d1):
        if v in targets1:
            for name2, v2 in non_crc_variants(d2):
                if name2 == name:
                    print("NONCRC HIT:", name, hex(v), "p2:", hex(v2),
                          v2 in targets2)
                    hits.append((name, v, v2 in targets2))
    # u16 variants against halfwords of target
    for name, v in non_crc_variants(d1):
        if v in ((P1_EXP & 0xFFFF), (P1_EXP >> 16) & 0xFFFF):
            print("NONCRC-16 HIT:", name, hex(v))
    if not hits:
        print("no algorithm found in swept families")


if __name__ == "__main__":
    main()
