#!/usr/bin/env python3
"""First-corruption localization: per-byte emit-event log of the engine decode
(Milestone Q).

Facts this tool exploits:

* Literals are GROUND TRUTH — their bytes come from the compressor stream
  itself and cannot be wrong. Only match copies can be wrong (wrong source
  distance / wrong length value).
* Output POSITIONS are fixed by the verified parse — a wrong value corrupts
  content, never alignment.
* The eCos image head executes as harmless zero-register ops until the
  reserved word 0x0000ffff at image+0x50 (a reserved-instruction exception
  on real hardware, so the true bytes there form a valid instruction).
  The head is therefore a sparse pattern of literal clusters separated by
  (assumed-zero) match spans.

For every match this tool reports which bytes it emitted, and — for spans
inside zero-assumed runs — the set of distances D that would have made the
copy emit the expected zeros. Intersecting those candidate sets per distance
symbol yields direct constraints on the engine's distance ladder (independent
of the validity oracle).

Models: m1 = RFC (len extra + dist extra consumed); m2 = len extra only.

Usage: python3 tools/fw_first_corruption.py <flash.bin> [--m2]
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import (Huffman, LBASE, LEXTRA, DBASE, DEXTRA,
                          parse_header, BitReader)

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C


def walk(stream, start_bit, lith, disth, len_extra, dist_extra, out_cap):
    """Event walk with emit attribution. Events carry (out, n, kind, sym,
    dsym, eb, deb, bit) and the byte span each event filled."""
    br = BitReader(stream, start_bit)
    events = []
    outpos = 0
    while (br.pos >> 3) < len(stream) and outpos < out_cap:
        bit = br.pos
        try:
            s = lith.decode(br)
        except EOFError:
            events.append(dict(kind="EOF", bit=bit, out=outpos))
            break
        if s <= 255:
            events.append(dict(kind="LIT", sym=s, bit=bit, out=outpos, n=1))
            outpos += 1
            continue
        if s == 256:
            events.append(dict(kind="EOB", bit=bit, out=outpos, n=0))
            continue
        li = s - 257
        eb = br.bits(LEXTRA[li]) if (len_extra and LEXTRA[li]) else 0
        try:
            ds = disth.decode(br)
        except (EOFError, ValueError):
            events.append(dict(kind="ERR_DIST", bit=bit, out=outpos))
            break
        deb = br.bits(DEXTRA[ds]) if (dist_extra and DEXTRA[ds]) else 0
        ln = LBASE[li] + eb
        events.append(dict(kind="MATCH", sym=s, dsym=ds, eb=eb, deb=deb,
                           dist=DBASE[ds] + deb, bit=bit, out=outpos, n=ln))
        outpos += ln
    return events


def realize(events, window_preset):
    """Materialize the output + per-byte producer map.
    Returns (out, producer) with producer[i] = event index that wrote byte i
    (or -1 = zero preset, i.e. dist>output copy source)."""
    win = bytearray(window_preset)  # logical window = preset + output
    out = bytearray()
    producer = []
    base = len(win)
    for idx, e in enumerate(events):
        if e["kind"] == "LIT":
            win.append(e["sym"])
            out.append(e["sym"])
            producer.append(idx)
        elif e["kind"] == "MATCH":
            d = e["dist"]
            n = len(win)
            for j in range(e["n"]):
                b = win[n - d + j] if d <= n else 0
                win.append(b)
                out.append(b)
                producer.append(idx)
            e["src"] = bytes(out[-e["n"]:])
        e.setdefault("n", 0)
    return out, producer


def word_class(w: int) -> str:
    """Classify a little-endian MIPS32r2 word for the executable-head oracle."""
    op = w >> 26
    if w == 0:
        return "nop"
    if op == 0:  # SPECIAL
        funct = w & 0x3F
        rd = (w >> 11) & 0x1F
        if funct in (0x00, 0x02, 0x03, 0x04, 0x06, 0x07):  # shifts
            return "harmless" if rd == 0 else "regop"
        if funct in (0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27):  # add..nor
            return "harmless" if rd == 0 else "regop"
        if funct in (0x0A, 0x0B):  # movz/movn
            return "harmless" if rd == 0 else "regop"
        if funct in (0x10, 0x11, 0x12, 0x13):  # mfhi/mthi/mflo/mtlo
            return "harmless" if rd == 0 and funct in (0x10, 0x12) else "regop"
        if funct in (0x18, 0x19, 0x1A, 0x1B):  # mult/multu/div/divu
            return "regop"
        if funct in (0x2A, 0x2B):  # slt/sltu
            return "harmless" if rd == 0 else "regop"
        if funct in (0x08, 0x09):  # jr/jalr
            return "jump"
        if funct in (0x0C, 0x0D, 0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32,
                     0x33, 0x34, 0x36):  # syscall/break/traps
            return "trap"
        if funct == 0x0F:  # sync
            return "harmless"
        if funct == 0x01:  # movf/movt
            return "regop"
        return "RESERVED(special)"
    if op == 1:  # REGIMM (bltz/bgez)
        return "branch"
    if op in (0x02, 0x03):  # j/jal
        return "jump"
    if op in (0x04, 0x05, 0x06, 0x07):  # beq/bne/blez/bgtz
        return "branch"
    if op in (0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E):  # addi..ori/xori/lui
        rt = (w >> 16) & 0x1F
        return "harmless" if rt == 0 else "immop"
    if op in (0x10, 0x11, 0x12, 0x13):  # COP0/COP1...
        return "cop"
    if op in (0x14, 0x15, 0x16, 0x17):  # beql/bnel/...
        return "branch"
    if op in (0x1C, 0x1F):  # SPECIAL2/SPECIAL3 (mmu/rdhwr ...)
        return "special23"
    if op in (0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x28, 0x29, 0x2B, 0x2F,
              0x30, 0x31, 0x33, 0x38, 0x39, 0x3B):  # loads/stores
        return "mem"
    return f"RESERVED(op=0x{op:02x})"


def head_words(out, n=0x100):
    """Yield (offset, word, class) for the first n bytes."""
    for off in range(0, min(n, len(out) - 3), 4):
        w = int.from_bytes(out[off:off + 4], "little")
        yield off, w, word_class(w)


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)

    if "--survey" in sys.argv:
        # Executable-head oracle across all four accountings: the true
        # decode's head is the eCos entry code — it must be executable
        # (no reserved word before the first branch/jump side effect).
        models = [
            ("m1 RFC", True, True),
            ("m2 len-only", True, False),
            ("m3 dist-only", False, True),
            ("m4 fixed", False, False),
        ]
        for name, le, de in models:
            events = walk(stream, 1024, lith, disth, le, de, 0x400)
            out, _ = realize(events, 0x8000)
            first_res = first_branch = None
            n_branch = 0
            for off, w, cls in head_words(out, 0x200):
                if cls.startswith("RESERVED") and first_res is None:
                    first_res = off
                if cls in ("branch", "jump"):
                    n_branch += 1
                    if first_branch is None:
                        first_branch = (off, w)
            head8 = out[:32].hex(' ')
            print(f"{name:14s} first_reserved="
                  f"{'none' if first_res is None else hex(first_res):>6s} "
                  f"branches_in_0x200={n_branch:2d} "
                  f"first_branch={first_branch} head={head8}")
        return

    # m1 = RFC (len+dist extras consumed, the emulator's model);
    # --m2 = len extras only (the §18.1 size-anchor accounting).
    m2 = "--m2" in sys.argv
    len_extra, dist_extra = (True, False) if m2 else (True, True)
    events = walk(stream, 1024, lith, disth, len_extra, dist_extra, 0x10000)
    out, producer = realize(events, 0x8000)
    # attach distances now that realize() stored src
    print(f"model={'m2 (len extra only)' if m2 else 'm1 (RFC)'}: "
          f"{len(events)} events, {len(out)} output bytes")
    # ---- 1. per-byte attribution of the head 0x00..0x80 -------------------
    print("\n=== head 0x00..0x7F: per-byte producer ===")
    run = []
    for i in range(0x80):
        ev = events[producer[i]]
        tag = ("LIT" if ev["kind"] == "LIT" else
               f"M[{ev.get('sym')},d={ev['dist']}]")
        run.append(f"{i:02x}:{out[i]:02x}:{tag}")
    for i in range(0, len(run), 4):
        print("  " + "  ".join(run[i:i + 4]))

    # ---- 2. first matches with full detail --------------------------------
    print("\n=== first 48 matches ===")
    shown = 0
    for idx, e in enumerate(events):
        if e["kind"] != "MATCH":
            continue
        src = e.get("src", b"")
        print(f"  out=0x{e['out']:05x} len={e['n']:3d} lsym={e['sym']:3d} "
              f"dsym={e['dsym']:2d} eb={e['eb']} deb={e['deb']} "
              f"src={src[:16].hex(' ')}")
        shown += 1
        if shown >= 48:
            break

    # ---- 3. zero-span distance solver -------------------------------------
    # Literal positions are ground truth. Spans of consecutive match bytes
    # between literals are assumed zero (executed-as-NOP head evidence).
    lit_pos = {e["out"] for e in events if e["kind"] == "LIT"}
    print("\n=== zero-span solver (first 0x400 output bytes) ===")
    win = bytearray(0x8000) + out  # logical window incl. zero preset
    # candidate distances per dsym: {dsym: set(d)}
    cand = {}
    mismatch = []
    for idx, e in enumerate(events):
        if e["kind"] != "MATCH" or e["out"] >= 0x400:
            continue
        n = e["out"] + len(win) - len(out)  # window index of out start
        n = e["out"] + 0x8000
        span = range(e["out"], e["out"] + e["n"])
        if any(p in lit_pos for p in span):
            continue  # span contains a literal → not pure zero-assumed
        got = bytes(out[e["out"]:e["out"] + e["n"]])
        if got == b"\x00" * e["n"]:
            continue  # already zero — no constraint
        # candidate distances pointing at len zeros in the window
        ds = set()
        for d in range(1, min(n, 0x8000) + 1):
            if win[n - d:n - d + e["n"]] == b"\x00" * e["n"]:
                ds.add(d)
        cand.setdefault(e["dsym"], set())
        if cand[e["dsym"]]:
            cand[e["dsym"]] &= ds
        else:
            cand[e["dsym"]] = set(ds)
        mismatch.append((e["out"], e["sym"], e["dsym"], e["n"],
                         e["deb"], got.hex(' ')))
    print(f"wrong zero-span matches in head: {len(mismatch)}")
    for m in mismatch[:12]:
        print(f"  out=0x{m[0]:04x} lsym={m[1]} dsym={m[2]} len={m[3]} "
              f"deb={m[4]} emitted={m[5]}")
    print("\nper-dsym candidate distance sets (zero-span constraints):")
    for dsym in sorted(cand):
        s = cand[dsym]
        rfc = DBASE[dsym]
        near = sorted(x for x in s if abs(x - rfc) <= 64)
        print(f"  dsym={dsym:2d} rfc_base={rfc:5d} |cands|={len(s):5d} "
              f"near_rfc={near[:12]}")


if __name__ == "__main__":
    main()
