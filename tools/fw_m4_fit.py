#!/usr/bin/env python3
"""M4 accounting fit on the SDK test vector (interval-constrained beam).

m4: the engine consumes ONLY Huffman codes; lengths = 29 fixed ROM
values, distances = 30 fixed ROM values. State = per-code (lo,hi)
intervals; copies whose whole source span is zeros (or the zero preset)
collapse into interval constraints without branching. Alphabet: the
pattern is sparse text (hex chars/newlines) in a zero sea.
"""
import re, sys, time

sys.path.insert(0, "/home/z/my-project/Truman-TM-X1-WiFi-Emulator/tools")
from fw_engine_lz import parse_header, Huffman

ALLOW = frozenset(b"0123456789abcdefABCDEF") | {0x0A, 0x0D, 0x20, 0x00}
LO, HI = 1, 64
UNB = (LO, HI)
BEAM = 2000
TARGET = 9216

class BR:
    __slots__ = ("b", "p")
    def __init__(s, b, p=0): s.b = b; s.p = p
    def bit(s):
        if s.p >= len(s.b) * 8: raise EOFError
        v = (s.b[s.p >> 3] >> (s.p & 7)) & 1; s.p += 1; return v

def decode_sym(br, huff):
    return huff.decode(br)

class St:
    __slots__ = ("li", "di", "tail", "n")
    def __init__(s, li, di, tail, n):
        s.li = li; s.di = di; s.tail = tail; s.n = n
    def key(s): return (s.li, s.di)

def main():
    src = open("/tmp/sunplus_cmd_sp_png_dec.c").read()
    m = re.search(r"ZlibBuf\[16\*234\] = \{(.*?)\};", src, re.S)
    buf = bytes(int(h, 16) for h in re.findall(r"0x([0-9a-fA-F]{2})", m.group(1)))
    stream = buf[8:]
    bfinal, litlens, distlens, hb = parse_header(stream)
    print(f"header: hlit={len(litlens)} hdist={len(distlens)}", flush=True)
    lith = Huffman(litlens); disth = Huffman(distlens)

    states = [St((UNB,)*29, (UNB,)*30, bytearray(), 0)]
    bitpos = 0
    t0 = time.time(); mi = 0
    while True:
        br = BR(stream, bitpos)
        try:
            s = decode_sym(br, lith)
        except EOFError:
            print(f"EOF at bit {br.p}, out={states[0].n}")
            break
        except ValueError:
            print(f"bad litlen code at bit {br.p}"); return
        if s <= 255:
            for st in states:
                st.tail.append(s); st.n += 1
            bitpos = br.p
            if states[0].n > TARGET + 64:
                print("size overflow"); break
            continue
        if s == 256:
            print(f"EOB at bit {br.p}, out={states[0].n} (expected {TARGET})")
            bitpos = br.p
            break
        li_idx = s - 257
        br2 = BR(stream, br.p)
        try:
            ds = decode_sym(br2, disth)
        except EOFError:
            print("EOF at dist"); break
        except ValueError:
            print(f"bad dist code at bit {br2.p}"); return
        nbit = br2.p
        mi += 1
        nstates = []
        for st in states:
            if st.n > TARGET:
                continue
            llo, lhi = st.li[li_idx]
            dlo, dhi = st.di[ds]
            n = st.n
            tail = st.tail
            for lv in range(max(1, llo), lhi + 1):
                # possible absolute source span for dv in [max(1,dlo)..dhi]
                dmin = max(1, dlo)
                if dmin > HI:
                    continue
                # zero-collapse check: is every possible copied byte 0?
                # source positions: n+i-dv for i in [0,lv), dv in [dmin,dhi]
                lo_src = n - dhi           # earliest source position
                hi_src = n - dmin + lv     # latest source end (exclusive)
                all_zero = False
                if hi_src <= 0:
                    all_zero = True
                elif lo_src >= 0 and hi_src <= n:
                    seg = tail[lo_src:hi_src]
                    all_zero = not any(seg)
                if all_zero:
                    nli = list(st.li); nli[li_idx] = (lv, lv)
                    ndi = list(st.di); ndi[ds] = (max(dlo, 1), dhi)
                    st2 = St(tuple(nli), tuple(ndi), bytearray(tail) + bytes(lv), n + lv)
                    nstates.append(st2)
                    continue
                for dv in range(dmin, dhi + 1):
                    ok = True; add = bytearray(); w2 = bytearray(tail)
                    for i in range(lv):
                        if dv <= n + i:
                            idx = n + i - dv
                            b = w2[idx] if idx >= 0 else 0
                        else:
                            b = 0
                        if b not in ALLOW:
                            ok = False; break
                        add.append(b); w2.append(b)
                    if not ok:
                        continue
                    nli = list(st.li); nli[li_idx] = (lv, lv)
                    ndi = list(st.di); ndi[ds] = (dv, dv)
                    nstates.append(St(tuple(nli), tuple(ndi), w2, n + lv))
        seen = {}
        for st in nstates:
            k = st.key()
            p = seen.get(k)
            if p is None or st.n > p.n:
                seen[k] = st
        cand = list(seen.values())
        if len(cand) > BEAM:
            cand.sort(key=lambda s: -s.n)
            cand = cand[:BEAM]
        states = cand
        if mi % 100 == 0:
            print(f"  m{mi}: states={len(states)} out={states[0].n if states else -1} ({time.time()-t0:.0f}s)", flush=True)
        if not states:
            print(f"DEAD at match {mi}")
            return

    print(f"\nend: states={len(states)} out={states[0].n if states else 0} ({time.time()-t0:.0f}s)")
    if states:
        st = states[0]
        o = st.tail if st.n <= len(st.tail) else st.tail
        for i in range(0, min(160, len(o)), 16):
            row = o[i:i+16]
            txt = "".join(chr(b) if 32 <= b < 127 else ("." if b else "·") for b in row)
            print(f"{i:04x}: " + " ".join(f"{b:02x}" for b in row) + "  " + txt)
        print("L intervals:", {i+257: v for i, v in enumerate(st.li)})
        print("D intervals:", {i: v for i, v in enumerate(st.di)})


if __name__ == "__main__":
    main()
