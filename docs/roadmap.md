# Roadmap

Milestones follow the original project plan (A → G). Status reflects what is
actually demonstrated by code + tests in this repository.

## Milestone A — Repository & firmware audit ✅ (commit `b30337b`)

- Reproducible analysis of `/firmware` (SHA-256 pinned, layout mapped).
- CPU architecture **verified** from opcode evidence: little-endian MIPS32r2
  (ext/ins/mul/cache/COP0 + branch-delay-slot behavior).
- SoC identified: Sunplus SPHE150x family ("SPHE15", zeroBoot 1506F strings).
- Unicorn 2 probed and rejected with a reproducible kseg-fetch failure.

## Milestone B — Rust skeleton ✅

- Cargo crate + CLI (`inspect` / `run` / `trace`), no dependencies.
- Firmware identification/validation (size, prologue words, banners, SHA-256).
- Memory subsystem: MIPS segment translation, RAM/ROM/device regions,
  alignment checks, clean faults.
- Machine composition from a documented `MachineConfig`.

## Milestone C — CPU execution proof ✅

- Known-answer tests for the interpreter (arithmetic, loads/stores,
  unaligned lwl/lwr/swl/swr, ext/ins, COP0, branch delay slots, `mul/div`).
- Real stage-1 loader executes past its prologue (integration test).

## Milestone D — Machine model & basic devices ✅

- Device models derived from boot-code evidence:
  UART (+0x900), timer (+0x600), SPI controller (+0x3E00), DDR PHY (+0x1580),
  DDR controller (+0x1500..0x16A0), param RAM, flash mirror window.
- Each model documents its evidence and confidence; stubs are visible in the
  access map, never silent.

## Milestone E — Boot progress ✅

- Full deterministic boot chain: zeroBoot → "Boot form SPI v1.13" → DDR
  training ("Trim PASS", "DRAM-1G") → payload decompress/copy → "Go!" →
  main firmware ("1ST BOOT", "Zero BSS", "IntCtrl", "LD eCos v1.16 1506f")
  → idle front-panel poll. ~1.3 M instructions, < 1 s wall clock.
- Covered by `tests/boot.rs::real_firmware_boots_through_ddr_training_into_main_firmware`.

## Milestone F — First credible receiver milestone ✅ (definition met)

The plan defined F as "bootloader executes / memory initialization succeeds"
— both are demonstrated. The next credibility rungs are application-level.

## Milestone H/I — Decompression engine: handshake ✅, eCos stage entered ✅

- **Blocker resolved**: the post-"LD eCos" park was the decompression
  engine's wait loop at 0xBFFE3800..0xBFFE3878, not a UI idle loop.
- Engine handshake modeled (devices::DecompEngineLo/Hi, machine service
  loop): table uploads via 0xBFFE3844/0xBFFE3848, start 0xBFFE3824 bit 0.
- **Completion semantics corrected (§13.3)**: the driver's success path is
  the poll's **bit-5 event branch** (ack 0x2000 → event counter reaches the
  header-mirrored limit 78 → accounting block at 0xA2002630 → return
  0x13800); the bit-6 branch is the failure epilogue. Modeling completion
  on bit 5 (the first event satisfies the counter because the second
  counter is initialized to u16@2 − 1 = 77) is what unlocks the loader.
- Compression format: "SP" container = raw DEFLATE; uploaded tables match
  the stream header bit-for-bit. Engine decode modeled with the programmed
  start bit (R3818 = 1024 stream bits; the 16-bit gap is zero padding) and
  decode-to-input-end semantics (end-of-block = marker).
- **Real firmware now prints "Go eCos" + "Boot from SPI" and jumps to the
  decompressed eCos entry at 0xA1600000** (~1.31 M instructions,
  deterministic). First entry into the eCos stage.
- **Open (§14)**: the engine's LZ value semantics — the bit parse is exact
  but the output bytes are not yet valid MIPS code (~83% word validity).
  The eCos entry trace is the next diagnostic anchor; the display pipeline
  sits behind it.

## Milestone G — Extension & stabilization (in progress)

Track by evidence-first value, in rough order:

1. ✅ **Interrupt controller + COP0 cause/compare (first pass)** — the
   "IntCtrl" init program was reverse-engineered and replays bit-identically
   (`devices::IntController`, hypothesis H11); CPU-side delivery is
   architecture-complete (Status/Cause/EPC/EXL/eret/vector selection, delay
   slots) and covered by 14 dedicated tests. **Open**: pending/ack
   semantics, per-source vectors, priorities — the firmware never enables
   `Status.IE` in the observed boot (docs/firmware-analysis.md §11).
2. ✅ **Front-panel/key register (0xBFFE384C) — decode + injection path** —
   the key-scan dispatcher is decoded (read = key lines, write = keycode
   0x100/0x400/0x1000/0x2000; docs/firmware-analysis.md §12) and virtual
   button presses are proven end-to-end on the real firmware
   (`devices::KeyPort`, `Machine::press_key`). **Open**: button labels
   (keycode→UI mapping) and a front-end that surfaces presses.
3. **Payload codec identification** (file 0x10000+): trace the stage-2
   decompressor to name the container/codec (Q4 in the analysis doc).
4. **0x98010000 / 0xB0000000 / 0xF0000000 windows**: name blocks by access
   correlation with the UART banner sequence (Q2).
5. **Performance**: ~5 M instr/s today; a decode cache would lift the
   100 M-step runs from ~20 s to a few seconds.
6. **CI**: GitHub Actions running fmt/clippy/test on push.
