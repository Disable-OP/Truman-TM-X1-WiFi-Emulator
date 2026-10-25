#!/usr/bin/env python3
"""The vendor vector's structural leak (docs/firmware-analysis.md §39).

The SDK's own test container (`sphe-cmd_sp_png_dec.c`, `ZlibBuf`, scramble
key 0xa6fc, height 9 -> 9216 bytes) is vendor-authored `sp_gzip` output for
the SAME hardware value tables as the Truman stream. This tool extracts it
from the archived vendor source, walks the base-independent parse, and
prints the evidence blocks §39 documents:

  1. EOB absence: no start bit (1-bit scan over [header_end, 4096)) makes
     the canonical-table parse reach the end-of-block symbol — the vector's
     decode is ended by the row-count completion, not by EOB.
  2. The literal backbone: the vector's literals are ~99% hex characters
     and the match-produced gaps (preset copies under RFC distances) sit
     between them — the pattern is the backbone plus gap contents.
  3. The engine-semantics decode (32 KiB zero preset): completes at every
     candidate start bit, over-running the 9216 anchor by 350-570 bytes.
  4. The joint two-stream length-anchor system: Truman (pinned 79,872
     anchor) and the vector (9216, no-waste hypothesis) require OPPOSITE
     total length deltas concentrated on the same dominant class — the
     per-extra-bit-class uniform family has ZERO joint survivors.

Usage: python3 tools/fw_vector_probe.py
Deterministic; no network, no firmware write access.
"""
import re
import struct
import sys
from collections import Counter
from itertools import product

sys.path.insert(0, "/home/z/my-project/Truman-TM-X1-WiFi-Emulator/tools")
from fw_engine_lz import (  # noqa: E402
    DBASE, LBASE, LEXTRA, Huffman, parse_header, symbol_walk,
)

HERE = "/home/z/my-project/Truman-TM-X1-WiFi-Emulator"
REF = f"{HERE}/tools/reference/sphe-cmd_sp_png_dec.c"
FLASH = (f"{HERE}/firmware/1506HV_512M_4M_SVT2_TRUMAN_TM_X1_WIFI_"
         "Internal_flashfile_20220902.bin")

HEXCH = frozenset(b"0123456789abcdef")
TRUMAN_RFC_LEN_PINNED = 75408  # Rust engine decode (§36.4/AI milestone)
TRUMAN_ANCHOR = 79872          # 78 rows x 1024
VECTOR_ANCHOR = 9216           # 9 rows x 1024 (SP header chunk count)

# RFC length codes grouped by extra-bit width; code 28 (fixed 258) is E0.
def class_of(code):
    return 0 if code == 28 else (code - 8) // 4 + 1 if code >= 8 else 0


def extract_vector():
    src = open(REF).read()
    m = re.search(r"ZlibBuf\[16\*234\] = \{(.*?)\};", src, re.S)
    if not m:
        raise SystemExit("ZlibBuf not found in archived vendor file")
    return bytes(int(h, 16) for h in re.findall(r"0x([0-9a-fA-F]{2})",
                                                m.group(1)))


def lit_class(b):
    if b in HEXCH:
        return "hex"
    return {0x0A: "nl", 0x0D: "cr", 0: "nul"}.get(b, "other")


def engine_decode(events):
    """Engine semantics: window = 32 KiB zero preset + output."""
    window = bytearray(32768)
    out = bytearray()
    for e in events:
        if e["kind"] == "LIT":
            window.append(e["sym"])
            out.append(e["sym"])
        elif e["kind"] == "MATCH":
            ln = LBASE[e["sym"] - 257] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            n = len(window)
            if d > n:
                return out, (e["bit"], d)
            for i in range(ln):
                b = window[n - d + i]
                window.append(b)
                out.append(b)
        elif e["kind"].startswith("ERR"):
            return out, (e["bit"], None)
    return out, None


def walk(stream, start):
    bf, ll, dl, hb = parse_header(stream)
    return symbol_walk(stream, start, Huffman(ll), Huffman(dl))


def eob_scan(stream, lo, hi):
    bf, ll, dl, hb = parse_header(stream)
    lith, disth = Huffman(ll), Huffman(dl)
    hits = 0
    for sb in range(lo, min(hi, len(stream) * 8 - 64)):
        try:
            events = symbol_walk(stream, sb, lith, disth)
        except Exception:  # noqa: BLE001
            continue
        if any(e["kind"] == "EOB" for e in events):
            hits += 1
    return hits


def census(events):
    kinds = Counter(e["kind"] for e in events)
    lits = [e["sym"] for e in events if e["kind"] == "LIT"]
    lc = Counter(lit_class(b) for b in lits)
    n_len = Counter()
    rfc_len = len(lits)
    gaps = []
    cur = 0
    for e in events:
        if e["kind"] == "LIT":
            if cur:
                gaps.append(cur)
            cur = 0
        elif e["kind"] == "MATCH":
            k = e["sym"] - 257
            n_len[k] += 1
            ln = LBASE[k] + e["eb"]
            rfc_len += ln
            cur += ln
    if cur:
        gaps.append(cur)
    return kinds, lc, n_len, rfc_len, Counter(gaps), lits


def class_weights(n_len):
    w = [0] * 6
    for c, k in n_len.items():
        w[class_of(c)] += k
    return w


def joint_sweep(n_v, target_v, n_t, target_t, verbose=True):
    """Per-extra-bit-class uniform deltas vs BOTH anchors."""
    w_v, w_t = class_weights(n_v), class_weights(n_t)
    lo = [0] * 6
    hi = [8] * 6
    for cls in range(6):
        used = [LBASE[c] for c in range(29)
                if class_of(c) == cls and n_v.get(c, 0) + n_t.get(c, 0) > 0]
        if used:
            lo[cls] = 3 - min(used)
    free = sorted(range(6), key=lambda k: hi[k] - lo[k])[:4]
    solve = [k for k in range(6) if k not in free]
    s1, s2 = solve
    survivors = 0
    for combo in product(*[range(lo[k], hi[k] + 1) for k in free]):
        d = [None] * 6
        for k, v in zip(free, combo):
            d[k] = v
        b1 = target_v - sum(w_v[k] * d[k] for k in free)
        b2 = target_t - sum(w_t[k] * d[k] for k in free)
        det = w_v[s1] * w_t[s2] - w_v[s2] * w_t[s1]
        if det == 0:
            continue
        x1 = (b1 * w_t[s2] - b2 * w_v[s2]) / det
        x2 = (w_v[s1] * b2 - w_t[s1] * b1) / det
        if x1 != int(x1) or x2 != int(x2):
            continue
        x1, x2 = int(x1), int(x2)
        if not (lo[s1] <= x1 <= hi[s1] and lo[s2] <= x2 <= hi[s2]):
            continue
        d[s1], d[s2] = x1, x2
        if all(LBASE[c] + d[class_of(c)] >= 3
               for c in set(n_v) | set(n_t)):
            survivors += 1
            if verbose and survivors <= 5:
                print(f"    survivor: deltas={d}")
    return survivors


def main():
    buf = extract_vector()
    print(f"vector container: {len(buf)} bytes, head "
          f"{buf[:8].hex(' ')}")
    nchunks = struct.unpack_from(">H", buf, 2)[0]
    print(f"SP header: chunks={nchunks} stream_len="
          f"0x{struct.unpack_from('>I', buf, 4)[0]:x} "
          f"expected_out={nchunks * 1024}")
    vstream = buf[8:]
    bf, ll, dl, hb = parse_header(vstream)
    print(f"vector header: bfinal={bf} hlit={len(ll)} hdist={len(dl)} "
          f"header_bits={hb}")

    print("\n[1] EOB scan (canonical tables, 1-bit steps):")
    hits = eob_scan(vstream, hb, 4096)
    print(f"    start bits reaching EOB in [{hb}, 4096): {hits}")

    flash = open(FLASH, "rb").read()
    cont = flash[0x1002C:0x1002C + 0x868C]
    assert cont[:2] == b"SP"
    tstream = cont[8:]
    tev = walk(tstream, 1024)
    tkinds, tlc, tn, trfc, tgaps, _ = census(tev)
    n_t, rfc_t = tn, TRUMAN_RFC_LEN_PINNED
    print(f"truman start=1024: kinds={dict(tkinds)} rfc_len={trfc} "
          f"(pinned {TRUMAN_RFC_LEN_PINNED})")

    for start in (320, 1024, 1088):
        events = walk(vstream, start)
        kinds, lc, n_len, rfc_len, gaps, lits = census(events)
        out, bad = engine_decode(events)
        print(f"\n[2/3] vector start={start}: kinds={dict(kinds)}")
        print(f"    literal composition: {dict(lc)} (n={len(lits)}) "
              f"-> backbone text: {bytes(lits[:40])!r}...")
        print(f"    gaps between fragments: total={sum(gaps.values())} "
              f"distinct={len(gaps)} top={gaps.most_common(5)}")
        print(f"    engine-semantics decode: len={len(out)} "
              f"(anchor {VECTOR_ANCHOR}) bad={bad}")
        print(f"    RFC over-run: {rfc_len - VECTOR_ANCHOR:+d}")
        print(f"    len-code census: {sorted(n_len.items())}")
        print(f"\n[4] joint anchor sweep (start={start}):", flush=True)
        target_v = VECTOR_ANCHOR - rfc_len
        target_t = TRUMAN_ANCHOR - rfc_t
        print(f"    vector: {target_v:+d} weights={class_weights(n_len)}")
        print(f"    truman: {target_t:+d} weights={class_weights(n_t)}")
        surv = joint_sweep(n_len, target_v, n_t, target_t)
        print(f"    joint survivors: {surv}")


if __name__ == "__main__":
    main()
