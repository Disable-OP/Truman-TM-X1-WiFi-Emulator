#!/usr/bin/env python3
"""Head-branch DFS (§19.4): can ANY per-symbol distance assignment make the
eCos head executable?

For one accounting, the head region (out[0..0x200)) is covered by literals
(fixed bytes) and matches (parse-fixed spans, searched distances). The DFS
assigns distances to head matches in output order; after each match every
COMPLETED word (its last writer assigned) must classify as executable
(nop/harmless/regop/immop/mem/cop/special23). The search stops at a
non-trivial branch/jump word ("escape": the entry code leaves the head
before any fault) and reports the assignment.

Distance domain per match: 1..0x8000 (the 32 KiB window), source = live
window (preset zeros + output so far, live-window copy semantics).

Usage: python3 tools/fw_head_dfs.py <flash.bin> [--m2|--m4] [--max-nodes N]
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_first_corruption import word_class
from fw_engine_lz import Huffman, parse_header, BitReader, LBASE, LEXTRA, DBASE, DEXTRA

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
PRESET = 0x8000
WIN_LIMIT = 0x8000
OK = {"nop", "harmless", "regop", "immop", "mem", "cop", "special23"}


def parse_events(lith, disth, stream, le, de, out_cap):
    """Head-region events without distances (positions are parse-fixed)."""
    br = BitReader(stream, 1024)
    ev = []
    outpos = 0
    while (br.pos >> 3) < len(stream) and outpos < out_cap:
        try:
            s = lith.decode(br)
        except EOFError:
            break
        if s <= 255:
            ev.append(dict(kind="LIT", sym=s, out=outpos, n=1))
            outpos += 1
            continue
        if s == 256:
            continue
        li = s - 257
        if li >= 29:
            break
        eb = br.bits(LEXTRA[li]) if (le and LEXTRA[li]) else 0
        try:
            ds = disth.decode(br)
        except (EOFError, ValueError):
            break
        deb = br.bits(DEXTRA[ds]) if (de and DEXTRA[ds]) else 0
        ln = LBASE[li] + eb
        ev.append(dict(kind="MATCH", dsym=ds, out=outpos, n=ln))
        outpos += ln
    return ev


def run_dfs(events, max_nodes):
    matches = [e for e in events if e["kind"] == "MATCH"]
    head_end = 0x200
    # word ownership: last writer of each word in the head
    word_owner = {}
    for mi, m in enumerate(matches):
        for w in range(m["out"] // 4, (m["out"] + m["n"] - 1) // 4 + 1):
            if 4 * w + 3 < head_end:
                word_owner[w] = mi
    # literals: fixed bytes
    lit_bytes = {e["out"]: e["sym"] for e in events if e["kind"] == "LIT"}
    end = max(m["out"] + m["n"] for m in matches)
    escapes = []
    nodes = 0

    def words_completed_by(mi):
        return sorted(w for w, owner in word_owner.items() if owner == mi)

    def emit_match(out, p, n_len, d):
        """Emit match at p with distance d into out (live window)."""
        src_base = p - d  # in out coordinates (negative = preset)
        for j in range(n_len):
            i = src_base + j
            out[p + j] = out[i] if 0 <= i < p + j else 0
            # note: out[i] for i < p+j was either a literal, an earlier
            # match's byte, or a preset zero (represented as 0)
        # preset region semantics: out[i] with i < 0 = 0 (handled above)

    def dfs(mi, out):
        nonlocal nodes
        if escapes:
            return True
        if mi >= len(matches):
            return False
        if nodes >= max_nodes:
            return False
        m = matches[mi]
        p, n_len = m["out"], m["n"]
        for d in range(1, min(WIN_LIMIT, PRESET + p) + 1):
            nodes += 1
            if nodes >= max_nodes:
                break
            trial = bytearray(out)
            emit_match(trial, p, n_len, d)
            good = True
            for w in words_completed_by(mi):
                if 4 * w + 3 >= len(trial):
                    good = False
                    break
                word = int.from_bytes(trial[4 * w:4 * w + 4], "little")
                cls = word_class(word)
                if cls == "trap" or cls.startswith("RESERVED"):
                    good = False
                    break
                if cls in ("branch", "jump"):
                    op = word >> 26
                    trivial = (op in (0x01, 0x04, 0x05, 0x06, 0x07, 0x14,
                                      0x15, 0x16, 0x17)
                               and (word & 0xFFFF) == 0)
                    if trivial:
                        good = False
                        break
                    escapes.append(dict(d=d, word_off=4 * w,
                                        word=f"{word:08x}", cls=cls,
                                        depth=mi + 1, buf=bytes(trial[:0x80])))
                    return True
            if good:
                if dfs(mi + 1, trial):
                    return True
        return bool(escapes)

    # seed literals into the base buffer (positions < first match stay 0)
    base = bytearray(end)
    for pos, v in lit_bytes.items():
        if pos < end:
            base[pos] = v
    dfs(0, base)
    return escapes, nodes


def main():
    flash = open(sys.argv[1], "rb").read()
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    stream = cont[8:]
    _, litlens, distlens, _ = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)
    max_nodes = 2_000_000
    if "--max-nodes" in sys.argv:
        max_nodes = int(sys.argv[sys.argv.index("--max-nodes") + 1])
    models = [("m1", True, True), ("m2", True, False), ("m4", False, False)]
    if "--m2" in sys.argv:
        models = [("m2", True, False)]
    if "--m4" in sys.argv:
        models = [("m4", False, False)]
    for name, le, de in models:
        ev = parse_events(lith, disth, stream, le, de, 0x400)
        esc, nodes = run_dfs(ev, max_nodes)
        print(f"{name}: nodes={nodes} escapes={len(esc)}")
        for e in esc[:6]:
            print(f"   escape@word 0x{e['word_off']:03x} = {e['word']} "
                  f"({e['cls']}) via d={e['d']} at match depth {e['depth']}")
            print(f"     head: {e['buf'][:32].hex(' ')}")


if __name__ == "__main__":
    main()
