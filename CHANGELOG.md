# Changelog

## 0.3.0 — Milestone G (first pass): interrupt controller + CPU interrupt delivery

### Added
- CPU interrupt delivery (architecture-specified): enabled/unmasked,
  non-nested interrupts vector before the next fetch; EPC + `Cause.BD`
  latch the interrupted context (delay-slot aware); vector selection via
  BEV/EBase/`Cause.IV` (0xBFC00200 / EBase+0x180 / +0x200); hardware lines
  drive `Cause.IP[7:2]` via the new `CpuBackend::set_ext_irq_lines`.
- `IntController` device at sysreg+0x480 (0x40 bytes): plain storage
  replaying the firmware's verified "IntCtrl" init program bit-identically
  + level-triggered aggregation (`sources & enable-words` → output line,
  wired to CPU IP2 by hypothesis H11). No invented pending/ack semantics.
- Machine API for virtual-device injection:
  `Machine::raise_int_sources` / `clear_int_sources` / `intc_output`; the
  run loop wires the controller output into the CPU each step.
- CLI `run --dump phys,len,path` (post-run memory dumps for analysis).
- Tests: 7 CPU interrupt KATs, 4 COP0 KATs, 1 device init-replay KAT,
  2 machine integration tests (`tests/interrupt.rs`: injection through the
  public API + lockstep determinism) and a real-firmware regression guard.

### Fixed
- `eret` implemented both MIPS32r2 paths (ERL→ErrorEPC/ERL=0,
  EXL→EPC/EXL=0); previously it always used ErrorEPC — a demonstrated
  correctness defect for the interrupt path.
- `mtc0 Cause` now applies the architectural write mask (only IV/DC/IP1:0
  writable; BD/ExcCode/IP[7:2] preserved).
- EBase (15,1) reset value 0x80000000 documented and modeled.

### Documented
- `docs/firmware-analysis.md` §11: full interrupt-controller evidence —
  the IntCtrl init function's store list (dump-verified), the COP0
  Status=0x1100FF00 programming (**IE stays 0**: the observed boot never
  enables interrupts), the zeroed vector area, and the explicitly
  unmodeled unknowns (pending/ack, per-source vectors, priorities).

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
