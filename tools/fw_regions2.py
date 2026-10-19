#!/usr/bin/env python3
"""Region-level analysis v2 (fixed slicing): headers, magics, config areas.

Usage: python3 fw_regions2.py <firmware.bin>
"""
import re
import struct
import sys

MAGICS = {
    b"\x1f\x8b": "gzip",
    b"BZh": "bzip2",
    b"\xfd7zXZ\x00": "xz",
    b"U-Boot": "u-boot",
    b"\x89LZO": "lzo",
    b"CrAmFs": "cramfs",
    b"hsqs": "squashfs-le",
    b"sqsh": "squashfs-be",
    b"JFFS": "jffs2",
    b"SPHE": "sunplus-sphe",
    b"\x04\x22": "mips-jr-ra?",
}


def hexdump_at(data: bytes, off: int, limit: int):
    out = []
    for i in range(0, limit, 16):
        chunk = data[off + i:off + i + 16]
        if not chunk:
            break
        hexs = " ".join(f"{b:02x}" for b in chunk)
        asc = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
        out.append(f"{off + i:08x}  {hexs:<47}  |{asc}|")
    return "\n".join(out)


def main():
    path = sys.argv[1]
    with open(path, "rb") as f:
        data = f.read()

    for off, label, lim in [
        (0x010000, "after stage1/2 (0x10000)", 0x100),
        (0x018000, "second half of 0x10000 block", 0x80),
        (0x01F000, "end of 0x10000 block", 0x80),
        (0x020000, "main blob start", 0x80),
        (0x030000, "main blob mid", 0x40),
        (0x37F000, "main blob end", 0x80),
        (0x380000, "region 0x380000", 0x100),
        (0x38B000, "SPHE15 string area", 0x100),
        (0x3B0000, "region 0x3B0000", 0x80),
        (0x3C0000, "region 0x3C0000", 0x80),
        (0x3D0000, "region 0x3D0000", 0x100),
        (0x3F0000, "region 0x3F0000", 0x80),
    ]:
        print(f"=== {label} ===")
        print(hexdump_at(data, off, lim))
        print()

    print("=== MAGIC SCAN ===")
    found = {}
    for magic, name in MAGICS.items():
        start = 0
        offs = []
        while True:
            i = data.find(magic, start)
            if i < 0:
                break
            offs.append(i)
            start = i + 1
        if offs and len(offs) < 500:
            found[name] = offs
    for name, offs in found.items():
        offs_s = ", ".join(f"0x{o:06x}" for o in offs[:10])
        more = f" (+{len(offs) - 10} more)" if len(offs) > 10 else ""
        print(f"  {name:<14}: {offs_s}{more}")

    print("\n=== STRINGS 0x010000-0x020000 ===")
    for m in re.finditer(rb"[\x20-\x7e]{6,}", data[0x010000:0x020000]):
        print(f"  0x{0x010000 + m.start():06x}  {m.group(0)[:110].decode('ascii', 'replace')}")

    print("\n=== STRINGS 0x380000-0x3C0000 ===")
    for m in re.finditer(rb"[\x20-\x7e]{8,}", data[0x380000:0x3C0000]):
        print(f"  0x{0x380000 + m.start():06x}  {m.group(0)[:110].decode('ascii', 'replace')}")

    print("\n=== STRINGS 0x3C0000-0x400000 ===")
    for m in re.finditer(rb"[\x20-\x7e]{8,}", data[0x3C0000:0x400000]):
        print(f"  0x{0x3C0000 + m.start():06x}  {m.group(0)[:110].decode('ascii', 'replace')}")

    print("\n=== STRINGS 0x9340-0x10000 (rest of stage2) ===")
    for m in re.finditer(rb"[\x20-\x7e]{6,}", data[0x9340:0x10000]):
        print(f"  0x{0x9340 + m.start():06x}  {m.group(0)[:110].decode('ascii', 'replace')}")


if __name__ == "__main__":
    main()
