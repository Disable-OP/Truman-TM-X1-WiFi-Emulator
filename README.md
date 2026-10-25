# Truman TM X1 WiFi — Firmware Emulator

A portable Rust emulator for the **Truman TM X1 WiFi** satellite TV receiver,
built against the real firmware dump in [`firmware/`](firmware/) (Sunplus
SPHE150x SoC, little-endian **MIPS32r2**, 4 MiB SPI NOR flash). The dump is
pinned by SHA-256
`5ca012b28e2a70e322a48d513d2a3011a0778a7afa7ecd53e4aad102c2cd28f0`
(`1506HV_512M_4M_SVT2_TRUMAN_TM_X1_WIFI_Internal_flashfile_20220902.bin`,
the audit anchor of `docs/firmware-analysis.md` §1).

The project's method: every emulated component is derived from **evidence in
the firmware itself** (static disassembly + dynamic execution traces), and
every hypothesis is documented and falsifiable. Nothing is faked to look
working — unimplemented hardware logs its accesses and is listed as unknown.

## Current status — the real firmware boots through the loader's handoff into the decompressed stage

```
$ truman-emulator run firmware/1506HV_..._flashfile_20220902.bin
...
-- serial console (UART @ 0xBFFE0900) --

zeroBoot v1.0 for 1506F
SPI SCS flow
jump to second boot!!
Boot form SPI v1.13
SPI SCS flow

Calc RAM Param

DDR-V6.2.01.09-v
SYS_306
DDR_1066
DRAM device ODT off
DDRPHY IO ODT off
...
Trim PASS

DRAM-1G
copy fw to RAM
Go!

1ST BOOT
Zero BSS
IntCtrl
LD eCos v1.16 1506f
```

The full chain — boot ROM stage 1 → SPI bootloader → DDR training → payload
decompress/copy → the "1ST BOOT" loader — runs **deterministically under the
emulator's own MIPS32r2 interpreter** (~1.30 M instructions, covered by an
automated acceptance test). The loader then talks to the hardware
decompression engine at 0xBFFE3800 (uploads the Huffman tables it parsed
from the compressed "SP" container, starts it, and waits for bit 5 of
0xBFFE384C — the same loop scans the front-panel keys). With the engine
handshake modeled (`src/inflate.rs`, a dependency-free RFC 1951 decoder) the
wait loop exits and the loader reaches its result check. Milestone N
identified the hardware: the "decompression engine" is a **PNG decoder**
(the SPHE15xx platform header names the whole 0xBFFE3800 block
`PNG_DEC_*`). The boot renders a 1024x78 8-bpp image into 0x01600000 and
**jumps into the pixel buffer** — the vendor tool used the boot ROM's PNG
engine as a decompressor and the eCos binary is the pixel data. The
emulator models the PNG row semantics (the engine completes every
programmed row; `tests/png.rs` pins the whole register program); the
remaining blocker is the engine's LZ *value* semantics — which bytes a
match copies (`docs/firmware-analysis.md` §17).

**Milestone AD maps what executes after the handoff** (§32):
`truman-emulator map` boots to the `jr v0` handoff and records the
post-handoff horizon — today 20 instructions in the entry page, zero
device accesses, zero console output, death on the RFC decode's first
bad word (0xFFFF) at 0xA1600050. The instrument is value-table-
agnostic: when the capture-recovered tables land, the same command maps
the real menu path (executed-page code map, device init order, console).

**Milestone AE adds the written-memory map** (§33): the same `map`
command also watches the whole DRAM span for data stores — written
pages, total stores and the first 64 stores with addresses and values.
A framebuffer lights up there without any assumption about stage
semantics; consecutive stores 0x400 apart are 1024-byte rows (the PNG
width). The RFC horizon measures zero stores (§33.3).

**Milestone R adds the decisive instrument** (`docs/firmware-analysis.md`
§20): the **entry harness** (`examples/entry_oracle.rs`) boots the real
firmware with a watch at 0xA1600000 (the loader's `jr v0` target), captures
the exact post-jump register state (v1 = 0x13800) and the full DRAM, then
executes candidate decodes as real MIPS code from the pixel buffer — a
register-aware dynamic oracle with NOP-sled discrimination (distinct-PC /
zero-fetch / UART metrics). The first family run through it — all
class-preserving literal-value maps (xor/add sweeps, bitrev8, nibswap,
lut-by-value, rank-shifts) plus per-length reversal and the m2/m4
accountings — is **refuted**: 522 candidates, 519 fault in the head, the 3
budget-survivors are NOP-sled artifacts. The register-aware oracle also
corrected Milestone Q's static m4 verdict: the head's `blez $v1` falls
through (v1 = 0x13800 > 0) and the head dies at +0x38, not +0x6c.

Milestone U (device coverage, §23): the COP0 Cause layout was corrected
against the architecture (BD=31, IV=23, TI=30 read-only) and the
Count/Compare timer interrupt completed to spec (match-edge latch, ack by
a Compare write, IP7 + Cause.TI, DC gating, IM7/IE delivery — proven by a
synthetic end-to-end test; the real boot never enables interrupts and is
unaffected). The keycode-consumer hunt closed negative: static
constant-propagation scan + dynamic watchpoint attribution show every
0xBFFE384C access comes from the dispatcher itself, so physical button
labels stay evidence-bounded (the consumer lives in the unreachable eCos
stage or hardware). The SoC timer block's full register program is pinned
(one delay(10) poll, two config writes, no IRQ semantics claimed).

Milestone J: the driver's engine interface is now fully reverse-engineered
(start-bit computation, table-upload protocol = engine table-RAM addresses,
completion accounting formula), and a **decode defect was found and fixed**:
the emulator's match copy used a frozen read pointer (constant fills instead
of live-window copies) — proven by re-implementation and covered by a
regression KAT. The bit stream itself is verified exact (25,027 symbols, zero
bad codes), the uploaded tables are verified canonical, and the programmed
output size is 78 × 1024 = 79,872 bytes; RFC 1951 value semantics produce
75,657 bytes of not-yet-valid code, so the engine's length mapping (hardware
base tables) remains the open question (§14).

Interrupt support (Milestone G, first pass): the CPU implements
architecture-complete MIPS32r2 interrupt delivery (Status/Cause/EPC/EXL,
`eret`, vector selection, delay-slot semantics) and the SoC interrupt
controller block found by the "IntCtrl" init is modeled with its verified
register program. The firmware itself keeps interrupts globally disabled
(`Status.IE = 0`) for the entire observed boot, so interrupt behavior is
demonstrated by injection tests (`tests/interrupt.rs`) — see
`docs/firmware-analysis.md` §11 for the evidence and the honest unknowns.

What is *not* claimed: video/audio output, tuner/demod lock, Wi-Fi, remote
control, interrupt dispatch inside the firmware (IE=0; §11). See
`docs/firmware-analysis.md` §10.5 for the honest boundary.

## Build & run

Requires a stable Rust toolchain (https://rustup.rs). No other dependencies.

```sh
cargo build --release
cargo test --release          # 183 tests: boot chain, entry harness, interrupt + COP0 timer delivery, key injection,
                              # watchpoints, Q8 runtime evidence, timer program, keycode-consumer pin,
                              # perf-model regressions (lazy describe, hot-region memo),
                              # post-handoff map (exec pages, MMIO, DRAM write map),
                              # value-table file seam (round-trip, rejections, file-loaded decode pins),
                              # divergence→symbol attribution, the capture→ladder solver (§36-§37),
                              # the §40.4 vendor vector gate (§41),
                              # the §40.3 class-prior solve ordering (§42),
                              # CLI surface/error/success-path contracts (§53-§56)
cargo build --release --example bench
./target/release/examples/bench firmware/*.bin   # throughput harness (Milestones V+Z): ≈25 M instr/s (machine-dependent; 25.4–25.6 measured across sandbox rebuilds)
                                                  # on the deterministic boot chain (17.5x over 0.17.0)

# Identify a firmware image
./target/release/truman-emulator inspect firmware/*.bin

# Execute (default budget 1,000,000 instructions)
./target/release/truman-emulator run firmware/*.bin --max-steps 100000000

# Per-instruction trace of a window (PC, instruction, memory accesses)
./target/release/truman-emulator trace firmware/*.bin --max-steps 2000 --limit 120

# Map the post-handoff horizon (executed pages, MMIO record, console;
# Milestone AD / §32) — today the RFC decode dies at entry+0x50; post-capture
# this same command maps the real menu path
./target/release/truman-emulator map firmware/*.bin

# Entry harness: execute candidate decodes as real code from the eCos entry
# (see docs/firmware-analysis.md §20; tools/fw_syms_family.py generates them)
python3 tools/fw_syms_family.py firmware/*.bin
./target/release/examples/entry_oracle firmware/*.bin \
    --budget 65536 tools/out/syms_family/*.bin

# Q8 instrument: boot with watchpoints over the descriptor/payload/container
# ranges — who reads what, with (step, PC) attribution (§22)
./target/release/examples/q8_watch firmware/*.bin

# Device-coverage instruments (Milestone U, §23): the SoC timer register
# program, and keycode-port access attribution under virtual key presses
./target/release/examples/timer_probe firmware/*.bin
./target/release/examples/keycode_consumer firmware/*.bin
python3 tools/fw_keycode_consumer.py firmware/*.bin

# Hardware-session ingestion (Milestone Y): the two decisive experiments
# of docs/firmware-analysis.md §22.5, one command each, decision tree
# pre-registered in docs/hardware-session.md
./target/release/examples/dram_compare --post firmware/*.bin p1_pixel.bin
./target/release/examples/dram_compare --pre p2_predecode.bin

# Value-ladder seam (Milestone AH / §35): the engine's LZ ladder as a data
# file — the post-capture swap is a flag, not a recompile
./target/release/examples/dram_compare --post firmware/*.bin p1_pixel.bin \
    --value-tables recovered.spvt
./target/release/truman-emulator map firmware/*.bin --value-tables recovered.spvt

# Band-window seam (Milestone AQ / §44): the engine's LZ window CONTENT as a
# data file — the §21.6 item-2 experiment is a flag, not a recompile
./target/release/examples/dram_compare --post firmware/*.bin p1_pixel.bin \
    --preset-window p2b_band.bin
./target/release/examples/ladder_eval firmware/*.bin \
    --value-tables candidates/foo.spvt --fingerprint 0xCAPTURED   # P3 elimination loop

# Residue re-boot seam (Milestone AR / §45): the P2 RESIDUE PRESENT row as
# one command — boot clean, boot under the residue, compare the decodes
./target/release/examples/dram_compare --pre p2_predecode.bin \
    --reboot-residue firmware/*.bin --base 0x01500000

# Divergence→symbol map (Milestone AI / §36): the P1 DIVERGENT row as one
# command — the first divergent byte attributed to its decode event, with
# a replay-integrity gate and the full per-symbol trace as TSV
./target/release/examples/solve_divergence firmware/*.bin p1_pixel.bin \
    --value-tables recovered.spvt --out p1_events.tsv
```

Exit codes: `0` step-limit (clean), `1` usage/IO, `2` emulation stopped on a
fault/unknown instruction.

## Repository layout

| Path | Contents |
|---|---|
| `src/cpu/` | MIPS32r2 LE interpreter + COP0 model + interrupt delivery (behind a `CpuBackend` trait) |
| `src/memory.rs` | Address space: kseg0/kseg1 translation, RAM/ROM/device regions, physical-range watchpoints |
| `src/devices.rs` | Device models: UART, timer, SPI controller, DDR PHY/controller, interrupt controller, param RAM, mirror window |
| `src/machine.rs` | Machine composition (memory map = documented hypotheses) + run loop + IRQ injection API |
| `src/solve.rs` | The capture→ladder solver (§37): one true decode output → the recovered value ladder, verified by the engine's own re-decode |
| `src/firmware.rs`, `src/boot.rs` | Image identification/validation, reset/diagnostics |
| `src/bin/truman-emulator.rs` | CLI frontend (`inspect` / `run` / `trace` / `map`) |
| `tests/boot.rs`, `tests/interrupt.rs` | Integration tests: real-firmware boot chain, interrupt injection |
| `tests/hwsession.rs` | Milestone Y: capture-comparison/residue analysis pins + the real-firmware reference path |
| `tests/watch.rs`, `tests/q8.rs` | Watchpoint mechanics; the §22 runtime-evidence pins (loader reads exactly three descriptor words, one-pass copy, no checksum pass) |
| `tests/timer.rs` | SoC timer model semantics + the real boot's timer register program (§23.4) |
| `docs/firmware-analysis.md` | The evidence base: CPU arch, flash map, register models |
| `tools/`, `scripts/` | Analysis scripts (capstone-based recon, disassembly, value-table fitters) |
| `tools/reference/` | Attributed vendor sources naming the PNG_DEC program (§30; GPL, not built here) |
| `firmware/` | The original, unmodified flash dump (SHA-256 pinned in docs) |

## Documentation

- `docs/firmware-analysis.md` — verified facts vs. hypotheses, per-register
  evidence, security observations
- `docs/architecture.md` — module boundaries and design decisions
- `docs/roadmap.md` — milestone status (A–G) and next steps
- `docs/hardware-session.md` — DRAM-capture protocol and the
  pre-registered decision tree for the two §22.5 experiments

## License & scope

This project emulates hardware for interoperability and archival study; the
firmware blob in `firmware/` remains the property of its author. No keys,
signature material, or personal data are included beyond what exists inside
the already-public factory image.
