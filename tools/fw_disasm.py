#!/usr/bin/env python3
"""Disassemble selected regions of the Truman TM X1 firmware with capstone.

Verifies CPU architecture from actual opcodes (not filenames or strings).
Usage: python3 fw_disasm.py <firmware.bin>
"""
import sys

from capstone import Cs, CS_ARCH_MIPS, CS_MODE_MIPS32, CS_MODE_LITTLE_ENDIAN


def disasm(data: bytes, off: int, count: int, label: str, addr_base: int = 0xBFC00000):
    md = Cs(CS_ARCH_MIPS, CS_MODE_MIPS32 | CS_MODE_LITTLE_ENDIAN)
    code = data[off:off + count * 4]
    print(f"--- {label} (file offset 0x{off:06x}, mapped at 0x{addr_base + off:08x}) ---")
    n = 0
    for insn in md.disasm(code, addr_base + off):
        print(f"  0x{insn.address:08x}: {insn.bytes.hex(' '):<12} {insn.mnemonic:<8} {insn.op_str}")
        n += 1
        if n >= count:
            break
    print()


def main():
    path = sys.argv[1]
    with open(path, "rb") as f:
        data = f.read()

    # Stage-1 loader entry candidates
    disasm(data, 0x0, 16, "offset 0x000000 (flash start)")
    disasm(data, 0x40, 24, "offset 0x000040")
    disasm(data, 0xC0, 16, "offset 0x0000C0")
    disasm(data, 0x170, 12, "offset 0x000170")
    # Stage-2 header (same structure repeated?)
    disasm(data, 0x8000, 16, "offset 0x008000 (stage-2 block)")
    disasm(data, 0x8040, 16, "offset 0x008040")


if __name__ == "__main__":
    main()
