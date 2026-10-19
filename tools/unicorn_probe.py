#!/usr/bin/env python3
"""Unicorn2 MIPS32 capability probe (evidence for CPU backend decision).

Result (unicorn 2.x python wheel, x86_64 Linux):
  - MIPS32 LE execution works for addresses < 0x80000000.
  - ANY fetch from kseg addresses (>= 0x80000000) fails with
    UC_ERR_FETCH_UNMAPPED, regardless of sign-extension of the address.
  - The Truman firmware executes from the MIPS boot window at 0xBFC00000
    and dereferences 0xBFFE0000/0xBFFC0000/0x98010000 constantly.

Conclusion: Unicorn2 (as shipped) cannot execute this firmware's kseg-based
code; a custom interpreter backend is justified. See docs/firmware-analysis.md.

Usage: python3 unicorn_probe.py
"""
from unicorn import Uc, UcError, UC_ARCH_MIPS, UC_MODE_MIPS32
from unicorn.mips_const import UC_MIPS_REG_T0

TEST = bytes.fromhex("3412083c78560835")  # lui $t0,0x1234 ; ori $t0,$t0,0x5678


def probe(addr: int, label: str):
    for a in (addr, addr | 0xFFFFFFFF00000000):
        try:
            uc = Uc(UC_ARCH_MIPS, UC_MODE_MIPS32)
            uc.mem_map(a, 0x10000)
            uc.mem_write(a, TEST)
            uc.emu_start(a, a + 8)
            t0 = uc.reg_read(UC_MIPS_REG_T0) & 0xFFFFFFFF
            print(f"OK    {label} @ 0x{a:016x}  (t0=0x{t0:08x})")
            return True
        except UcError as e:
            print(f"FAIL  {label} @ 0x{a:016x}  ({e})")
    return False


def main():
    ok = []
    for addr, label in [
        (0x7FF00000, "below-kseg0"),
        (0x1FC00000, "physical boot window"),
        (0x80000000, "kseg0 base"),
        (0xA0000000, "kseg1 base"),
        (0xBFC00000, "MIPS boot vector window"),
    ]:
        if probe(addr, label):
            ok.append(label)
    print("\nSummary:", ", ".join(ok) or "none")


if __name__ == "__main__":
    main()
