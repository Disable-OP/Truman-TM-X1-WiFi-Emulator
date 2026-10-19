#!/usr/bin/env python3
"""Quantify the repeated-prologue structure across 64KB blocks.

Usage: python3 fw_repeat.py <firmware.bin>
"""
import struct
import sys

from capstone import Cs, CS_ARCH_MIPS, CS_MODE_MIPS32, CS_MODE_LITTLE_ENDIAN


def main():
    path = sys.argv[1]
    with open(path, "rb") as f:
        data = f.read()

    sig = bytes.fromhex("b56f002419081a3c11205a3753551a3c")

    print("=== SIGNATURE OCCURRENCES (28-byte prologue magic) ===")
    offs = []
    start = 0
    while True:
        i = data.find(sig, start)
        if i < 0:
            break
        offs.append(i)
        start = i + 1
    aligned = [o for o in offs if o % 0x10000 == 0]
    print(f"total occurrences: {len(offs)}, 64KB-aligned: {len(aligned)}")
    for o in aligned:
        # how many leading bytes identical to block 0?
        n = 0
        while n < 0x10000 and o + n < len(data) and data[o + n] == data[n]:
            n += 1
        print(f"  0x{o:06x}: identical to block0 prefix = 0x{n:04x} bytes")

    print("\n=== NON-ALIGNED OCCURRENCES (first 20) ===")
    for o in [x for x in offs if x % 0x10000 != 0][:20]:
        print(f"  0x{o:06x}")

    # Disassemble what follows the prologue at 0x20000 (the 'compressed' region)
    print("\n=== DISASM @0x20000 +0x70..0x140 (does real code follow?) ===")
    md = Cs(CS_ARCH_MIPS, CS_MODE_MIPS32 | CS_MODE_LITTLE_ENDIAN)
    base = 0xBFC00000
    count = 0
    for insn in md.disasm(data[0x20000 + 0x70:0x20000 + 0x140], base + 0x20000 + 0x70):
        print(f"  0x{insn.address:08x}: {insn.bytes.hex(' '):<12} {insn.mnemonic:<8} {insn.op_str}")
        count += 1
        if count >= 30:
            break

    # Compare block 0x10000 region beyond prologue against block 0x20000
    print("\n=== BLOCK DIVERGENCE MAP (byte index where each block diverges from block0) ===")

    print("\n=== HEADER WORDS @0x20000 vs @0x0 (first mismatch) ===")
    for i in range(0, 0x400, 4):
        w0 = struct.unpack_from("<I", data, i)[0]
        w1 = struct.unpack_from("<I", data, 0x20000 + i)[0]
        if w0 != w1:
            print(f"  first word mismatch at 0x{i:03x}: block0=0x{w0:08x} block0x20000=0x{w1:08x}")
            break
    else:
        print("  identical through 0x400")


if __name__ == "__main__":
    main()
