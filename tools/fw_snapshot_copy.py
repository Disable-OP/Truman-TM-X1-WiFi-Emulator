#!/usr/bin/env python3
"""Snapshot-copy hypothesis + delta-class search (Milestone L exp. 4).

H1: the engine's match copy is a DMA block copy WITHOUT byte-by-byte
    overlap handling: the source is snapshotted before the copy, so
    overlapping matches (dist < len) do NOT replicate. Classic hardware
    behavior, never tested in §14.3.

H2: the accounting delta 79,872 - 75,398 = 4,474 is a count of some
    natural match class; find classes with exactly that size.

Usage: python3 tools/fw_snapshot_copy.py <flash.bin>
"""
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from fw_engine_lz import LBASE, DBASE
from fw_ring_sweep import Validator, load_events, rfc_decode, byte_stats


def snapshot_decode(events, preset=0x8000, mode="snapshot"):
    """mode: snapshot | rfc | snapshot_word (snapshot at word granularity)"""
    out = bytearray()
    for e in events:
        k = e["kind"]
        if k == "LIT":
            out.append(e["sym"])
        elif k == "MATCH":
            ln = LBASE[e["sym"] - 257] + e["eb"]
            d = DBASE[e["dsym"]] + e["deb"]
            q = len(out)
            if mode == "rfc" or d >= ln:
                for i in range(ln):
                    p = q - d + i
                    out.append(out[p] if p >= 0 else 0)
            else:
                # snapshot: read the whole source range first (positions
                # beyond the current output read as preset zeros)
                src = bytes((out[q - d + i] if 0 <= q - d + i < q else 0)
                            for i in range(ln))
                out.extend(src)
        elif k.startswith("ERR") or k == "EOF":
            break
    return bytes(out)


def main():
    flash = open(sys.argv[1], "rb").read()
    val = Validator(flash)
    events = load_events(flash)
    matches = [e for e in events if e["kind"] == "MATCH"]
    lens = [LBASE[e["sym"] - 257] + e["eb"] for e in matches]
    dists = [DBASE[e["dsym"]] + e["deb"] for e in matches]
    outs = [e["out"] for e in matches]

    ref = rfc_decode(events, 0x8000)
    s, _ = val.strict(ref)
    z, ent = byte_stats(ref)
    print(f"RFC baseline      : len={len(ref)} strict={s * 100:.1f}% zeros={z * 100:.1f}% ent={ent:.2f}")

    ov = [(i, l, d) for i, (l, d) in enumerate(zip(lens, dists)) if d < l]
    print(f"\noverlapping matches (dist < len): {len(ov)} of {len(matches)}")

    snap = snapshot_decode(events, mode="snapshot")
    s, _ = val.strict(snap)
    z, ent = byte_stats(snap)
    print(f"snapshot copy     : len={len(snap)} strict={s * 100:.1f}% zeros={z * 100:.1f}% ent={ent:.2f}")

    # H2: class-size search for 4474
    target = 4474
    print(f"\nclasses of size exactly {target}:")
    found = 0
    import itertools
    # thresholds on len
    for t in range(3, 259):
        c = sum(1 for l in lens if l <= t)
        if c == target:
            print(f"  len <= {t}: {c}"); found += 1
        c = sum(1 for l in lens if l >= t)
        if c == target:
            print(f"  len >= {t}: {c}"); found += 1
    for t in range(1, 32744):
        c = sum(1 for d in dists if d <= t)
        if c == target:
            print(f"  dist <= {t}: {c}"); found += 1
    # alignment classes
    for a in (2, 4, 8, 16):
        for m in range(a):
            c = sum(1 for (l, d, q) in zip(lens, dists, outs) if (q - d) % a == m)
            if c == target:
                print(f"  (q-d) % {a} == {m}: {c}"); found += 1
            c = sum(1 for q in outs if q % a == m)
            if c == target:
                print(f"  q % {a} == {m}: {c}"); found += 1
            c = sum(1 for (l, d) in zip(lens, dists) if d % a == m)
            if c == target:
                print(f"  d % {a} == {m}: {c}"); found += 1
            c = sum(1 for l in lens if l % a == m)
            if c == target:
                print(f"  len % {a} == {m}: {c}"); found += 1
    if not found:
        print("  (none among simple thresholds/alignments)")


if __name__ == "__main__":
    main()
