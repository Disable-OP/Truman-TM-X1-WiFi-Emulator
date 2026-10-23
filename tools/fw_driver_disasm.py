#!/usr/bin/env python3
"""Disassemble the "LD eCos" driver (payload 0xA2000000+) to extract the
hardware decompression engine's exact interface (Milestone J ground truth).

The driver ran under the emulator and programmed the engine; every register
write and table upload it performs is the specification of the engine.
"""
import struct
import sys
import capstone

md = capstone.Cs(capstone.CS_ARCH_MIPS, capstone.CS_MODE_MIPS32 | capstone.CS_MODE_LITTLE_ENDIAN)
md.detail = False

BASE = 0xA2000000


def disasm(payload, start, end, label=""):
    print(f"\n===== {label} 0x{BASE + start:08x}..0x{BASE + end:08x} =====")
    code = payload[start:end]
    for ins in md.disasm(code, BASE + start):
        print(f"  {ins.address:08x}: {ins.bytes.hex(' '):<12s} {ins.mnemonic:<8s} {ins.op_str}")


def main():
    payload = open("/home/z/my-project/analysis/payload.bin", "rb").read()
    print(f"payload: {len(payload)} bytes (0x{len(payload):x})")

    # Driver entry per docs: 0xA2001F50. Disassemble generously; the driver
    # and its helpers extend through the wait loop (~0xA2002548) and the
    # accounting block (~0xA2002630).
    disasm(payload, 0x2100, 0x2400, "driver: header parse + table build/upload")
    disasm(payload, 0x2400, 0x2b00, "driver: upload end, start, wait loop, accounting")


if __name__ == "__main__":
    main()
