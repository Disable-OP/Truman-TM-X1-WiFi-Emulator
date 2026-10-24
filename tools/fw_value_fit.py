#!/usr/bin/env python3
"""Fit the PNG_DEC hardware length/distance value tables on the Truman
stream with the strict MIPS-validity oracle (interval-constrained beam).

State: per-code delta INTERVALS (lo,hi) + the decoded tail + validity.
Zero-preset copies and all-zero source spans bind distances to ranges
without branching, so the zero-dominated image stays cheap.
"""
import sys, time, struct, json

sys.path.insert(0, "/home/z/my-project/Truman-TM-X1-WiFi-Emulator/tools")
from fw_engine_lz import parse_header, symbol_walk, Huffman, LBASE, DBASE
from fw_ring_sweep import Validator

FLASH = ("/home/z/my-project/Truman-TM-X1-WiFi-Emulator/"
         "firmware/1506HV_512M_4M_SVT2_TRUMAN_TM_X1_WIFI_Internal_flashfile_20220902.bin")

DL_LO, DL_HI = -5, 5
DD_LO, DD_HI = -20, 20
UNB_L = (DL_LO, DL_HI)
UNB_D = (DD_LO, DD_HI)
BAD_TOL = 20
BEAM = 1200
TRUNC_KEEP = 0x8200   # >= max distance (32,743) + max copy (258)
TRUNC_AT = 0x8400


def load_events():
    flash = open(FLASH, "rb").read()
    cont = flash[0x1002C:0x1002C + 0x868C]
    assert cont[:2] == b"SP"
    stream = cont[8:]
    bfinal, litlens, distlens, hb = parse_header(stream)
    return symbol_walk(stream, 1024, Huffman(litlens), Huffman(distlens))


def wvalid(val, cache, word_bytes):
    r = cache.get(word_bytes)
    if r is None:
        r = val.word_valid(word_bytes)
        cache[word_bytes] = r
    return r


class St:
    __slots__ = ("lt", "dt", "tail", "n", "ok", "bad", "vpos", "oknz")

    def __init__(self, lt, dt, tail, n, ok, bad, vpos=0, oknz=0):
        self.lt = lt; self.dt = dt
        self.tail = tail; self.n = n
        self.ok = ok; self.bad = bad
        self.vpos = vpos   # absolute offset validated up to (word-aligned)
        self.oknz = oknz   # valid words that are not all-zero (evidence)

    def key(self):
        return (self.lt, self.dt)


def step_words(st, val, cache):
    """Validate every completed word in the new tail region."""
    start = (st.n - len(st.tail))
    first = max(st.vpos, (start + 3) // 4 * 4)
    pos = first - start
    while pos + 4 <= len(st.tail):
        if wvalid(val, cache, bytes(st.tail[pos:pos+4])):
            st.ok += 1; st.bad = 0
            if st.tail[pos:pos+4] != b"\x00\x00\x00\x00":
                st.oknz += 1
        else:
            st.bad += 1
            if st.bad > BAD_TOL:
                return False
        pos += 4
    st.vpos = start + (pos if pos + 4 <= len(st.tail) else len(st.tail))
    return True


def main():
    val = Validator(open(FLASH, "rb").read())
    cache = {}
    events = load_events()
    print(f"events: {len(events)}", flush=True)

    states = [St((UNB_L,) * 29, (UNB_D,) * 30, bytearray(), 0, 0, 0)]
    t0 = time.time()
    mi = 0
    for e in events:
        if e["kind"] == "LIT":
            for st in states:
                st.tail.append(e["sym"]); st.n += 1
                if len(st.tail) > TRUNC_AT:
                    del st.tail[:len(st.tail) - TRUNC_KEEP]
            nb = []
            for st in states:
                if step_words(st, val, cache):
                    nb.append(st)
            states = nb
        elif e["kind"] == "MATCH":
            mi += 1
            li = e["sym"] - 257
            Lraw = LBASE[li] + e["eb"]
            draw = DBASE[e["dsym"]] + e["deb"]
            ds = e["dsym"]
            nbeam = []
            for st in states:
                n = st.n
                ltl = st.lt[li]
                dtl = st.dt[ds]
                dl_lo, dl_hi = ltl
                dd_lo, dd_hi = dtl
                # effective absolute distance range
                D_min = max(1, draw + dd_lo)
                D_max = draw + dd_hi
                for dl in range(max(DL_LO, dl_lo), min(DL_HI, dl_hi) + 1):
                    L2 = Lraw + dl
                    if L2 < 1:
                        continue
                    # enumerate candidate dd values, but collapse when the
                    # whole copy region is zeros for every D2 in range
                    dd_vals = range(max(DD_LO, dd_lo), min(DD_HI, dd_hi) + 1)
                    D2s = [draw + dd for dd in dd_vals]
                    D2s = [d for d in D2s if d >= 1]
                    if not D2s:
                        continue
                    lo_d, hi_d = min(D2s), max(D2s)
                    # source span across all D2: positions n-hi_d .. n-lo_d+L2-1
                    tail = st.tail
                    tlen = len(tail)
                    start_idx = n - hi_d           # absolute
                    end_idx = n - lo_d + L2        # absolute (exclusive)
                    # map to tail slice
                    s_i = start_idx - (n - tlen)
                    e_i = end_idx - (n - tlen)
                    all_zero = False
                    if e_i <= 0:
                        all_zero = True            # whole span = preset zeros
                    elif s_i >= 0:
                        seg = tail[s_i:e_i]
                        all_zero = not any(seg)
                    if all_zero:
                        # every D2 in range copies zeros; bind dd to the range
                        nlo = max(dd_lo, DD_LO, 1 - draw)
                        nhi = min(dd_hi, DD_HI, hi_d - draw)
                        if nlo > nhi:
                            continue
                        st2 = St(st.lt, st.dt, st.tail, st.n, st.ok, st.bad, st.vpos, st.oknz)
                        nl = list(st2.lt); nl[li] = (dl, dl); st2.lt = tuple(nl)
                        nd = list(st2.dt); nd[ds] = (nlo, nhi); st2.dt = tuple(nd)
                        st2.tail = bytearray(st2.tail)
                        for _ in range(L2):
                            st2.tail.append(0); st2.n += 1
                            if len(st2.tail) > TRUNC_AT:
                                del st2.tail[:len(st2.tail) - TRUNC_KEEP]
                        if step_words(st2, val, cache):
                            nbeam.append(st2)
                    else:
                        for D2 in D2s:
                            st2 = St(st.lt, st.dt, st.tail, st.n, st.ok, st.bad, st.vpos, st.oknz)
                            nl = list(st2.lt); nl[li] = (dl, dl); st2.lt = tuple(nl)
                            nd = list(st2.dt)
                            nd[ds] = (D2 - draw, D2 - draw); st2.dt = tuple(nd)
                            st2.tail = bytearray(st2.tail)
                            n = len(st2.tail)
                            for i in range(L2):
                                if D2 <= st2.n:
                                    idx = len(st2.tail) - D2
                                    b = st2.tail[idx] if idx >= 0 else 0
                                else:
                                    b = 0
                                st2.tail.append(b); st2.n += 1
                                if len(st2.tail) > TRUNC_AT:
                                    del st2.tail[:len(st2.tail) - TRUNC_KEEP]
                            if step_words(st2, val, cache):
                                nbeam.append(st2)
            # dedupe by intervals, keep best
            seen = {}
            for st in nbeam:
                k = st.key()
                p = seen.get(k)
                if p is None or (st.ok, st.n) > (p.ok, p.n):
                    seen[k] = st
            cand = list(seen.values())
            if len(cand) > BEAM:
                cand.sort(key=lambda s: (-s.oknz, -s.ok / max(1, s.n // 4 or 1), -s.n))
                cand = cand[:BEAM]
            states = cand
            if mi % 200 == 0 and states:
                best = max(states, key=lambda s: (s.ok, s.n))
                lb = sum(1 for a in best.lt if a != UNB_L)
                db = sum(1 for a in best.dt if a != UNB_D)
                print(f"  m{mi}: states={len(states)} ok={best.ok} "
                      f"words={best.n//4} bad={best.bad} bound={lb}L/{db}D "
                      f"({time.time()-t0:.0f}s)", flush=True)
            if not states:
                print(f"DEAD at match {mi}")
                return

    print(f"\nfinal: {len(states)} states ({time.time()-t0:.0f}s)")
    states.sort(key=lambda s: (-s.oknz, -s.ok / max(1, s.n // 4 or 1), -s.n))
    for rank, st in enumerate(states[:4]):
        print(f"#{rank}: ok={st.ok}/{st.n//4} "
              f"({100*st.ok/max(1,st.n//4):.1f}%) n={st.n}")
        print("  L:", {i+257: d for i, d in enumerate(st.lt) if d != UNB_L})
        print("  D:", {i: d for i, d in enumerate(st.dt) if d != UNB_D})
    st = states[0]
    json.dump({"lt": [list(x) for x in st.lt], "dt": [list(x) for x in st.dt],
               "n": st.n, "ok": st.ok},
              open("/home/z/my-project/scripts/truman_fit.json", "w"))


if __name__ == "__main__":
    main()
