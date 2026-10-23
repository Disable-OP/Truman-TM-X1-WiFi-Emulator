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

## Milestone M — Q2 resolved; SP stream structure probed; CI ✅

- **Q2 closed with runtime + static evidence** (§16.4): 0x98000000/
  0x98010000 = the flash mirror's code/data paths (the 0x10000-package
  descriptor reads land there during "LD eCos"); 0xB0000000/0x90000000/
  0xF0000000 are address-classification masks in the loader's dispatcher,
  never dereferenced — the phys 0x10000000/0x30000000 stubs stay visible
  and unused. Tool: `examples/window_correlation.rs` (UART-milestone
  phase segmentation of per-window access logs).
- **SP stream structure probed** (§16.1, `tools/fw_stream_structure.py`):
  block 1 is a well-formed FINAL dynamic block (ends at bit 106,259,
  28,260 bytes); 169,229 bits follow with **no standard continuation**
  (dynamic/fixed/stored refuted by exhaustive per-bit search); both
  Huffman tables are complete (Kraft = 1), so the single-block walk was
  never actually verified past EOB#1.
- **Checksum sweep** (§16.2, `tools/fw_ck_sweep.py`): ~250 standard
  families against both anchors — no hit; Q8 stays open.
- **Position-shift reframing** (§16.3, `tools/fw_shift_predicate.py`):
  only output bytes 0..1 are positionally certain; a length-accounting
  shift (+4,474 via exact-count predicates `dist<96` / `dist≥3485` /
  `len≥5&dist<3073`, all +2) is the remaining structural hypothesis
  class alongside the tail structure.
- **CI**: GitHub Actions workflow (`.github/workflows/ci.yml`) running
  `cargo fmt --check`, `clippy -D warnings`, `cargo test --release` and
  `cargo check` on push/PR.
- New API: `Machine::access_log_names()` (tested).

## Milestone L — container formats resolved; LZ narrowed by five families ✅

- **Q4 resolved for the driver**: the payload is stored **plaintext** at
  flash 0x2F6C (100.000% verbatim vs a post-boot DRAM dump); "copy fw to
  RAM" is a plain copy — pinned by
  `tests/boot.rs::driver_payload_is_stored_plaintext_in_flash`.
- The 0x10000 `CED9D8CC` package decoded: 28-byte descriptors at
  0x100010+idx*28; flags dispatch = 0 plain copy / 1 hardware engine /
  **2 software LZMA decoder** (props div-45/div-9, u64 size, decoder at
  0xA200304C) — a second codec found in the image (§15.3–§15.4).
- desc[6] checksum is never read by the loader; two checksum anchors
  recorded (Q8). Engine window-usage scan: no modes beyond the decoded
  driver/loader/key-dispatcher traffic (§15.5).
- Engine LZ value semantics narrowed by five new refutation families —
  ring-size sweep (incl. a W=32768≡RFC cross-check), word-aligned copy
  sources, snapshot copy, position-periodic write transforms, and the
  size-constrained length-base-table search (64 exact-size solutions,
  all ~50%) — plus the corruption-structure map: validity **decays**
  67→45% and is **not chunk-periodic** (§15.6).

## Milestone K — engine register program complete; LZ semantics narrowed ✅

- The driver's engine program is fully decoded from the runtime access
  log (`Machine::access_log` + `examples/engine_probe.rs`): input base /
  **input end** (`round128(base+len)`), 32 KiB zeroed window, output,
  32-bit input length = container+6, and `0xBFFE383C = (BTYPE<<1)|BFINAL`
  (docs/firmware-analysis.md §13.2).
- The LZ value-semantics search was widened (presets, transforms, ring
  indexing, remap families) against **calibrated** validity oracles and
  then narrowed: the wrongness is structural, not a wrong base table
  (§14.2–§14.4). Next: stage-2 decompressor disassembly (Q4) and
  chunk-periodicity mapping of the corruption.

## Milestone J — Engine interface decoded; decode defect fixed ✅

- **The driver's engine interface is fully reverse-engineered** (§13.6):
  start-bit computation, table-upload protocol (command = engine
  table-RAM address), the canonical counting-sort table builder, the
  completion accounting formula, and the poll countdown preloading.
- **The emulator's engine decode had a frozen read pointer** (every
  match emitted a constant fill of its first source byte). Proven by
  re-implementation (100.000% byte match against the previous output)
  and fixed with a regression KAT. All §13.4 measurements taken with
  the buggy decode are superseded.
- The bit-stream parse is independently verified exact (25,027 symbols,
  zero bad codes); the uploaded tables are verified canonical from the
  DRAM dump; the output size program is verified 78×1024 = 79,872.
- **Open (§14)**: the engine's LZ value semantics — RFC 1951 values give
  75,657 bytes of not-yet-valid code vs the programmed 79,872; the
  length mapping (hardware base tables) is the primary unknown, the
  desc[6] checksum algorithm the decisive missing oracle.

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
3. ✅ **Payload codec identification** — resolved in Milestone L: the
   driver payload is plaintext in flash (no codec); the eCos "SP"
   container is a DEFLATE-shaped stream consumed only by the hardware
   engine; the SDK also ships a software LZMA decoder (flags 2 path).
   Remaining: the engine's LZ value semantics (§15.6/§14).
4. ~~**0x98010000 / 0xB0000000 / 0xF0000000 windows**~~ ✅ resolved in
   Milestone M (§16.4): flash-mirror data path + address-classification
   masks; no peripherals found behind them.
5. **Performance**: ~5 M instr/s today; a decode cache would lift the
   100 M-step runs from ~20 s to a few seconds.
6. ✅ **CI**: GitHub Actions running fmt/clippy/test on push (Milestone M).
7. **SP tail structure + LZ value semantics** (§16.1/§16.3/§16.5): the
   primary boot blocker — decode the 169,229 bits after SP block 1.
