# Changelog

## 0.2.0 — Milestones B/C/D/E: the real firmware boots to eCos idle

### Added
- MIPS32r2 LE interpreter (pure Rust, dependency-free): integer ISA incl.
  `mul/madd/msub`, unaligned `lwl/lwr/swl/swr`, MIPS32r2 `ext/ins/seb/seh/wsbh`,
  `clz/clo`, branch-likely annulment, COP0 subset (Status/Config/Config1/
  Count/Cause/PRId/EBase/TagLo/TagHi), `cache`/`pref` as logged no-ops,
  `rdhwr`, BREAK/SYSCALL/traps as clean errors, and the DSP ASE indexed
  loads `lwx`/`lhx` found in the main firmware's eCos routines.
- COP0 model with self-consistent Config1 cache geometry.
- Address space: kseg0/kseg1 translation, RAM/ROM/device regions, alignment
  faults, `peek_phys` diagnostics, per-instruction tick fan-out.
- Device models (all evidence-documented, all access-logged):
  - UART0 (TX data +0x900; status bit 0 TX-ready, bit 6 TX-idle)
  - Free-running timer (+0x600, software resettable — `delay(N)` semantics)
  - SPI controller (+0x3E00: TX/start/status/RX/CS; RX=TX[15:8] echo hypothesis)
  - DDR PHY training engine (+0x1580) and DDR controller block
    (+0x1500..+0x16A0) with the polled ack/done bits
  - Param RAM windows, IRAM overlay, flash mirror window (0x18000000)
- CLI `inspect` / `run` / `trace` with per-instruction memory events,
  device touchmap, serial console capture, deterministic trace tails.
- Tests: 37 unit + 5 integration, including the real-firmware boot chain
  to the eCos idle poll (`Trim PASS` → `Go!` → `1ST BOOT` → `LD eCos v1.16`).
- Docs: `docs/firmware-analysis.md` §10 (runtime register models),
  `docs/architecture.md`, `docs/roadmap.md`.

### Fixed
- `ext`/`ins` field placement (pos in SA, msb in RD) — caught by the real
  firmware's Status.ERL check.
- kseg1 physical masking for the overlay windows (0x1FFC0000/0x1FFE0000).
- sysreg param RAM byte/half access slicing.
- Real firmware's `jal` into the 0x18000000 fast-mirror window (fetchable
  device support).

## 0.1.0 — Milestone A: firmware analysis
- Reproducible audit of the 4 MiB flash dump: SHA-256, layout, strings,
  capstone cross-checks.
- CPU architecture verified: little-endian MIPS32r2 (opcode-level evidence).
- SoC identified: Sunplus SPHE150x ("SPHE15", zeroBoot 1506F).
- Unicorn Engine 2 evaluated and rejected with a reproducible kseg-fetch
  failure; decision recorded for a custom Rust interpreter behind a
  `CpuBackend` trait.
