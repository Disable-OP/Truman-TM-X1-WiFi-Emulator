# Changelog

## 0.8.0 — Milestone K: engine register program fully decoded; LZ semantics search widened, then narrowed

### Added
- `Machine::access_log(name)` — public diagnostics access to the
  instrumented-window access logs, and `examples/engine_probe.rs`: runs the
  real boot and prints the decompression engine's complete register program
  plus the post-decode window/scratch/output memory state. Every value in
  docs/firmware-analysis.md §13.2 is now reproducible from a fresh clone.

### Documented (docs/firmware-analysis.md §13.2/§14)
- **The engine register program is now complete.** New decodings from the
  driver disassembly: 0xBFFE380C/3810 is the **input end pointer**
  (`(base + container_len + 0x7F) & ~0x7F` = 0x02090700, not a second
  source); 0xBFFE3834/0x3838 is the 32-bit input length (container length
  **+ 6**, the add is visible at 0xA2002224); **0xBFFE383C =
  `(BTYPE << 1) | BFINAL` = 5**, built by the driver's own bit-reader from
  the stream header; 0xBFFE3830 = 0x68 stays semantically unknown. The
  §13.1 zero-fill range is now identified as the 32 KiB LZ window
  (0x02040000..0x02048000) plus 256 KiB scratch.
- **The LZ value semantics search was widened and then narrowed** (§14):
  calibrated validity oracles (lenient capstone scoring cannot separate
  garbage from data — random words score 62.5%; strict common-instruction
  scoring puts real code at 91–95% and the RFC decode at 45.9%), the
  below-base "window violation" resolved in favor of a legitimate 32 KiB
  zero preset dictionary, and new refutation sweeps: window presets,
  output rotations/lanes/per-position XOR-ADD, length remap families,
  distance remap families, absolute ring indexing and reversed copies.
  Conclusion (§14.4): the wrongness is structural, not a wrong base
  table; the remaining hypothesis classes and next steps are listed.
- Stale "bit 6" completion references in code comments corrected to the
  verified bit-5 event line (§13.3).

## 0.7.0 — Milestone J: engine interface decoded; decode defect found and fixed

### Fixed
- **Frozen read pointer in the DEFLATE match copy** (`inflate_engine` and
  `inflate_block`): `window.len()` was captured before the copy loop, so
  every match wrote a constant fill of its first source byte instead of a
  live-window copy. Proven, not suspected: re-implementing the frozen
  semantics reproduces the previous engine output byte-for-byte (100.000%,
  75,398/75,398). The first visible corruption was output byte 195
  (a `len=7 dist=8` match decoded as seven copies of `0xCF`); earlier
  bytes agreed only because the region is zero-dominated. Regression KAT
  added (`overlapping_match_replicates_from_live_window`).
- `docs/firmware-analysis.md` §13.4's open-items measurements (word
  validity, phase drift, ruled-out list) were all taken against the
  buggy decode and are superseded by §13.6/§14.

### Added
- **Complete engine-interface reverse-engineering** (§13.6): start-bit
  computation (128-byte round-up + 32-bit alignment), table-upload
  protocol (command = engine table-RAM address: lit symbols 0x000,
  first/cum 0x180/0x190, dist first/cum 0x1A0/0x1B0, dist symbols
  0x1C0), the canonical table builder at 0xA2001C5C (counting sort),
  the completion accounting formula `(R381C + acc<<16) * (8 << state) +
  field10`, and the poll countdown preloading (`limit − 1`).
- Analysis tools: `tools/fw_engine_lz.py` (independent symbol walk +
  RFC reference decode), `tools/fw_engine_lz2.py` (divergence map +
  frozen-pointer proof), `tools/fw_driver_disasm.py` (driver
  disassembly).

### Verified (independent from-scratch tooling)
- The container's bit stream parses exactly under canonical Huffman for
  its whole length (25,027 symbols, zero bad codes); the engine consumes
  exactly RFC 1951 bit positions.
- The driver-built Huffman tables (dumped intact from emulated DRAM) are
  canonical with a one-index `first[]` convention; the code → symbol
  assignment is standard RFC 1951.
- Output size program: 78 × 1024 = 79,872 bytes (chunk registers,
  accounting formula, and header u16@2 agree); RFC 1951 yields 75,657
  from the engine's input window — the ~4.2 KiB gap is the remaining
  value-semantics unknown (§14).

## 0.6.0 — Milestone I: "Go eCos" — the boot chain enters the eCos stage

### Changed (evidence-driven corrections)
- **Engine completion line moved from bit 6 to bit 5 of 0xBFFE384C**
  (docs/firmware-analysis.md §13.3): full re-read of the driver's poll
  showed the bit-6 branch is the *failure* epilogue (returns 0 →
  "LD eCos fail"), while the bit-5 branch acks (writes 0x2000), counts
  the event against the limit mirrored from the container header
  (u16@2 = 78, second counter pre-loaded with 77 so the first event
  satisfies it), and routes to the success block that returns
  0x400 × 78 = 0x13800.
- Engine decode reworked to the programmed semantics
  (`inflate::inflate_engine`): symbols from the start-bit register
  (R3818 = 1088 container bits = 1024 stream bits — the 16-bit gap after
  the 1008-bit header is zero padding, verified in the stream) through
  the end of the input length register (0xBFFE3838 = 0x8692), with
  end-of-block symbols as markers instead of terminators.
- KeyPort: the engine event ORs into bit 5; the 0x2000 ack write clears
  it (the driver's bit-5 handler writes exactly that value).

### Verified behavior (real firmware, deterministic, ~1.31 M steps)
- boot → "LD eCos v1.16 1506f" → engine decode (75,657 bytes written to
  0x01600000 through the bus) → bit-5 event → driver returns 0x13800 →
  **"Go eCos" + "Boot from SPI"** → jump to the decompressed eCos entry
  at 0xA1600000. First entry into the eCos stage; every value on the
  success path is real firmware state (§13.5).

### Open
- The engine's LZ value semantics: the bit-stream parse is provably exact
  but the output bytes are not yet valid MIPS code (~83% word validity).
  Next diagnostic anchor: the eCos entry execution trace (§14).

## 0.5.0 — Milestone H: hardware decompression engine handshake

### Added
- **Boot progress**: the real firmware now passes the post-"LD eCos" wait
  loop for the first time. The loop was the decompression engine's wait
  (also the front-panel key scan), not a UI idle loop.
- New devices `DecompEngineLo`/`DecompEngineHi` (0xBFFE3800..0xBFFE3878):
  register storage, table-upload FIFO (0xBFFE3844/0xBFFE3848), start
  handshake (0xBFFE3824 bit 0), busy/done flags; the machine run loop
  services the engine with real bus reads/writes.
- New `src/inflate.rs`: dependency-free RFC 1951 DEFLATE decoder. The
  compressed "SP" container at flash 0x1002C is a raw deflate stream whose
  Huffman tables match the ones the firmware uploads bit-for-bit.
- `KeyPort` reads OR the engine-done flag into bit 6 (the loader polls it).
- `tests/decomp.rs`: real-container decode KAT + machine-level handshake
  reach; engine/inflate unit tests.

### Verified behavior (real firmware, deterministic)
- boot → "LD eCos v1.16 1506f" → engine decodes 28,256 bytes → wait loop
  exits via 0xBFFE384C bit 6 → loader result check → "LD eCos fail" →
  deterministic dead-end at 0xA2000D90. The success exit needs the full
  0x8684-byte decode; the continuation is a per-symbol co-processor
  protocol (open — docs/firmware-analysis.md §13.4).

## 0.4.0 — Milestone G: front-panel key port decode + virtual button injection

### Added
- `devices::KeyPort` at sysreg+0x384C: the front-panel key data port,
  decoded from the main firmware's key-scan dispatcher — read = raw
  key-line bits (lines 0/2/4/5/6 tested), write = keycode acknowledgment
  (bit 0→0x100, 2→0x400, 4→0x1000, 5→0x2000) captured into a shared event
  queue. Level semantics; no invented ack/latch.
- Virtual button injection API: `Machine::press_key` / `release_key` /
  `take_key_events`.
- End-to-end proof on the real firmware: boot to the idle poll, inject
  key-line bit 2 → the firmware's own dispatcher emits keycode 0x400;
  releasing the line stops the events
  (`real_firmware_reacts_to_injected_key_press`).
- `docs/firmware-analysis.md` §12: key-port register decode, the
  0xBFFE3874..0x387C latch/strobe pair, bit-6 long path, unknowns.

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
