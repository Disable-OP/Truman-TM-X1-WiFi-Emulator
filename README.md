# Truman TM X1 WiFi — Firmware Emulator

A portable Rust emulator for the **Truman TM X1 WiFi** satellite TV receiver,
built against the real firmware dump in [`firmware/`](firmware/) (Sunplus
SPHE150x SoC, little-endian **MIPS32r2**, 4 MiB SPI NOR flash).

The project's method: every emulated component is derived from **evidence in
the firmware itself** (static disassembly + dynamic execution traces), and
every hypothesis is documented and falsifiable. Nothing is faked to look
working — unimplemented hardware logs its accesses and is listed as unknown.

## Current status — the real firmware boots to its eCos idle loop

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
cargo test --release          # 78 tests: boot chain, entry harness, interrupt delivery, key injection

# Identify a firmware image
./target/release/truman-emulator inspect firmware/*.bin

# Execute (default budget 1,000,000 instructions)
./target/release/truman-emulator run firmware/*.bin --max-steps 100000000

# Per-instruction trace of a window (PC, instruction, memory accesses)
./target/release/truman-emulator trace firmware/*.bin --max-steps 2000 --limit 120

# Entry harness: execute candidate decodes as real code from the eCos entry
# (see docs/firmware-analysis.md §20; tools/fw_syms_family.py generates them)
python3 tools/fw_syms_family.py firmware/*.bin
./target/release/examples/entry_oracle firmware/*.bin \
    --budget 65536 tools/out/syms_family/*.bin
```

Exit codes: `0` step-limit (clean), `1` usage/IO, `2` emulation stopped on a
fault/unknown instruction.

## Repository layout

| Path | Contents |
|---|---|
| `src/cpu/` | MIPS32r2 LE interpreter + COP0 model + interrupt delivery (behind a `CpuBackend` trait) |
| `src/memory.rs` | Address space: kseg0/kseg1 translation, RAM/ROM/device regions |
| `src/devices.rs` | Device models: UART, timer, SPI controller, DDR PHY/controller, interrupt controller, param RAM, mirror window |
| `src/machine.rs` | Machine composition (memory map = documented hypotheses) + run loop + IRQ injection API |
| `src/firmware.rs`, `src/boot.rs` | Image identification/validation, reset/diagnostics |
| `src/bin/truman-emulator.rs` | CLI frontend (`inspect` / `run` / `trace`) |
| `tests/boot.rs`, `tests/interrupt.rs` | Integration tests: real-firmware boot chain, interrupt injection |
| `docs/firmware-analysis.md` | The evidence base: CPU arch, flash map, register models |
| `tools/`, `scripts/` | Analysis scripts (capstone-based recon, disassembly) |
| `firmware/` | The original, unmodified flash dump (SHA-256 pinned in docs) |

## Documentation

- `docs/firmware-analysis.md` — verified facts vs. hypotheses, per-register
  evidence, security observations
- `docs/architecture.md` — module boundaries and design decisions
- `docs/roadmap.md` — milestone status (A–G) and next steps

## License & scope

This project emulates hardware for interoperability and archival study; the
firmware blob in `firmware/` remains the property of its author. No keys,
signature material, or personal data are included beyond what exists inside
the already-public factory image.
