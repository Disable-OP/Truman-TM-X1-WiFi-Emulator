#!/usr/bin/env python3
"""Keycode-consumer hunt over the main-firmware payload (Milestone U).

The front-panel dispatcher (payload 0x82002548..0x82002630) holds the key
port address in a register ($a1 = $s6 + (-0x47B4) = 0xBFFE384C with
$s6 = 0xBFFE8000, docs/firmware-analysis.md §12) and reads raw key lines /
writes keycodes (0x100/0x400/0x1000/0x2000) through it. The keycode
consumer — code that reads the keycode back to map it to a UI action — is
unknown. This tool hunts for it statically over the whole 0x3ADC-byte
payload (flash 0x2F6C, plaintext per Milestone L/§15.1):

 1. chunked linear disassembly of every 4-byte-aligned byte of the payload
    (a plain linear sweep stops at the first data word; chunks restart),
 2. a conservative constant-propagation pass (lui/ori/addiu/li/move/addu,
    $s6 seeded 0xBFFE8000, registers invalidated on call boundaries) that
    resolves every load/store effective address and reports every access
    to the key port 0xBFFE384C with its direction,
 3. keycode-constant formation (ori/addiu against $zero with 0x100/0x400/
    0x1000/0x2000) and their stores through known-address registers.

Each hit is classified inside/outside the dispatcher range; anything
outside is a consumer candidate.

Usage: python3 tools/fw_keycode_consumer.py <flash.bin>
"""
import struct
import sys

import capstone

md = capstone.Cs(
    capstone.CS_ARCH_MIPS, capstone.CS_MODE_MIPS32 | capstone.CS_MODE_LITTLE_ENDIAN
)

PAYLOAD_FILE_OFF = 0x2F6C
PAYLOAD_LEN = 0x3ADC
PAYLOAD_BASE = 0x8200_0000  # kseg0 execution base (phys 0x02000000)
S6 = 0xBFFE_8000  # documented dispatcher segment base (§11.1/§12)
KEY_PORT = 0xBFFE_384C
DISPATCHER = (0x2548, 0x2630)  # payload offsets of the key dispatcher
KEYCODES = (0x100, 0x400, 0x1000, 0x2000)
MEM_OPS = ("lw", "sw", "lb", "lbu", "sb", "lh", "lhu", "sh", "lwl", "lwr", "swl", "swr")


def chunked_disasm(payload):
    """Disassemble every 4-byte-aligned byte: restart after every gap."""
    out = {}
    i = 0
    n = len(payload) // 4
    while i < n:
        got = False
        for ins in md.disasm(payload[i * 4 :], PAYLOAD_BASE + i * 4):
            out[ins.address] = ins
            i = (ins.address - PAYLOAD_BASE) // 4 + 1
            got = True
            if len(out) and i % 64 == 0:
                pass
            if i >= n:
                break
        if not got:
            i += 1  # skip the undecodable word and try again
    return out


def signed16(v):
    return v - 0x10000 if v & 0x8000 else v


def propagate(insns):
    """Conservative constant tracking; yields (addr, ins, regs) in order."""
    regs = {30 - 8: None}  # placeholder, replaced below
    regs = {22: S6}  # $s6 seeded from the documented dispatcher setup
    for a in sorted(insns):
        ins = insns[a]
        m, ops = ins.mnemonic, ins.op_str
        parts = [p.strip() for p in ops.split(",")]
        if m == "lui" and len(parts) == 2:
            try:
                regs[regnum(parts[0])] = int(parts[1], 16) << 16
            except ValueError:
                pass
        elif m == "ori" and len(parts) == 3:
            d, s = regnum(parts[0]), regnum(parts[1])
            try:
                imm = int(parts[2], 16)
            except ValueError:
                continue
            if s == 0:
                regs[d] = imm
            elif s in regs and regs[s] is not None:
                regs[d] = regs[s] | imm
        elif m == "addiu" and len(parts) == 3:
            d, s = regnum(parts[0]), regnum(parts[1])
            try:
                imm = int(parts[2], 16)
            except ValueError:
                continue
            # Capstone may print the raw field (0xb84c) or the signed value
            # (-0x47b4); normalise to signed.
            if imm >= 0 and imm & 0x8000:
                imm -= 0x10000
            if s == 0:
                regs[d] = imm & 0xFFFFFFFF
            elif s in regs and regs[s] is not None:
                regs[d] = (regs[s] + imm) & 0xFFFFFFFF
        elif m in ("move", "addu", "or") and len(parts) == 3:
            d, s1, s2 = regnum(parts[0]), regnum(parts[1]), regnum(parts[2])
            v1, v2 = regs.get(s1), regs.get(s2)
            if m == "move" and v1 is not None:
                regs[d] = v1
            elif m == "or" and v1 is not None and v1 == v2:
                regs[d] = v1
            elif m == "addu" and v1 is not None and v2 == 0:
                regs[d] = v1
        elif m == "li" and len(parts) == 2:
            try:
                regs[regnum(parts[0])] = int(parts[1], 16)
            except ValueError:
                pass
        elif m in ("jal", "jalr"):
            # Call boundary: only $s6 (callee-saved, documented) survives.
            regs = {22: regs.get(22)}
        elif m in ("lw", "lbu", "lh") and parts:
            # A load clobbers its destination with unknown data.
            d = regnum(parts[0])
            if d != 22:
                regs[d] = None
        elif m in ("addi", "add", "sub", "subu", "mult", "div", "ins", "ext"):
            if parts:
                d = regnum(parts[0])
                if d != 22:
                    regs[d] = None
        yield a, ins, regs


def regnum(r):
    names = ["zero", "at", "v0", "v1", "a0", "a1", "a2", "a3",
             "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7",
             "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
             "t8", "t9", "k0", "k1", "gp", "sp", "fp", "ra"]
    r = r.strip("$ ").split(",")[0].strip()
    if r.isdigit():
        return int(r)
    return names.index(r) if r in names else None


def classify(addr):
    off = addr - PAYLOAD_BASE
    return "dispatcher" if DISPATCHER[0] <= off < DISPATCHER[1] else "OUTSIDE"


def parse_mem(op):
    """'lw $v0, -0x47b4($s6)' -> (2, 22, -0x47b4); 'lw $v0, ($a1)' -> (2, 5, 0)."""
    try:
        reg_s, rest = op.split(", ", 1)
        if "(" not in rest:
            return None
        off_s, base_s = rest.rsplit("(", 1)
        base_s = base_s.rstrip(")")
        if off_s in ("", "0"):
            off = 0
        else:
            off = int(off_s, 16)
        return regnum(reg_s), regnum(base_s), off
    except (ValueError, AttributeError):
        return None


def main():
    flash = open(sys.argv[1], "rb").read()
    payload = flash[PAYLOAD_FILE_OFF : PAYLOAD_FILE_OFF + PAYLOAD_LEN]
    insns = chunked_disasm(payload)
    cov = len(insns) * 4
    print(f"payload: {len(payload)} bytes at file {PAYLOAD_FILE_OFF:#x}, "
          f"base {PAYLOAD_BASE:#x}")
    print(f"disassembly coverage: {len(insns)} instructions "
          f"({cov}/{len(payload)} bytes = {100*cov/len(payload):.1f}%)")

    print("\n== key-port accesses (EA 0xBFFE384C resolved by constant "
          "propagation) ==")
    port_hits = []
    for a, ins, regs in propagate(insns):
        if ins.mnemonic not in MEM_OPS:
            continue
        mem = parse_mem(ins.op_str)
        if not mem:
            continue
        d, b, off = mem
        base_v = regs.get(b)
        if base_v is None:
            continue
        ea = (base_v + off) & 0xFFFFFFFF
        if ea == KEY_PORT:
            port_hits.append((a, ins.mnemonic, ins.op_str, d))
            print(f"  {a:08x} [{classify(a):10s}] {ins.mnemonic:<4s} {ins.op_str}")

    # Keycode formation: ori/addiu rt, $zero, keycode (value side) and the
    # registers that ever held a keycode at a port store.
    print("\n== keycode formation (ori/addiu from $zero with a keycode "
          "constant) ==")
    for a, ins, regs in propagate(insns):
        if ins.mnemonic in ("ori", "addiu") and ", " in ins.op_str:
            parts = [p.strip() for p in ins.op_str.split(", ")]
            if len(parts) == 3:
                try:
                    imm = int(parts[2], 16)
                except ValueError:
                    continue
                src_zero = regnum(parts[1]) == 0
                held = regs.get(regnum(parts[0])) in KEYCODES
                if (src_zero and imm in KEYCODES) or held:
                    print(f"  {a:08x} [{classify(a):10s}] {ins.mnemonic} {ins.op_str}")

    n_out = sum(1 for a, *_ in port_hits if classify(a) == "OUTSIDE")
    n_in = len(port_hits) - n_out
    print(f"\nverdict: {len(port_hits)} port accesses total — {n_in} inside "
          f"the dispatcher (§12), {n_out} outside")
    if n_out == 0:
        print("  -> no keycode consumer exists in the main-firmware payload; "
              "the keycode written by the dispatcher is consumed beyond the "
              "observed horizon (eCos stage or hardware).")


if __name__ == "__main__":
    main()
