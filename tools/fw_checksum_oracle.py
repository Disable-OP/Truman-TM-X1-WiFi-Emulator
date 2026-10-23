#!/usr/bin/env python3
"""Checksum oracle search for the eCos descriptor (Milestone L).

The 28-byte descriptor at flash 0x10010 ends with the word 0xB5EBF9FF —
per docs §14 the "desc[6] checksum" whose algorithm is the decisive
missing oracle: a matching checksum validates the true eCos decode
offline, without executing anything.

This tool sweeps common checksum families over the plausible coverage
regions (container bytes, decoded image candidates) and reports hits.

Usage: python3 tools/fw_checksum_oracle.py <flash.bin> <ecos_decode.bin>
"""
import struct
import sys
import zlib

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_ring_sweep import load_events, rfc_decode, ring_decode

FLASH = sys.argv[1]
ECOS = sys.argv[2]

EXPECTED = 0xB5EBF9FF


def candidates(data):
    """All simple checksum values of data."""
    out = {}
    s8 = sum(data) & 0xFFFFFFFF
    out["sum_u8"] = s8
    words_le = struct.unpack(f"<{len(data) // 4}I", data[: len(data) // 4 * 4])
    words_be = struct.unpack(f">{len(data) // 4}I", data[: len(data) // 4 * 4])
    out["sum_u32le"] = sum(words_le) & 0xFFFFFFFF
    out["sum_u32be"] = sum(words_be) & 0xFFFFFFFF
    h = len(data) // 2 * 2
    out["sum_u16le"] = sum(struct.unpack_from(f"<{h // 2}H", data)) & 0xFFFFFFFF
    out["sum_u16be"] = sum(struct.unpack_from(f">{h // 2}H", data)) & 0xFFFFFFFF
    x = 0
    for w in words_le:
        x ^= w
    out["xor_u32le"] = x
    x = 0
    for b in data:
        x ^= b
    out["xor_u8"] = x
    out["crc32"] = zlib.crc32(data) & 0xFFFFFFFF
    out["adler32"] = zlib.adler32(data) & 0xFFFFFFFF
    # BSD sum, SysV sum
    b = 0
    for byte in data:
        b = (b >> 1) + ((b & 1) << 15)
        b = (b + byte) & 0xFFFF
    out["bsd_sum16"] = b
    return out


def main():
    flash = open(FLASH, "rb").read()
    eng = open(ECOS, "rb").read()

    regions = {
        "container(0x1002C,0x868C)": flash[0x1002C:0x1002C + 0x868C],
        "container stream(+8)": flash[0x1002C + 8:0x1002C + 0x868C],
        "container+hdr(0x1002C,+6..)": flash[0x1002C:0x1002C + 0x8692],
        "whole package(0x10000,0x1B000)": flash[0x10000:0x10000 + 0x1B000],
        "desc region(0x10010,0x1C)": flash[0x10010:0x10010 + 0x1C],
        "engine dump(79872)": eng,
        "engine written(75398)": eng[:75398],
        "engine nonzero(75657)": eng[:75657],
    }
    # RFC decode candidate
    events = load_events(flash)
    ref = rfc_decode(events, 0x8000)
    regions["rfc_decode"] = ref
    regions["rfc_pad_79872"] = ref + b"\x00" * (79872 - len(ref))

    for name, data in regions.items():
        c = candidates(data)
        hits = [k for k, v in c.items() if v == EXPECTED]
        flag = "  <<<< MATCH" if hits else ""
        show = {k: f"{v:08X}" for k, v in sorted(c.items())}
        print(f"{name:32s} {show}{flag}")
        if name == "desc region(0x10010,0x1C)":
            print(f"    (expected {EXPECTED:08X}; region = {data.hex(' ')})")


if __name__ == "__main__":
    main()
