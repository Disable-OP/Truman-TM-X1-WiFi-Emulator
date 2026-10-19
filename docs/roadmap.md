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

## Milestone G — Extension & stabilization (next)

Track by evidence-first value, in rough order:

1. **Payload codec identification** (file 0x10000+): trace the stage-2
   decompressor to name the container/codec (Q4 in the analysis doc).
2. **Interrupt controller + COP0 cause/compare**: the eCos "IntCtrl" step
   currently runs without real interrupt delivery; modeling the controller
   would let the scheduler actually tick.
3. **Front-panel/key register (0xBFFE384C)**: decode bits 0/2/4/5/6 from the
   key-scan code so synthetic key presses can be injected for testing.
4. **0x98010000 / 0xB0000000 / 0xF0000000 windows**: name blocks by access
   correlation with the UART banner sequence (Q2).
5. **Performance**: ~5 M instr/s today; a decode cache would lift the
   100 M-step runs from ~20 s to a few seconds.
6. **CI**: GitHub Actions running fmt/clippy/test on push.
