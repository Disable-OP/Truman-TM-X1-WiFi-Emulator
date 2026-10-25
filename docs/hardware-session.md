# Hardware-session protocol — ingesting a real device's DRAM captures

Milestone Y. Per docs/firmware-analysis.md §22.5/§26.4, every hypothesis
class on the SP stream that is reachable without hardware is refuted with
data; further progress on the engine LZ head paradox requires a real
device's post-decode DRAM dump. This document is the handoff: what to
capture, how, how to verify the capture, and the pre-registered decision
tree the ingestion command implements.

**Nothing in this document is validated on hardware.** The JTAG path is
the plan, not a tested claim; pinout discovery on the actual board is part
of the session. Every claim in this repo about the emulated boot itself
remains backed by `docs/firmware-analysis.md`; this page only plans the
physical capture.

## The decisive experiments

### P1 — post-decode pixel buffer (the LZ-semantics oracle)

| item | value |
|---|---|
| window | phys `0x01600000` (kseg1 view `0xA1600000` uncached; kseg0 view `0x81600000` cached) |
| length | 79,872 bytes = IMG_WIDTH 0x400 × IMG_HEIGHT 78 (from the engine's own registers, PNG_DEC_*) |
| when | after the engine's completion accounting (78 row events) and before the loader jumps to `0xA1600000` |
| emulator reference | `dram_compare --post` (boots the real firmware, extracts the same window) |

### P2 — pre-decode residue (the head-paradox oracle)

| item | value |
|---|---|
| window | the same, plus ideally 1 MiB below `0x01600000` (the §18.3 never-written region) |
| when | immediately after reset, before the payload decompress stage starts |
| expectation | all-zero per the emulator side (§18.3: the boot never writes below the pixel base); **any** non-zero content is the §21.6 item-2 answer |

P2 must be captured on a **separate cold boot** from P1: P2 is a
capture-at-reset, P1 is a capture-at-handoff, and you cannot rewind the
hardware between them.

### P3 — the probe-window fingerprint (the cheap oracle, Milestone AG)

Vendor-named in §34.2: the PNG_DEC probe window
(`PNG_PROBE_ADDR` sysreg +0x3878 / `PNG_PROBE_VALUE` +0x387C) exposes
decoder internals; the vendor drivers' end-of-image branch reads the
decode's **Adler-32** as two 16-bit halves (probe address 0 = high,
1 = low), and the Truman loader's own end-of-image handler performs
exactly that probe sequence (§12's "strobe + two samples"). At the P1
halt window, before (or after) the pixel dump:

```
# via the same debugger that performs the P1 dump (mips_m4k memory
# access while halted); addresses are the kseg1 views the loader uses
mww 0xBFFE3878 0x00000000        # probe address 0
mdw 0xBFFE387C                   # -> high half
mww 0xBFFE3878 0x00000001        # probe address 1
mdw 0xBFFE387C                   # -> low half; adler = (hi<<16)|lo
```

| item | value |
|---|---|
| cost | four debugger operations — no capture file |
| precondition | the same halt window as P1 |
| compare against | the pinned emulator fingerprint `0xcd8d03bf` (or the `dram_compare --post` report line "reference adler-32") |
| **caveat (registered)** | whether the engine accumulates Adler-32 over a `zlib_mode=1` **raw** decode is **unverified** — the vendor readout is shown in PNG image mode only, and the emulator deliberately does not model an accumulator (§34.2). A zero/stuck/implausible readout is the documented "does not accumulate" outcome, not a capture error |

A matching fingerprint would mean the emulator's decode IS the device's
decode — decide what that would mean via the P1 tree before trusting it
(a match this strong must survive the P1 capture-point suspicion check
too). A non-matching fingerprint is still a 32-bit constraint no
offline search had before: any candidate value-table decode whose
output does not produce that Adler-32 is eliminated (the §30.3
m4 underdetermination collapses under one output fingerprint). P3 does
not replace P1 — P1's divergence map, not a checksum, drives the solve
— but it is the fastest first read of the session and survives even if
the DRAM dump path fails.

## How to capture

### Primary plan — JTAG/EJTAG

The SPHE15xx CPU is a MIPS 24KEc (MIPS32r2, per the platform DTS cited in
§17.1). MIPS32 cores implement EJTAG, and the de-facto 14-pin MIPS EJTAG
header convention carries TRST#, TDI, TDO, TMS, TCK, Vtref and GND (the
same pinout family documented for other MIPS set-top/router hardware).
OpenOCD speaks this via its `mips_m4k` target class:

```
# adapter/bring-up specific — pinout discovery is part of the session
openocd -f <adapter.cfg> -f <target.cfg> -c "adapter speed 1000" \
        -c "init" -c "halt" \
        -c "dump_image /tmp/p1_pixel.bin 0xA1600000 79872" \
        -c "shutdown"
```

Notes for the session (all to be confirmed on hardware, none assumed):

* dump the **kseg1 (0xA1600000)** view — that is the window the loader
  itself writes through; if the tooling only offers kseg0, record which
  view was dumped (cache-state differences are capture errors, not
  device answers);
* halt before dumping; a running CPU may be mid-scanline in the engine;
* P2: reset, halt as early as possible, dump the same window (and the
  extra 1 MiB below) as `p2_predecode.bin`;
* if the EJTAG DMA facility is available on this core it allows memory
  access without halting — prefer halting anyway for P1 so the capture
  point is exact.

### Secondary plan — a serial monitor, if one exists

The observed boot prints UART logs but no monitor is known to exist
(UART is output-only in everything we traced, §11). If a vendor or eCos
monitor surfaces (the platform heritage is eCos/RedBoot), the RedBoot
`dump -b <addr> -l <length>` pattern would be the shape of it. Treat any
such find as opportunistic; do not spend session time hunting for one.

### Not applicable — flash readers

An SOIC-8 SPI clip reads the flash (which we already have, SHA-pinned).
It cannot answer P1/P2 — those live in DRAM at runtime.

## Capture integrity checklist

For **every** capture, record in the session log:

1. SHA-256 of the captured file (host side, after transfer);
2. which window and view (phys / kseg1 / kseg0), and the byte length;
3. the halt point: PC at capture (EJTAG) or the trigger condition;
4. the last ~10 UART lines before the halt (boot-phase provenance);
5. whether the board cold-booted since power-on (P2 and P1 need two
   separate cold boots, in that order).

Never re-run the boot between halting and dumping. Never capture through
a cached view for P1 without recording it.

## Ingestion — one command per experiment

```
cargo run --release --example dram_compare -- \
    --post firmware/<flashfile>.bin p1_pixel.bin --out p1_map.txt
cargo run --release --example dram_compare -- \
    --pre p2_predecode.bin --out p2_map.txt
cargo run --release --example dram_compare -- \
    --ref saved_pixel_dump.bin p1_pixel.bin        # offline re-analysis
```

The P1 DIVERGENT row's decisive constraint is one command (§36,
Milestone AI): it boots, compares, replays the boot's own decode with a
per-symbol event trace, verifies the replay byte-identical against the
boot window (hard error otherwise), and attributes the first divergent
byte to its producing event — literal (value-table-unreachable, §19.3),
match (the first match whose value semantics differ), or band fill
(the §26.4 length-accounting family):

```
cargo run --release --example solve_divergence -- \
    firmware/<flashfile>.bin p1_pixel.bin --value-tables recovered.spvt \
    --out p1_events.tsv
```

Exit codes: `0` identical (the P1 IDENTICAL row applies — suspect the
capture point first) · `1` divergent or size-Δ (the attribution report
is the actionable data) · `2` usage, IO or replay-integrity error.

The solve itself is now one command (§37, Milestone AJ) — it routes
through the P1 tree (identical → the capture-point row; non-zero band →
the §26.4 row), runs the inversion, verifies the recovered ladder by the
engine's own re-decode, and writes the `SPHEVTBL` container the §35 seam
consumes:

```
cargo run --release --example ladder_solve -- \
    firmware/<flashfile>.bin p1_pixel.bin --out recovered.spvt
# then the post-capture boot under the recovered ladder:
truman-emulator map firmware/<flashfile>.bin --value-tables recovered.spvt
```

Exit codes: `0` identical window (P1 IDENTICAL) or solved AND
engine-verified · `1` divergent but unsolved (the attribution from
`solve_divergence` is the actionable data; a budget failure with a
differing P3 fingerprint is recorded information — the deviation is
outside the enumerated family, §37.2) · `2` usage, IO or stream error.
The reference self-test (the solve recovering the RFC ladder from its
own pinned output, 57/57 codes) runs as `cargo test --test
solve_ladder`.

The post-capture value swap (§35, Milestone AH) is a data file, not a
recompile: `dram_compare --post` (and the CLI `map`) accept
`--value-tables <file>` — a 191-byte `SPHEVTBL` container written with
`ValueTables::to_bytes` once the ROM constants are recovered. The P3
elimination row is executable directly (`examples/ladder_eval.rs`):

```
cargo run --release --example ladder_eval -- firmware/<flashfile>.bin \
    --value-tables candidates/foo.spvt --fingerprint 0xCAPTURED
# exit 0 = SURVIVES, exit 1 = ELIMINATED (the pre-registered row below);
# loop it over every candidate family §15.6/§18.2/§26 bounded.
```

Exit codes: `0` identical / zero residue · `1` divergent / residue
present (actionable data — this is a *good* outcome for the solve) ·
`2` usage, IO or empty-overlap error.

**The vector gate (§40.4, Milestones AM+AN — mandatory second validation,
mechanical since AN).**
A ladder recovered from P1 must also re-decode the vendor's own SDK test
container (the same hardware ROM values; `tools/reference/`), which
`sp_gzip` compressed against a *known* 9,216-byte pattern. The
recovered-ladder vector decode must be clean sparse hex text
(`[0-9a-f]`, `\n`, `\r`, NUL only) with the pinned literal backbone
(§40.2: ≥99% of the stream's literals are hex characters) and must
account to the container's own 9-chunk size (§40.3). A ladder that fits
P1 but fails the vector gate is wrong.

**Since Milestone AN the gate runs mechanically inside `ladder_solve`
itself** (`src/vendor_vector.rs`): every verified solve is automatically
gated at the three candidate start bits before `--out` is written. A
gate failure is reported per start bit (decode length, anchor delta,
literal census, alphabet violations) and the container is NOT written
(exit 1 — the report is the actionable data). There is no skip flag; a
missing container is an environment error (exit 2). The reference
implementation of the check's data side (censuses, backbone, joint
anchors) remains offline and deterministic, and doubles as the
independent cross-check of the Rust gate (the §40 numbers are
dual-pinned: `tests/vendor_vector.rs` against the Python output):

```
python3 tools/fw_vector_probe.py
```

It also prints the §40.3 joint anchor system — Truman's pinned
+4,464 against the vector's start-bit-dependent −570/−377/−358 — which
any candidate ladder must satisfy jointly (per-class uniform families
have zero survivors; the true deltas are per-code structured, short
codes at-or-below RFC, long codes above).

The comparison is pure integer math (`Machine::compare_dumps` /
`Machine::classify_residue` in `src/machine.rs`, pinned by
`tests/hwsession.rs`): matching count and per-mille, first divergence,
1024-byte divergent/residue block map, longest match run.

## Pre-registered decision tree

Decide **before** looking at any capture what each outcome means. This
tree is the registration; amending it after seeing data must be called
out explicitly in the session record.

| outcome | meaning | next action |
|---|---|---|
| P1 IDENTICAL (same size, no divergence) | the emulator's engine decode equals the device's — contradicting §19 (this reference's head dies in-emulator) | suspect the capture point first: re-capture at the `jr v0` handoff, kseg1 view; only then revisit the engine model |
| P1 PREFIX-IDENTICAL, size Δ | same decode, different allocation/length accounting | record both lengths (§26.4's 75,398 vs 79,872 family); re-run the size-anchor algebra with the true length |
| P1 DIVERGENT | the LZ value semantics differ from every model tested in §14–§26 | run `solve_divergence` (§36): the first divergent byte is attributed to its producing decode event — the first match whose value semantics differ — with the identical prefix's pin; the `--out` event trace is the per-symbol map for the solve |
| P2 ZERO | residue ruled out in the captured window | widen the P2 window (the 1 MiB below the base), repeat once; then the paradox residue is elsewhere in DRAM |
| P2 RESIDUE PRESENT | the §21.6 item-2 answer: the engine starts from non-zero state | load the residue via `AddressSpace::copy_in` before the engine start in a fresh emulator boot and re-run the decode compare — the emulator supports this today |
| P3 MATCHES `0xcd8d03bf` | the device's decode equals the emulator's reference output (a 2^-32 coincidence for any other decode) | first re-verify the capture point per the P1 IDENTICAL row (a match this strong must survive the same suspicion); then the blocker reframes: the engine decode is known — re-run the §18.4 algebra with the *true* output and re-pin `ENGINE_VALUE_TABLES` from the solved semantics |
| P3 PRESENT, ≠ reference | the true decode has a 32-bit fingerprint; every candidate decode whose output mismatches it is eliminated offline | re-run the §15.6/§18.2/§26 swept families under the fingerprint oracle (`adler32(decode) == captured`) — executable today as a `ladder_eval` loop over `SPHEVTBL` candidate files (§35); a family whose best candidate matches is the answer; none matching = the true ladder is outside every swept family — record and close that space too |
| P3 ZERO / STUCK / implausible | the engine does not accumulate Adler-32 over raw decodes (the registered caveat outcome) | no information lost — proceed with P1/P2 as planned; record the probe values verbatim in the session log regardless |

## Where the outputs go

Captures and maps are session artifacts: they are **not** committed to
the repository (no third-party dumps, no unverifiable binaries). The
session record cites their SHA-256s and the verdicts; the tools make
every verdict independently reproducible from the cited files.
