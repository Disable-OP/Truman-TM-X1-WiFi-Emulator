#!/usr/bin/env python3
"""The three README claims re-measured as one command (Milestone BI, §62).

BA (§54) and BF (§59.2) re-verified the README's measurable claims by hand
with ad-hoc commands; this tool makes that audit reproducible:

- claim 1 (hard): the boot chain stops at exactly 1,301,783 steps on the
  pinned trap (exit 2, pc 0xa1600050) — a budget stop below the handoff
  is a failure of the pin;
- claim 2 (advisory): the bench harness throughput, reported and compared
  to the README band (25.4-25.6 M instr/s). The band is machine-dependent
  by the README's own wording — a miss warns but does not fail;
- claim 3 (hard): every content line of the README's console sample
  appears in the live run output in order (subsequence match, `...`
  elision lines skipped, CR and trailing-whitespace normalization per
  §54/§59).

Usage:
    python3 tools/claims_audit.py [path-to-truman-emulator] [firmware.bin ...]

Defaults: <repo>/target/release/truman-emulator (built if missing) and
<repo>/firmware/*.bin. Exit 0 = all hard pins hold, 1 = a hard pin
failed, 2 = usage error.
"""

import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
BAND = (25.4, 25.6)
PIN_STEPS = "1301783"
PIN_TRAP = "0xa1600050"
PIN_BUDGET = "1310000"


def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def ensure_binary(binary: Path, fw: Path) -> None:
    if not binary.exists():
        print(f"claims: building {binary.name} …")
        r = sh(["cargo", "build", "--release", "-q"])
        if r.returncode != 0:
            sys.exit(f"claims: cargo build failed:\n{r.stderr}")


def read_readme_sample() -> list[str]:
    """Content lines of the README console sample, in order, elision and
    blanks skipped."""
    readme = (REPO / "README.md").read_text()
    block = re.search(r"## Current status.*?```\n(.*?)```", readme, re.S)
    if not block:
        sys.exit("claims: the README status sample block was not found")
    sample = []
    seen_console = False
    for line in block.group(1).splitlines():
        if "-- serial console" in line:
            seen_console = True
            continue
        if not seen_console:
            continue
        t = line.rstrip("\r").rstrip()
        if not t or t == "...":
            continue
        sample.append(t)
    return sample


def live_console(raw: bytes) -> list[str]:
    """Console lines of a run report, `  | ` prefixes stripped.

    The capture must be BYTES and the split on `\n` only: the device
    stream carries CR artifacts (the §54 family), so a console line
    reaches the report as `  | \rzeroBoot ...` — str.splitlines() or
    python's universal-newline text mode would split on the embedded
    `\r` and shred the `  | ` prefix away from its content. The `\r`
    artifacts are stripped explicitly on both ends of the content.
    """
    out, in_console = [], False
    for line in raw.split(b"\n"):
        line = line.decode("utf-8", errors="replace").rstrip("\r")
        if "-- serial console" in line:
            in_console = True
            continue
        if not in_console:
            continue
        if line.startswith("-- ") and "serial console" not in line:
            break  # next report section
        if line.startswith("  | "):
            content = line[4:].lstrip("\r").rstrip()
            if content:
                out.append(content)
        elif not line.strip():
            continue
        else:
            break  # unindented tail (e.g. the empty-console line)
    return out


def main() -> int:
    argv = sys.argv[1:]
    binary = Path(argv[0]) if argv else REPO / "target/release/truman-emulator"
    fws = [Path(p) for p in argv[1:]] or sorted((REPO / "firmware").glob("*.bin"))
    if not fws:
        sys.exit("claims: no firmware image found")
    ensure_binary(binary, fws[0])
    fw = fws[0]
    ok = True

    # Claim 1 + claim 3 in one bounded run past the pinned trap. The
    # capture is BYTES: text mode would translate the device CR artifacts
    # into line breaks (see live_console).
    r = subprocess.run([str(binary), "run", str(fw), "--max-steps", PIN_BUDGET],
                       capture_output=True)
    out = r.stdout
    print(f"claim 1 (boot chain pin): exit={r.returncode}")
    steps_ok = (f"stopped after {PIN_STEPS} steps".encode() in out
                and PIN_TRAP.encode() in out)
    print(f"  stopped after {PIN_STEPS} steps on {PIN_TRAP}: "
          f"{'PASS' if steps_ok else 'FAIL'}")
    if r.returncode != 2 or not steps_ok:
        ok = False
        print(f"  stdout:\n{out.decode(errors='replace')}"
              f"\nstderr:\n{r.stderr.decode(errors='replace')}")

    sample = read_readme_sample()
    live = live_console(out)
    i = 0
    misses = []
    for s in sample:
        j = next((k for k in range(i, len(live)) if live[k] == s), None)
        if j is None:
            misses.append(s)
        else:
            i = j + 1
    print(f"claim 3 (banner in-order): {len(sample) - len(misses)}/{len(sample)} "
          f"sample lines matched in order")
    for m in misses:
        print(f"  MISSING: {m!r}")
    if misses:
        ok = False

    # Claim 2: advisory throughput.
    bench = REPO / "target/release/examples/bench"
    try:
        b = sh(["cargo", "build", "--release", "-q", "--example", "bench"])
    except OSError:
        b = None
        print("claim 2 (bench throughput): SKIP (cargo not on PATH)")
    if b is not None and b.returncode != 0:
        print("claim 2 (bench throughput): SKIP (bench build failed)")
    elif bench.exists():
        r = sh([str(bench), str(fw)])
        m = re.search(r"throughput\s*:\s*([\d.]+) M instr/s", r.stdout) if bench.exists() else None
        if not m and bench.exists():
            print("claim 2 (bench throughput): SKIP (no throughput line)")
        elif not m:
            pass
        else:
            t = float(m.group(1))
            in_band = BAND[0] <= t <= BAND[1]
            print(f"claim 2 (bench throughput): {t:.2f} M instr/s — "
                  f"{'inside' if in_band else 'OUTSIDE'} the README band "
                  f"{BAND[0]}-{BAND[1]} (machine-dependent; advisory only)")
            if not in_band:
                print("  advisory: the README states the band as measured "
                      "across sandbox rebuilds; a miss on other hardware is "
                      "not a defect — consider re-stating the band.")

    print("claims audit:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
