#!/usr/bin/env python3
"""Position-shift decode experiment (Milestone N, follows §16.3).

§16.3 found three predicates whose match count is exactly 2,237 (so a
uniform +2 emission per qualifying match reproduces the programmed output
size 79,872). The counts were never tested by DECODING: this tool decodes
the verified symbol stream under each candidate semantics and scores the
result with the calibrated strict validity oracle.

  dist<96 | dist>=3485 | len>=5&dist<3073   -> 2,237 each (adjustment +2)
  (dist<96 | dist>=3485)                    -> 4,474      (adjustment +1)
  dist<3073 / dist<3485 single thresholds   -> counted below

The three +2 predicates are pairwise disjoint in part (dist<96 vs
dist>=3485), so at most one of them can be the real rule; equal counts
must then be structure, not mechanism.

Copy semantics for the extra bytes: the match copy continues from the
same window source (hardware copies L+adj bytes), i.e. the emission is
window[n-d : n-d+L+adj].

Usage: python3 tools/fw_shift_decode.py <flash.bin> [variant]
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import Huffman, LBASE, DBASE, parse_header, symbol_walk
from fw_ring_sweep import Validator

CONTAINER_OFF = 0x1002C
CONTAINER_LEN = 0x868C
TARGET_TOTAL = 79_872
WINDOW = 0x8000


def load_matches(flash):
    cont = flash[CONTAINER_OFF:CONTAINER_OFF + CONTAINER_LEN]
    assert cont[:2] == b"SP"
    stream = cont[8:]
    bfinal, litlens, distlens, hb = parse_header(stream)
    lith, disth = Huffman(litlens), Huffman(distlens)
    events = symbol_walk(stream, 1024, lith, disth)
    return events


def shift_decode(events, pred, adj):
    """Decode with match emission L+adj when pred(len, dist) is true.

    pred takes the RFC-decoded (len, dist). Extra bytes continue the
    copy from the same live-window source. Returns linear output.
    """
    window = bytearray(WINDOW)  # driver zero-fills the 32 KiB window
    out = bytearray()
    for e in events:
        k = e["kind"]
        if k == "LIT":
            window.append(e["sym"])
            out.append(e["sym"])
        elif k == "MATCH":
            li = e["sym"] - 257
            ln = LBASE[li] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            n = len(window)
            if d > n:
                raise ValueError(f"dist {d} beyond window at out {len(out)}")
            L = ln + adj if pred(ln, d) else ln
            for i in range(L):
                b = window[n - d + i]
                window.append(b)
                out.append(b)
        # EOB / EOF: marker, no output (engine model semantics)
    return out


def main():
    flash = open(sys.argv[1], "rb").read()
    events = load_matches(flash)
    matches = []
    for e in events:
        if e["kind"] == "MATCH":
            ln = LBASE[e["sym"] - 257] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            matches.append((ln, d, e["out"]))
    n_lit = sum(1 for e in events if e["kind"] == "LIT")
    rfc = n_lit + sum(m[0] for m in matches)
    print(f"matches={len(matches)} literals={n_lit} rfc={rfc} "
          f"delta={TARGET_TOTAL - rfc}")

    # dist-edge counts (predicate candidates the §16.3 search could not
    # hit because it only enumerated len-conjoined thresholds)
    def c(pred):
        return sum(1 for m in matches if pred(m[0], m[1]))
    print(f"dist<96={c(lambda l, d: d < 96)} "
          f"dist<3073={c(lambda l, d: d < 3073)} "
          f"dist<3485={c(lambda l, d: d < 3485)} "
          f"dist>=3485={c(lambda l, d: d >= 3485)} "
          f"len>=5&dist<3073={c(lambda l, d: l >= 5 and d < 3073)} "
          f"len<5&dist<3073={c(lambda l, d: l < 5 and d < 3073)}")

    val = Validator(flash)
    variants = {
        "A dist<96 +2": (lambda l, d: d < 96, 2),
        "B dist>=3485 +2": (lambda l, d: d >= 3485, 2),
        "C len>=5&dist<3073 +2": (lambda l, d: l >= 5 and d < 3073, 2),
        "E (dist<96|dist>=3485) +1": (lambda l, d: d < 96 or d >= 3485, 1),
        "D dist<3073 +1": (lambda l, d: d < 3073, 1),
        "D2 dist<3485 +1": (lambda l, d: d < 3485, 1),
        "F len<5&dist<3073 +2": (lambda l, d: l < 5 and d < 3073, 2),
        "RFC reference": (lambda l, d: False, 0),
    }
    only = sys.argv[2] if len(sys.argv) > 2 else None
    for name, (pred, adj) in variants.items():
        if only and only not in name:
            continue
        out = shift_decode(events, pred, adj)
        head = " ".join(f"{b:02x}" for b in out[:8])
        score, n = val.strict(bytes(out))
        print(f"{name:28s} out={len(out):6d} strict={score * 100:5.1f}% "
              f"(n={n}) head[{head}]")


if __name__ == "__main__":
    main()
