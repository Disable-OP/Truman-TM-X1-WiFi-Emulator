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

## The two decisive experiments

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

Exit codes: `0` identical / zero residue · `1` divergent / residue
present (actionable data — this is a *good* outcome for the solve) ·
`2` usage, IO or empty-overlap error.

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
| P1 DIVERGENT | the LZ value semantics differ from every model tested in §14–§26 | `first_divergence` + block map become the decisive constraint; feed the solve (the first divergent byte identifies the first match whose value semantics differ) |
| P2 ZERO | residue ruled out in the captured window | widen the P2 window (the 1 MiB below the base), repeat once; then the paradox residue is elsewhere in DRAM |
| P2 RESIDUE PRESENT | the §21.6 item-2 answer: the engine starts from non-zero state | load the residue via `AddressSpace::copy_in` before the engine start in a fresh emulator boot and re-run the decode compare — the emulator supports this today |

## Where the outputs go

Captures and maps are session artifacts: they are **not** committed to
the repository (no third-party dumps, no unverifiable binaries). The
session record cites their SHA-256s and the verdicts; the tools make
every verdict independently reproducible from the cited files.
