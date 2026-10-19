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
decompress/copy → eCos RTOS start — runs **deterministically under the
emulator's own MIPS32r2 interpreter** (~1.3 M instructions, covered by an
automated acceptance test). Execution currently idles in the application's
front-panel status poll, which is the correct behavior for a receiver with
no key pressed.

What is *not* claimed: video/audio output, tuner/demod lock, Wi-Fi, remote
control. See `docs/firmware-analysis.md` §10.5 for the honest boundary.

## Build & run

Requires a stable Rust toolchain (https://rustup.rs). No other dependencies.

```sh
cargo build --release
cargo test --release          # 42 tests incl. the full boot-chain acceptance

# Identify a firmware image
./target/release/truman-emulator inspect firmware/*.bin

# Execute (default budget 1,000,000 instructions)
./target/release/truman-emulator run firmware/*.bin --max-steps 100000000

# Per-instruction trace of a window (PC, instruction, memory accesses)
./target/release/truman-emulator trace firmware/*.bin --max-steps 2000 --limit 120
```

Exit codes: `0` step-limit (clean), `1` usage/IO, `2` emulation stopped on a
fault/unknown instruction.

## Repository layout

| Path | Contents |
|---|---|
| `src/cpu/` | MIPS32r2 LE interpreter + COP0 model (behind a `CpuBackend` trait) |
| `src/memory.rs` | Address space: kseg0/kseg1 translation, RAM/ROM/device regions |
| `src/devices.rs` | Device models: UART, timer, SPI controller, DDR PHY/controller, param RAM, mirror window |
| `src/machine.rs` | Machine composition (memory map = documented hypotheses) + run loop |
| `src/firmware.rs`, `src/boot.rs` | Image identification/validation, reset/diagnostics |
| `src/bin/truman-emulator.rs` | CLI frontend (`inspect` / `run` / `trace`) |
| `tests/boot.rs` | Integration tests, incl. the full real-firmware boot chain |
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
