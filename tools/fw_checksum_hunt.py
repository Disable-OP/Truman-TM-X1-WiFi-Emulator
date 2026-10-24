#!/usr/bin/env python3
"""Q8: recover the SDK checksum algorithm from the known pair
(driver payload flash[0x2F6C..0x6948] -> 0x5DB06FB5, desc word @0x1224).

Sweeps CRC-32 parameter catalogs, Fletcher/Adler, sums (byte/word,
1's-complement, weighted), XOR folds, common hashes, and 2-byte-unit
accumulations. Data variants: raw, reversed, word-swapped.
"""
import struct

flash = open("/home/z/my-project/Truman-TM-X1-WiFi-Emulator/"
             "firmware/1506HV_512M_4M_SVT2_TRUMAN_TM_X1_WIFI_Internal_flashfile_20220902.bin", "rb").read()
PAY = flash[0x2F6C:0x2F6C + 0x3ADC]
TARGETS = {0x5DB06FB5, 0xB56FB05D, 0x5DB06FB5 ^ 0xFFFFFFFF, 0xB56FB05D ^ 0xFFFFFFFF}
found = []

def crc32_generic(data, poly, init, refin, refout, xorout):
    if refin:
        # reflected implementation
        crc = init & 0xFFFFFFFF
        for b in data:
            crc ^= b
            for _ in range(8):
                crc = (crc >> 1) ^ (poly if crc & 1 else 0)
                poly = poly  # noqa
        return crc
    crc = init & 0xFFFFFFFF
    for b in data:
        crc ^= b << 24
        for _ in range(8):
            crc = ((crc << 1) ^ poly) & 0xFFFFFFFF if crc & 0x80000000 else (crc << 1) & 0xFFFFFFFF
    return crc

REF_POLYS = {  # reflected (bit-reversed) polys for LSB-first
    "CRC32/ISO": 0xEDB88320, "CRC32/C": 0x82F63B78, "CRC32/D": 0xA833982B,
    "CRC32/Q": 0x813C7FBB, "CRC32/AUTOSAR": 0xC8DF352F, "CRC32/BZIP2r": 0x04C11DB7,
    "CRC32/XFER": 0x8AFCEE0B, "CRC32/CDMA2000": 0xE3069283,
    "CRC32/GP4": 0x486852B9, "CRC32/K": 0xDA9C0F9B, "CRC32/BASE91-D": 0xA833982B,
    "CRC32/ISCSI": 0x82F63B78, "CRC32/AIXM": 0x814141AB, "CRC32/CD-ROM": 0x8001801B,
}
INITX = [(0, 0), (0xFFFFFFFF, 0), (0xFFFFFFFF, 0xFFFFFFFF), (0, 0xFFFFFFFF)]

def variants(data):
    yield "raw", data
    yield "rev", data[::-1]
    b2 = bytearray(data)
    for i in range(0, len(b2) - 1, 2):
        b2[i], b2[i+1] = b2[i+1], b2[i]
    yield "swap2", bytes(b2)

def report(name, v):
    if v in TARGETS:
        found.append((name, hex(v)))

# --- CRC sweep ---
for pname, rp in REF_POLYS.items():
    for init, xout in INITX:
        for dname, data in variants(PAY):
            v = crc32_generic(data, rp, init, True, True, xout)
            report(f"crc32r {pname} init={init:08x} xor={xout:08x} {dname}", v)
            # non-reflected with the normal poly
            npoly = int(f"{rp:032b}"[::-1], 2)
            v2 = crc32_generic(data, npoly, init, False, False, xout)
            report(f"crc32n {pname} init={init:08x} xor={xout:08x} {dname}", v2)

# --- Fletcher / Adler ---
def fletcher32(data, mod=65535, swap=False):
    s1 = s2 = 0
    it = range(0, len(data) - 1, 2)
    for i in it:
        a, b = data[i], data[i+1]
        w = (b << 8) | a if swap else (a << 8) | b
        s1 = (s1 + w) % mod
        s2 = (s2 + s1) % mod
    return (s2 << 16) | s1

def adler32(data, mod=65521):
    a, b = 1, 0
    for byte in data:
        a = (a + byte) % mod
        b = (b + a) % mod
    return (b << 16) | a

for dname, data in variants(PAY):
    report(f"fletcher32 le {dname}", fletcher32(data))
    report(f"fletcher32 be {dname}", fletcher32(data, swap=True))
    report(f"adler32 {dname}", adler32(data))
    report(f"adler32 m65535 {dname}", adler32(data, mod=65535))

# --- sums ---
def ip_csum(data):
    if len(data) & 1:
        data += b"\x00"
    s = 0
    for i in range(0, len(data), 2):
        s += struct.unpack_from(">H", data, i)[0]
        s = (s & 0xFFFF) + (s >> 16)
    return (~s) & 0xFFFF

for dname, data in variants(PAY):
    u8 = sum(data) & 0xFFFFFFFF
    u16l = sum(struct.unpack_from("<%dH" % (len(data)//2), data, 0)) & 0xFFFFFFFF
    u16b = sum(struct.unpack_from(">%dH" % (len(data)//2), data, 0)) & 0xFFFFFFFF
    u32l = sum(struct.unpack_from("<%dI" % (len(data)//4), data, 0)) & 0xFFFFFFFF
    u32b = sum(struct.unpack_from(">%dI" % (len(data)//4), data, 0)) & 0xFFFFFFFF
    x8 = 0
    for b in data: x8 ^= b
    x32 = 0
    for i in range(0, len(data)-3, 4):
        x32 ^= struct.unpack_from("<I", data, i)[0]
    bsd = 0
    for b in data:
        bsd = ((bsd >> 1) | ((bsd & 1) << 15)) + b
        bsd &= 0xFFFF
    # 1's complement 32-bit sum
    c32 = 0
    for i in range(0, len(data)-3, 4):
        c32 += struct.unpack_from(">I", data, i)[0]
        c32 = (c32 & 0xFFFFFFFF) + (c32 >> 32)
    c32 = (~c32) & 0xFFFFFFFF
    for nm, v in (("sum8", u8), ("sum16le", u16l), ("sum16be", u16b),
                  ("sum32le", u32l), ("sum32be", u32b), ("xor8", x8),
                  ("xor32", x32), ("bsd", bsd), ("csum32be", c32),
                  ("ip16be", ip_csum(data))):
        report(f"{nm} {dname}", v)
        report(f"{nm} {dname} bsw", struct.unpack("<I", struct.pack(">I", v))[0])

# --- hashes ---
def fnv1(data):
    h = 0x811C9DC5
    for b in data:
        h = (h * 0x01000193) & 0xFFFFFFFF
        h ^= b
    return h

def fnv1a(data):
    h = 0x811C9DC5
    for b in data:
        h ^= b
        h = (h * 0x01000193) & 0xFFFFFFFF
    return h

def djb2(data):
    h = 5381
    for b in data:
        h = ((h << 5) + h + b) & 0xFFFFFFFF
    return h

def sdbm(data):
    h = 0
    for b in data:
        h = (b + (h << 6) + (h << 16) - h) & 0xFFFFFFFF
    return h

def joaat(data):
    h = 0
    for b in data:
        h = (h + b) & 0xFFFFFFFF
        h = (h + (h << 10)) & 0xFFFFFFFF
        h ^= h >> 6
        h = (h + (h << 3)) & 0xFFFFFFFF
        h ^= h >> 11
        h = (h + (h << 15)) & 0xFFFFFFFF
    return h

for dname, data in variants(PAY):
    for nm, f in (("fnv1", fnv1), ("fnv1a", fnv1a), ("djb2", djb2),
                  ("sdbm", sdbm), ("joaat", joaat)):
        report(f"{nm} {dname}", f(data))

# --- 2-byte-unit themed rotations ---
def rotl(x, n):
    return ((x << n) | (x >> (32 - n))) & 0xFFFFFFFF

for dname, data in variants(PAY):
    for sh in (1, 2, 3, 4, 7, 8, 15, 16, 17, 31):
        acc = 0
        for i in range(0, len(data)-1, 2):
            w = struct.unpack_from("<H", data, i)[0]
            acc = ((rotl(acc, sh) + w) & 0xFFFFFFFF)
        report(f"rotsum{sh} le {dname}", acc)
        report(f"rotsum{sh} le x", acc ^ 0xFFFFFFFF)

print("sweep done;", len(found), "hits")
for f in found:
    print("HIT:", f)
