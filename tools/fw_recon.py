#!/usr/bin/env python3
"""Firmware first-pass analysis for Truman TM X1 WiFi flash image.

Reproducible evidence collection:
  - identity (size, sha256, md5)
  - 64 KiB-block entropy map (locate code vs. compressed/FS vs. padding)
  - hexdump of head/tail regions
  - keyword-filtered strings with offsets

Usage: python3 fw_recon.py <firmware.bin>
"""
import hashlib
import math
import re
import sys

BLOCK = 64 * 1024


def entropy(data: bytes) -> float:
    if not data:
        return 0.0
    counts = [0] * 256
    for b in data:
        counts[b] += 1
    e = 0.0
    n = len(data)
    for c in counts:
        if c:
            p = c / n
            e -= p * math.log2(p)
    return e


def hexdump(data: bytes, base: int, limit: int = 512):
    out = []
    for i in range(0, min(len(data), limit), 16):
        chunk = data[i:i + 16]
        hexs = " ".join(f"{b:02x}" for b in chunk)
        asc = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
        out.append(f"{base + i:08x}  {hexs:<47}  |{asc}|")
    return "\n".join(out)


def main():
    path = sys.argv[1]
    with open(path, "rb") as f:
        data = f.read()

    print("=== IDENTITY ===")
    print(f"size   : {len(data)} bytes (0x{len(data):x})")
    print(f"sha256 : {hashlib.sha256(data).hexdigest()}")
    print(f"md5    : {hashlib.md5(data).hexdigest()}")

    print("\n=== HEAD 0x00000000 ===")
    print(hexdump(data, 0, 512))
    print("\n=== TAIL (last 256 bytes) ===")
    print(hexdump(data[-256:], len(data) - 256, 256))

    print("\n=== ENTROPY MAP (64 KiB blocks) ===")
    for i in range(0, len(data), BLOCK):
        blk = data[i:i + BLOCK]
        e = entropy(blk)
        nonzero = sum(1 for b in blk if b != 0)
        ff = sum(1 for b in blk if b == 0xFF)
        tag = ""
        if ff == len(blk):
            tag = "ERASED(FF)"
        elif nonzero == 0:
            tag = "ZERO"
        elif e > 7.5:
            tag = "COMPRESSED?"
        elif e < 4.0:
            tag = "LOW-ENTROPY"
        print(f"  0x{i:06x}-0x{i + len(blk) - 1:06x}  entropy={e:5.2f}  zero={100 * (len(blk) - nonzero - ff) / len(blk):4.1f}%  ff={100 * ff / len(blk):4.1f}%  {tag}")

    print("\n=== KEYWORD STRINGS (offset : text) ===")
    keywords = re.compile(
        rb"(?i)(mips|arm|arm9|cortex|risc|jz4|sunplus|sphe|1506|alias|boot|uboot|"
        rb"u-boot|linux|rtos|ecu|loader|flash|spi|nand|nor|ddr|sdram|iram|"
        rb"exception|vector|reset|uart|panic|version|checksum|crc|partition|"
        rb"rootfs|squashfs|jffs|cramfs|minix|vfat|fat|ext2|upgrade|ota)"
    )
    seen = set()
    shown = 0
    for m in re.finditer(rb"[\x20-\x7e]{6,}", data):
        s = m.group(0)
        if keywords.search(s):
            off = m.start()
            key = s[:60]
            if key in seen:
                continue
            seen.add(key)
            try:
                text = s.decode("ascii")
            except UnicodeDecodeError:
                continue
            print(f"  0x{off:06x}  {text[:120]}")
            shown += 1
            if shown >= 200:
                print("  ... (limit reached)")
                break


if __name__ == "__main__":
    main()
