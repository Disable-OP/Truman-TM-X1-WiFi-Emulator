#!/usr/bin/env python3
"""Combined length+distance value semantics sweep — the §16.3 surviving
class, tested jointly for the first time (Milestone X, §26).

Every prior length search ran with RFC distances (§15.6's 64 exact-size
solutions and §18.2's 477-seed descent, all ~50% strict validity), and
every prior distance probe ran with RFC lengths at ±16 global offsets,
ring aliases or word-aligned sources (§15.6, §17.3). The joint class —
"combined length+distance value semantics with position shift" — is what
§25.5 left as the only in-silico hypothesis after the framing family was
refuted. This tool sweeps it exhaustively on the evidence-pinned exact
parse (§14.1: same Huffman codes, same extra-bit counts; only the VALUE
of a symbol can differ, never the bits consumed).

Length families (f: len_sym -> base, len = f(ls) + eb):
  L0  RFC 1951                                  (baseline control)
  L1  the three §16.3 exact-count predicates    (+2 on exactly 2,237
      matches each => size exactly 79,872)        matches, RFC-population)
  L2  group-uniform offsets g0..g5 in 0..12 with the exact size equation
      sum(count[li] * g_group(li)) == 4,474      (reproduces §15.6's 64)
  L3  code-shifted ladder f(ls) = LBASE[clamp(ls+d)], d in {-2,-1,+1,+2}

Distance families (g: dist_sym -> base, dist = g(ds) + deb; bit counts
stayed pinned, so only the base table can move):
  D0  RFC 1951                                  (baseline control)
  D1  global offsets k in {+-1,..,+-8192}       (extends §17.3's ±16)
  D2  code-shifted g(ds) = DBASE[clamp(ds+d)], d in {-2,-1,+1,+2}

Scoring (pre-registered, mirroring §25.4's criteria):
  size     output length within one row (1024) of 79,872
  head     first 16 words: every word strictly decodable by capstone,
           no COP0-with-reserved-rs (the 24KEc reserved-exception class
           that kills the RFC head at word 0), no BREAK
  content  strict capstone validity >= 90% (real code measures 91-95%),
           plus ASCII runs >= 6 and eCos marker presence, reported

Usage: python3 tools/fw_combined_semantics.py <flash.bin> [--top N]
"""
import argparse
import math
import sys
from collections import Counter

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import LBASE, LEXTRA, DBASE, DEXTRA
from fw_ring_sweep import Validator, load_events

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
IMG = 78 * 1024
WINDOW = 0x8000
EOB = 256

# LEXTRA groups (the six extra-bit values + the 258-code)
GROUP_OF = {0: 0, 1: 1, 2: 2, 3: 3, 4: 4, 5: 5}


def decode_with(events, lbase_of, dbase_of, pred_extra=None, cap=IMG + 2048):
    """Decode the pinned symbol path with value maps.

    lbase_of: function(len_sym) -> base length
    dbase_of: function(dist_sym) -> base distance
    pred_extra: optional function(event) -> extra bytes (the §16.3
      predicate families: +2 on the RFC-population members)
    Returns the output bytearray (truncated at cap) and the final size.
    """
    out = bytearray()
    window = bytearray([0]) * WINDOW  # driver zero-fills the band (§13.1)
    for e in events:
        k = e["kind"]
        if k == "LIT":
            window.append(e["sym"])
            out.append(e["sym"])
        elif k == "MATCH":
            ls, ds = e["sym"], e["dsym"]
            ln = lbase_of(ls) + e["eb"]
            if pred_extra is not None:
                ln += pred_extra(e)
            d = dbase_of(ds) + e["deb"]
            if d < 1:
                d = 1
            n = len(window)
            for i in range(ln):
                p = n - d + i
                # self-overlapping copies read freshly appended bytes
                # (RFC 1951 run-length semantics; matches fw_ring_sweep)
                b = window[p] if 0 <= p < len(window) else 0
                window.append(b)
                out.append(b)
                if len(out) >= cap:
                    return out, len(out)
        elif k == EOB or k == "EOB":
            continue  # continuous walk: mid-stream EOBs are no-ops (§25.3)
        else:
            break
    return out, len(out)


def head_verdict(out):
    """Static head oracle on the first 16 words (§19.1 shape)."""
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_MIPS, capstone.CS_MODE_MIPS32 |
                     capstone.CS_MODE_LITTLE_ENDIAN)
    words = [int.from_bytes(out[i * 4:i * 4 + 4], "little") for i in range(16)]
    for w in words:
        op = (w >> 26) & 0x3F
        if op == 0:  # SPECIAL
            funct = w & 0x3F
            if funct == 0x0D:
                return "BREAK"
            continue
        if op == 0x10:  # COP0
            rs = (w >> 21) & 0x1F
            if rs >= 0x10:
                return f"COP0-reserved-rs-0x{rs:X}"
            continue
        ok = False
        for _ in md.disasm(w.to_bytes(4, "little"), 0):
            ok = True
            break
        if not ok:
            return "UNDECODABLE"
    return "OK"


def ascii_stats(out):
    runs = 0
    cur = 0
    for b in out:
        if 32 <= b < 127:
            cur += 1
            if cur == 6:
                runs += 1
        else:
            cur = 0
    markers = [m for m in (b"eCos", b"RedBoot", b"SPHE", b"1506", b"UART",
                           b"Banner") if m in out]
    return runs, markers


def length_families(events):
    """Yield (name, lbase_of, pred_extra, exact_size)."""
    matches = [e for e in events if e["kind"] == "MATCH"]
    n_lit = sum(1 for e in events if e["kind"] == "LIT")
    rfc_total = n_lit + sum(LBASE[e["sym"] - 257] + e["eb"] for e in matches)
    delta = IMG - rfc_total

    yield "L0:rfc", (lambda ls: LBASE[ls - 257]), None, False

    # L1: the three §16.3 exact-count predicates (RFC populations)
    if delta:
        preds = {
            "dist<96": lambda e: (DBASE[e["dsym"]] + e["deb"]) < 96,
            "dist>=3485": lambda e: (DBASE[e["dsym"]] + e["deb"]) >= 3485,
            "len>=5&dist<3073":
                lambda e: (LBASE[e["sym"] - 257] + e["eb"]) >= 5 and
                          (DBASE[e["dsym"]] + e["deb"]) < 3073,
        }
        for name, p in preds.items():
            cnt = sum(1 for e in matches if p(e))
            if cnt and delta % cnt == 0:
                adj = delta // cnt
                if adj in (1, 2, 4):
                    yield (f"L1:{name}(+{adj})",
                           (lambda ls: LBASE[ls - 257]),
                           (lambda e, _p=p: adj if _p(e) else 0), True)

    # L2: group-uniform offsets, exact size equation
    cnt_by_code = Counter(e["sym"] for e in matches)
    group_cnt = [0] * 6
    for ls, c in cnt_by_code.items():
        g = GROUP_OF[LEXTRA[ls - 257]] if ls != 285 else 0
        group_cnt[g] += c
    sols = []
    g0, g1, g2, g3, g4 = 0, 0, 0, 0, 0
    c = group_cnt
    for g0 in range(13):
        for g1 in range(13):
            s01 = c[0] * g0 + c[1] * g1
            if s01 > delta:
                break
            for g2 in range(13):
                s012 = s01 + c[2] * g2
                if s012 > delta:
                    break
                for g3 in range(13):
                    s0123 = s012 + c[3] * g3
                    if s0123 > delta:
                        break
                    for g4 in range(13):
                        s01234 = s0123 + c[4] * g4
                        if s01234 > delta:
                            break
                        rem = delta - s01234
                        if c[5] and rem % c[5] == 0 and 0 <= rem // c[5] <= 12:
                            sols.append((g0, g1, g2, g3, g4, rem // c[5]))
    gs = {tuple(s) for s in sols}
    for g in sorted(gs):
        tbl = {ls: LBASE[ls - 257] +
               g[GROUP_OF[LEXTRA[ls - 257]] if ls != 285 else 0]
               for ls in range(257, 286)}
        yield f"L2:g{g}", (lambda ls, _t=tbl: _t[ls]), None, True

    # L3: code-shifted ladder (keeps the extra-bit counts of the code)
    for d in (-2, -1, 1, 2):
        tbl = {ls: LBASE[min(28, max(0, ls - 257 + d))] for ls in range(257, 286)}
        yield f"L3:shift{d:+d}", (lambda ls, _t=tbl: _t[ls]), None, False


def distance_families():
    """Yield (name, dbase_of)."""
    yield "D0:rfc", (lambda ds: DBASE[ds])
    for k in (1, 2, 3, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096,
              8192, -1, -2, -3, -4, -8, -16, -32, -64, -128, -256, -512,
              -1024, -2048, -4096, -8192):
        yield (f"D1:k{k:+d}", (lambda ds, _k=k: max(1, DBASE[ds] + _k)))
    for d in (-2, -1, 1, 2):
        tbl = {ds: DBASE[min(29, max(0, ds + d))] for ds in range(30)}
        yield f"D2:shift{d:+d}", (lambda ds, _t=tbl: _t[ds])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("flash")
    ap.add_argument("--top", type=int, default=12)
    args = ap.parse_args()

    flash = open(args.flash, "rb").read()
    events = load_events(flash)
    val = Validator(flash)

    n_lf = 0
    results = []
    exact_size_combos = 0
    head_ok = 0
    for lf_name, lbase_of, pred, exact in length_families(events):
        n_lf += 1
        for df_name, dbase_of in distance_families():
            out, size = decode_with(events, lbase_of, dbase_of, pred)
            size_ok = abs(size - IMG) <= 1024
            if exact:
                exact_size_combos += 1
            head = head_verdict(out)
            if head == "OK":
                head_ok += 1
            s, _ = val.strict(out)
            strict = s * 100.0
            runs, markers = ascii_stats(out)
            results.append((lf_name, df_name, size, head, strict, runs,
                            markers))
    results.sort(key=lambda r: -r[4])

    print(f"length families: {n_lf}")
    print(f"total combinations scored: {len(results)} "
          f"(exact-size x any-distance: {exact_size_combos})")
    base = [r for r in results if r[0] == "L0:rfc" and r[1] == "D0:rfc"][0]
    print(f"control L0:rfc x D0:rfc: out={base[2]} head={base[3]} "
          f"strict={base[4]:.1f}% runs={base[5]} markers={base[6]}")
    print(f"head-OK among size-qualified: {head_ok}")
    best_strict = max(r[4] for r in results)
    print(f"best strict validity anywhere: {best_strict:.1f}%")
    print(f"\ntop {args.top} by strict validity:")
    for lf, df, size, head, strict, runs, markers in results[:args.top]:
        print(f"  {lf:26s} {df:12s} out={size:6d} head={head:22s} "
              f"strict={strict:5.1f}% runs>=6:{runs:3d} markers={markers}")
    survivors = [r for r in results
                 if r[3] == "OK" and r[4] >= 90.0
                 and abs(r[2] - IMG) <= 1024]
    print(f"\nsurvivors (head OK and strict >= 90%): {len(survivors)}")
    for lf, df, size, head, strict, runs, markers in survivors[:20]:
        print(f"  {lf} x {df} out={size} strict={strict:.1f}% "
              f"runs={runs} markers={markers}")


if __name__ == "__main__":
    main()
