# Changelog

## 0.22.0 — Milestone Z: the per-step loop measured and closed (+30 % boot throughput)

### Added
- `examples/loop_probe.rs` — the §28.1 attribution probe: full-run
  anchor plus isolated microbenches (tick fan-out whole/intc/timer,
  engine poll, pending hint, watch check, raw-core ceiling).
- `tests/looptune.rs` — three regression pins: the intc
  change-detection tick (raised sources assert on the next step
  boundary; enable writes immediately), the engine pending-hint
  lifecycle (set on post, cleared by take_request, take stays
  authoritative), the shared timer counter cell (shared increments,
  Device::tick compat, software reset).
- `TimerCounter::with_count` — shared-counter constructor (§28).

### Changed
- `IntController::tick` recomputes the output line only when the
  external sources changed since the last step (enable-register writes
  recompute immediately as before) — two `BTreeMap` lookups per
  instruction become one compare.
- The SoC timer counter lives in a `Rc<Cell<u32>>` the run loops
  increment directly; the timer is registered non-tickable.
  `Device::tick` semantics unchanged for standalone buses.
- `EngineShared::pending` (`Rc<Cell<bool>>`): the per-step engine poll
  checks a plain Cell instead of `RefCell::borrow_mut`;
  `take_request` stays authoritative.

### Results (docs/firmware-analysis.md §28)
- Deterministic boot chain: 19.50 → 25.4 M instr/s (+30 %), identical
  1,301,783 steps; all prior tests unchanged (119 total).
- Tick decimation refuted for this repo (§28.5); the decode-cache
  verdict (§24.5) stands with post-V data.

## 0.21.0 — Milestone Y: the hardware-session handoff (P1/P2 one-command ingestion)

### Added
- `Machine::compare_dumps` / `Machine::classify_residue` (`src/machine.rs`)
  — the pure, integer-only comparison behind the §22.5 handoff: matching
  count/per-mille, first divergence, 1024-byte block maps, longest runs;
  size mismatch compares the shorter prefix and records both lengths.
  Deterministic; pinned by `tests/hwsession.rs` (10 tests, incl. the
  real-firmware pixel-reference extraction and a determinism re-pin).
- `examples/dram_compare.rs` — one command per decisive experiment:
  `--post` (boot + engine reference vs device capture), `--pre` (residue
  classification vs the §18.3 all-zero expectation), `--ref` (offline
  file-vs-file); `--out` writes the block map; exit codes 0/1/2
  distinguish clean matches from actionable divergence.
- `docs/hardware-session.md` — the capture protocol (windows, sizes,
  halt points, JTAG/EJTAG plan explicitly labeled unvalidated,
  integrity checklist) and the pre-registered decision tree for P1/P2.

### Findings (docs/firmware-analysis.md §27)
- No new hardware claims: the full path is verified in-emulator only
  (synthetic perturbations localized exactly; boot determinism re-pinned
  through the comparator's extraction path). §26's refutations stand;
  the SP stream remains hardware-blocked — but the session cost is now
  one command per experiment.

## 0.20.0 — Milestone X: the combined length+distance class refuted jointly

### Added
- `tools/fw_combined_semantics.py` — the §16.3/§25.5 surviving class
  tested as such for the first time: 72 length families (RFC control,
  the three §16.3 exact-count predicates, the 64 §15.6-style exact-size
  group-uniform ladders, code-shifted ladders) × 35 distance families
  (RFC control, global offsets to ±8192, code-shifted tables) on the
  evidence-pinned exact parse, scored by three pre-registered oracles
  (size within one row of 79,872, static head filter, strict capstone
  validity ≥ 90 %, plus ASCII-run and eCos-marker reporting).

### Findings (docs/firmware-analysis.md §26)
- Baselines reproduce exactly: the RFC control decodes to 75,398 bytes
  (§25.3's anchor) and the exact-size ladder family yields exactly the
  64 solutions §15.6 recorded (all ~50 % strict with RFC distances).
- **Zero survivors in 2,520 combinations**: best strict validity 55.0 %
  (size-qualified) vs the 91–95 % code band; the §16.3 predicate
  families peak at 54.2 %; zero eCos markers anywhere.
- Consequence: every hypothesis class reachable without hardware on the
  SP stream is now refuted with data; the remainder is hardware-side
  residue (§22.5) or the §18.4 m2/m4 accounting solve that §19.3's
  head-DFS already structurally de-prioritized.

## 0.19.0 — Milestone W: the chunk-resync dialect refuted (§16.5's three open probes closed)

### Added
- `tools/fw_chunk_resync.py` — one decisive experiment combining the three
  §16.5 probes: per-chunk framing from the 128-byte rounding rule, the
  dialect meaning of the two mid-stream EOB symbols, and tail alignment to
  the engine's start-bit + k×1024 grid. Sweeps 4 semantics (eob-grid /
  hard-grid / eob-byte control / continuous control) × 4 grid origins × 4
  periods (36 walks) with scoring by output size, strict validity (the
  capstone word oracle), ASCII runs and eCos markers; `chain`, `gap` and
  `sweep` subcommands.

### Findings (docs/firmware-analysis.md §25)
- **New structural fact**: the 16 body-gap bits (stream 1008..1023) are
  all zero — the encoder padded the dynamic header to the 128-bit
  boundary; `PNG_BS_OFFSET = 1088 = 64 (SP header) + round_up(1008, 128)`.
  The "body = stream bit 1024" convention is now evidence-backed.
- Baselines reproduced exactly (out 75,398 / EOB#1 106,259→28,260 /
  EOB#2 45,137 / head `60 81`), validating the tool against §16.1/§16.3.
- **Verdict**: no grid variant approaches the 79,872-byte image (best
  76,799, Δ ≥ 3,073) and validity stays on the 46–49 % plateau; every
  walk finds exactly 2 EOBs and runs to stream end. With the tool's
  premise evidence-forced (the tables are uploaded once, §13.2), the
  entire "same-tables, re-framed tail" class is refuted; the remaining
  space stays as §21.6 bounded it.

## 0.18.0 — Milestone V: performance (13.4× boot throughput, no semantic change)

### Fixed
- **Device access logs are O(1) rings** (`src/devices.rs`, all 12 record
  sites): the capped log used `Vec::remove(0)` at capacity — an O(n)
  128–256 KiB shift on every logged device access once full. The boot's hot
  windows (param RAM, DDR PHY/controller, UART, keyport, engine registers)
  log on every access, so this one line dominated whole runs. Backed by
  `VecDeque` (`push_back`/`pop_front`), identical capped/oldest-dropped
  semantics; the touchmap and every log-consuming test are unchanged.
- Untraced run loops no longer build and drop a formatted trace `String`
  per executed instruction: `CpuBackend::set_describe` gates
  `StepReport::desc` formatting (`run`/`run_until_pc` disable it,
  `run_traced` keeps it). Pinned by a CPU unit test (identical PC sequences
  with and without descriptions; `desc` empty iff disabled).
- `set_ext_irq_lines` returns early while the platform lines are unchanged
  (the steady state of the entire observed boot); `watch_armed`
  short-circuits on an empty watch list.

### Added
- `examples/bench.rs` — reproducible throughput harness over the
  deterministic real-firmware boot chain (the roadmap item 5 instrument).
- A hashed hot-region memo for physical region lookup (4 direct-mapped
  slots, range-validated on hit, filled on miss; `add_region` asserts the
  non-overlap invariant). Transparency pinned by a slot-conflict unit test.
  Measured neutral on the current region layout; kept as insurance.

### Results (docs/firmware-analysis.md §24)
- Deterministic boot chain: 1.45 → 19.4–19.6 M instr/s (13.4×); 100 M-step
  runs ≈ 5 s; full test suite ~29 s → ~3.2 s.
- Semantic equivalence: all 104 pre-existing tests pass unchanged,
  including the deterministic boot-chain step pin and the §20/§22/§23
  register-state/watchpoint/COP0 pins.
- Decode-cache verdict: refuted as the needed mechanism by measurement
  (raw interpreter core already 71.6 M steps/s); optional future work.

## 0.17.0 — Milestone U: device coverage (Cause fixes, COP0 timer, keycode verdict)

### Added
- COP0 Count/Compare timer interrupt (MIPS32r2 architecture completion):
  match-edge pending latch, ack-by-Compare-write, IP7 input composed into
  Cause reads, read-only Cause.TI, DC gating of delivery/readout, and
  IM7/IE delivery through the existing exception path. 7 COP0 unit tests
  plus the end-to-end synthetic delivery test
  `tests/interrupt.rs::count_compare_timer_interrupt_delivers_via_ip7_and_acks`
  (arm Status = IE|IM7 and Compare, idle, handler acks with a far Compare
  write, marker + eret pinned).
- SoC timer block tests (`tests/timer.rs`): counter tick / software-reset /
  config-store semantics with logged unmodeled-register reads, and the
  real firmware's complete timer register program pinned (writes +0x00=0
  once, +0x0C=0x8095, +0x1C=0; 226 counter reads, max count 0x385).
- Keycode-consumer instruments: `tools/fw_keycode_consumer.py` (chunked
  linear disassembly with constant propagation over the whole payload,
  99.0% coverage) and `examples/keycode_consumer.rs` (watchpoint
  attribution of every key-port access on the real boot), plus the pin
  test
  `tests/interrupt.rs::keycode_port_traffic_comes_only_from_the_dispatcher`.
- `examples/timer_probe.rs` — extracts the SoC timer register program
  from a real boot.

### Fixed
- **COP0 Cause bit layout** (`src/cpu/cop0.rs`): BD is bit 31 (was
  wrongly bit 22 = WP), IV is bit 23 (was wrongly bit 30 = TI), TI added
  as read-only bit 30. Cross-checked against Linux
  `arch/mips/include/asm/mipsregs.h` (`CAUSEF_*`); pinned by regression
  tests. The real boot never reads these fields (IE=0, §11) and is
  unaffected — all boot tests pass unchanged.

### Analysis (§23)
- The keycode consumer is a closed negative: exactly six payload accesses
  resolve to 0xBFFE384C, all inside the dispatcher (setup read 0x82002518,
  four keycode stores, loop re-read); dynamically, every port access in a
  30M-step boot with keys held issues from dispatcher PCs. The keycode is
  consumed beyond the horizon (eCos stage/hardware); button labels remain
  unknown. Runtime confirmation that the engine event (bit 5) shares the
  key-port read (reads show 0x24 = key line 2 + event).
- The SoC timer block is used polled-only: one delay(10)-style poll
  (90×10 + 1 overshoot = 901 max), two one-shot config writes; no IRQ
  line observed or claimed.

## 0.16.0 — Milestone T: the Q8 runtime verdict (no checksum code path)

### Added
- Physical-range watchpoints: `AddressSpace::add_watch`/`clear_watches`/
  `take_watch_hits` (+ kind filtering via `WatchKinds`, capped hit buffer
  with an explicit overflow flag) and `Machine::run`/`run_until_pc`/
  `run_traced` stamping the executing (step, PC) so every hit attributes
  to the instruction that issued the access. 9 mechanics tests in
  `tests/watch.rs`.
- `examples/q8_watch.rs` — the §22 instrument: boots the real firmware
  with watches over the descriptor words, driver payload, eCos container
  (both flash windows) and the payload's DRAM destination, reports per-
  range PC histograms with file-offset attribution and ordered traces,
  then continues past the eCos entry with a pixel-buffer data-read
  watch. 5 runtime-evidence tests in `tests/q8.rs` pin the verdicts.

### Fixed
- docs/firmware-analysis.md §15.2: the driver-payload checksum anchor is
  `0x5DB06FB5` (bytes `b5 6f b0 5d` at 0x1224), not the byte-swapped
  `0xB56FB05D`; `tools/fw_ck_sweep.py` now targets the corrected value
  (its negative result stands — it always compared both orders). The
  stage-1 descriptor field readings updated per §22.3 (0x121C = source
  offset from base 0x1C10; 0x1218 never read).

### Analysis (§22)
- The stage-1 loader reads exactly three descriptor words (source offset
  0x121C = 0x135C, dest 0x1214 = 0xA2000000, length 0x1220 = 0x3ADC) and
  computes the source as hardcoded base 0x1C10 + offset → file 0x2F6C;
  the checksum word 0x1224 is never read by any code.
- The payload copy is a one-pass `lwl/lwr`+`sw` loop (no accumulator);
  the 19 post-copy destination reads are the driver's config table; the
  container streaming loop (`lbu; sb; bnez`) accumulates nothing and
  returns zero unconditionally; neither anchor constant exists in the
  image outside its descriptor word; no post-entry data-read pass over
  the image occurs.
- Verdict: no checksum validation exists anywhere in the observed boot
  chain — the anchors are packaging metadata and the Q8 oracle does not
  exist on-device; the engine LZ head paradox is bounded by hardware-only
  residue (§22.5).

## 0.15.0 — Milestone S: rank classes and output transforms refuted

### Added
- `tools/fw_rank_transform.py` — the rank/T-byte class-rule and output-
  transform family generator: a parameterised walker (class rule =
  value/rank/T-byte, length ladder = RFC/uniform, extra-bit accounting =
  m1..m4, start bit) with three self-checks gating every verdict (the
  sym-rule m1/m2/m4 walks reproduce the §19.2/§20.3 static verdicts
  byte-for-byte; the m1 identity output byte-matches the independent
  `fw_syms_family.render`), plus the static head-oracle prefilter and
  dynamic-oracle invocation list.
- docs/firmware-analysis.md §21; docs/roadmap.md Milestone S.

### Analysis (§21)
- The upload inventory (§13.6) closes the class-rule input space: only
  syms/first/cum are ever uploaded, so the token class is
  f(rank, T[r], first[], cum[], ladder-ROM). No public PNG_DEC
  documentation exists (Team-Proton/linux-sunplus-15xx has no PNG_DEC
  code) — external check 2026-10.
- The rank-boundary rule: head `60 81 03 43 1a 6c 02 1c` is ladder-
  independent (formed before any match); the register-aware dynamic
  oracle kills all ten variants at step 0 on `0x43038160` = COP0 rs=0x18
  (reserved on the real 24KEc too); all rank walks under-fill the image
  (45.6–52.9 kB vs 79,872).
- Start-bit variants under the rank rule (1008..1216 × m1/m2): all
  refuted (fetch fault at 0xA8080AC8; unknown delay-slot word; static
  fatals).
- The T-byte boundary rule: statically fatal before any branch.
- The output-transform family: 96 candidates (23 period-4 lane perms ×
  m1/m2, 7 period-8 rotations, swap16, bitrev8, nibswap, row mirror, and
  both image-shaped transposes) — 24 static survivors, all dynamically
  dead ≤ 14 steps, zero budget-survivors.
- Consequence: the head paradox is evidence-bounded; remaining space =
  Q8 checksum escalation, hardware residue, or unevidenced two-tier
  rules (§21.6).

## 0.14.0 — Milestone R: the entry harness (register-aware dynamic oracle)

### Added
- `Machine::run_until_pc(max_steps, watch)` — run loop stops before
  executing the instruction at the watch PC (`StopReason::EntryReached`).
- `CpuBackend::restore` + `Machine::restore_cpu_state` — full GPR/hi/lo/PC
  snapshot restore (entry-harness support).
- `AddressSpace::copy_out`/`copy_in` — bulk diagnostic snapshot/restore of
  RAM/ROM regions, with unit tests.
- `examples/entry_oracle.rs` — the decisive instrument §19.4 asked for:
  boots the real firmware with a watch at 0xA1600000, captures the loader's
  post-jump register state (v1 = 0x13800) and full DRAM, then evaluates
  candidate decodes as real MIPS code from the pixel buffer; budget-
  survivors are automatically screened for NOP-sled artifacts (distinct
  PCs, zero-word fetch share, UART emission).
- `tests/entry.rs` — 5 integration tests: entry register state, baseline
  dynamic fault at the static oracle's word, complete state restoration
  (bit-identical replays), step-limit path.
- `tools/fw_syms_probe.py` — pins the litlen table structure: all 286
  symbols coded, canonical rank(s) != s, length symbols 276/285 at ranks
  256/257 as bytes 20/29.
- `tools/fw_syms_family.py` — the symbol-mapping family generator
  (class-preserving literal-value maps + structure representatives), with
  an independent rank-walk cross-checked byte-identical against the event
  renderer.
- docs/firmware-analysis.md §20; docs/roadmap.md Milestone R.

### Analysis (§20)
- The class-preserving literal-value family is refuted by the dynamic
  oracle: 522 candidates (xor/add sweeps, bitrev8/nibswap, lut-by-value,
  rank-shifts, per-length reversal, m2/m4 re-walks) — 519 fault in the
  head, the 3 budget-survivors are NOP-sled artifacts (99% zero-word
  fetches, no UART).
- §19.2's static m1/m2 verdicts confirmed dynamically (m1 +0x50
  `0000ffff`; m2 +0x18 `6200003b`); the m4 verdict is corrected: the
  head's `blez $v1` falls through (v1 = 0x13800 > 0) and the head dies at
  +0x38, not +0x6c/+0x74.
- Remaining head-paradox space narrowed to: (1) a rank-driven class rule
  (the byte table cannot carry the literal/length split), (2) an unmodeled
  output transform, (3) a pre-transformed container plaintext.

## 0.13.0 — Milestone Q: the executable-head oracle; accounting/convention families refuted

### Added
- `tools/fw_first_corruption.py`: per-byte emit attribution of the engine
  decode (literals = ground truth, matches = suspect), a MIPS32r2 word
  classifier, and `--survey` mode (first reserved word / first branch per
  accounting).
- `tools/fw_head_sweep.py`: uniform extra-bit-width sweep (len/dist fields
  at fixed widths 0..8) against the executable-head oracle.
- `examples/pixel_dump.rs`: boots the real firmware and dumps the engine's
  pixel output with the PNG geometry read from the engine registers.
- docs/firmware-analysis.md §19: the head paradox, the refutation record,
  the structural conclusion (ladders cannot fix literal-byte faults), and
  the designed head-branch DFS.

### Analysis (§19)
- The head paradox: the boot provably executes LZ output byte 0, yet every
  tested accounting (m1..m4 × uniform widths × start bits × table-RAM
  conventions) reserved-faults within +0x74 on literal bytes.
- Consequence: length/distance ladder values are irrelevant to the entry;
  the remaining hypothesis space is the symbol→value mapping or an
  unmodeled output transform, both parse-preserving.

## 0.12.0 — Milestone P: the size anchor; m2 length-ladder refuted; pre-decode DRAM bounded

### Added
- `examples/ladder_search.rs` — the Milestone P ladder hunt: the 29
  length-ladder values as unknown ROM constants under the m2 accounting,
  searched against the image-truncated validity oracle with the new
  linear size anchor (in-band seeding, band-preserving coordinate
  descent with pair moves, monotone projection). Oracle = the crate's
  own MIPS32r2 decoder calibrated on the flash's code regions.
- `tools/fw_ladder_verify.py` — capstone cross-check of the reported
  finalists on the published §14.2 oracle.
- `examples/dram_prebound.rs` — canary/fresh boot replays bounding the
  pre-decode DRAM content below the PNG pixel buffer.
- `AddressSpace::poke_phys` — diagnostics-only physical write (RAM and
  ROM; the CPU-facing store still faults on ROM), with tests.

### Resolved
- **The size anchor** (§18.1): the m2 symbol path is value-independent,
  so the full-parse decode length is linear in the ladder; the RFC
  ladder gives 103,014 bytes vs the 79,872 image — Milestone O's
  "exact fill" was the walk cap, not an input-end property.
- **The m2 length-ladder family is refuted** (§18.2): no size-feasible
  ladder beats the baseline beyond oracle-exploitation noise (+2.5
  points, unstructured); unconstrained high scores are
  size-infeasible.
- **Pre-decode DRAM below the pixel base is never written by the boot**
  (§18.3): 0 of 1 MiB across canary + fresh replays; device determinism
  forces the engine's dist>output source to the zero-filled band
  buffer; RFC-impact bound 458 matches / 3,001 bytes (3.98%).

### Documented
- docs/firmware-analysis.md §18; docs/roadmap.md Milestone P. Next: the
  bit-accounting hypothesis space (m4 both-ladders joint solve), Q8.

## 0.11.1 — Milestone O: the ladder hunt (analysis session)

### Added
- `tools/fw_pixel_lz.py` — fixed-filter pixel-domain LZ probe (5 PNG
  filters, both directions) — refuted.
- `tools/fw_fixed_ladder.py` — walks that consume NO distance extra bits
  (fixed 30-entry distance ladder): the only model family that fills the
  79,872-byte image exactly from the stream; ladder mini-grid recorded.
- `tools/fw_beam_lz.py` — beam search over per-match length adjustments
  with exact incremental word-validity scoring: 73.8% oracle upper
  bound, no consistent delta rule (oracle exploitation).
- `examples/pixel_precontent.rs` — dumps the pre-decode DRAM around the
  pixel buffer and the band-buffer zero-fill state.

### Documented (docs/firmware-analysis.md §17.5)
- Fixed-filter unfiltering refuted in both directions; uniform +k/match
  re-scored with image truncation (the PNG framing invalidates the old
  full-output scoring of size-overflowing ladders) — still refuted;
  distance-symbol histogram supports an RFC-like ladder ordering (the
  length side is the likelier defect); row-boundary predicates refuted;
  next probes listed for Milestone P.

## 0.11.0 — Milestone N: the engine is a PNG decoder; row semantics modeled; shift/field-order/filter families refuted

### Added
- `tests/png.rs` — pins the firmware's PNG register program on the real
  firmware (IMG_WIDTH 0x400, IMG_HEIGHT 78, IMG_TYPE 0x68, BLK_INFO 5,
  PXL_LINE 77) and the completion accounting (BS_USED_CNT 624,
  last_result 79,872).
- `Machine::decomp_reg(off)` / `Machine::decomp_last_result()` — engine
  register/result accessors for diagnostics and tests.
- `EngineShared::set_reg` — machine-side engine-internal register writes
  (completion feedback).
- Analysis tools: `fw_png_attack.py` (682 exact-count candidates vs the
  PNG filter-byte oracle), `fw_png_rows.py` (raw-filter row geometry),
  `fw_png_ladder.py` (135 exact-size ladders vs the PNG oracle),
  `fw_field_order.py` (match-token field orders), `fw_shift_decode.py`
  (the §16.3 predicates decoded, not just counted), `fw_tail_probe.py`
  (Kraft-validated header search + validity profile),
  `fw_literal_diagnostic.py` (literal-ancestry provenance).

### Changed
- `Machine::service_decomp_engine` now models the PNG row semantics: the
  output is padded/truncated to the programmed PNG_IMG_WIDTH x
  PNG_IMG_HEIGHT pixel image (rows complete from the zero-filled band
  buffer when the bitstream ends early), and PNG_BS_USED_CNT reports the
  completed 128-pixel units (624 x 128 = 79,872 — equal to the real
  device's DRAM-pinned accounting). Engine register docs now carry the
  PNG_DEC_* names (docs/firmware-analysis.md §17).

### Resolved
- **The engine's identity** (§17.1): PNG decoder — register map, image
  geometry, per-row events, band/scaling buffers, and the jump-into-pixel
  output all cohere.
- **The §16.3 size-delta premise** (§17.2): 79,872 is the image
  allocation (row padding), not a decoded-byte claim; the SP tail
  "structure" question is moot.

### Documented (docs/firmware-analysis.md §17, docs/roadmap.md)
- Refuted with decoding (not just counting): the §16.3 position-shift
  predicates (48.5/48.2/46.8/48.0% vs baseline 48.3%), all four
  match-token field orders, raw-8-bit PNG filter geometry, 682
  exact-count candidates + 135 group-constant ladders for the filtered
  target, global distance offsets -16..+16 (flat 48-50%), and any
  second dynamic block header after EOB#1 (Kraft-validated search).

## 0.10.0 — Milestone M: Q2 resolved (windows named by evidence); SP stream structure probed; CI

### Added
- `Machine::access_log_names()` — names of all instrumented windows, for
  phase-by-phase access correlation; covered by
  `tests/boot.rs::instrumented_window_names_cover_the_q2_windows`.
- `examples/window_correlation.rs` — runs the real boot in step segments,
  segments it by UART milestones, and reports per-phase per-window access
  deltas plus first-touch steps (the Q2 evidence generator).
- Analysis tools (all reproducible against the pristine flash):
  `fw_stream_structure.py` (SP block-chain decoder, exhaustive
  terminating-header search, Kraft completeness check),
  `fw_ck_sweep.py` (~250 checksum families vs the two anchors),
  `fw_shift_predicate.py` (exact-count length-accounting predicates for
  the +4,474 output delta).
- CI: `.github/workflows/ci.yml` — fmt/clippy/test/check on push and PR.

### Resolved
- **Q2** (docs/firmware-analysis.md §16.4): 0x98000000/0x98010000 are the
  flash mirror's code-fetch and data-read paths (the 0x10000-package
  descriptor reads land there during "LD eCos"); 0xB0000000/0x90000000/
  0xF0000000 are **address-classification masks** in the loader's
  dispatcher (mask-and-compare chains, no dereference) — the phys
  0x10000000/0x30000000 windows are never accessed by the observed boot.
  `MachineConfig` field docs and the module map updated accordingly
  (0xF0000000 corrected to kseg2).

### Documented (docs/firmware-analysis.md §16, docs/roadmap.md)
- The SP container is **not** one deflate block: block 1 is a well-formed
  FINAL dynamic block (ends at bit 106,259 with 28,260 bytes); 169,229
  bits follow with no standard continuation (dynamic/fixed/stored all
  refuted by exhaustive per-bit search); both Huffman tables are complete
  (Kraft = 1), so "zero bad codes" never protected the single-block tail.
- The checksum anchors are not reproduced by ~250 standard families —
  the algorithm is custom or the coverage differs (Q8 open).
- Position-shift reframing of the LZ search: only output bytes 0..1 are
  positionally certain; exact-count predicates for a +2-byte-per-match
  shift reported (`dist<96`, `dist≥3485`, `len≥5&dist<3073`, 2,237
  matches each).

## 0.9.0 — Milestone L: container formats resolved; engine LZ narrowed by five refutation families

### Added
- `tests/boot.rs::driver_payload_is_stored_plaintext_in_flash` — pins the
  Milestone L discovery that the driver payload (0x3ADC bytes) is stored
  **plaintext** at flash 0x2F6C and "copy fw to RAM" is a byte-for-byte
  copy (100.000% verbatim vs the post-boot DRAM state).
- Analysis tools (all reproducible against the pristine flash):
  `fw_dist_stats.py` (symbol-stream distance/length statistics),
  `fw_ring_sweep.py` (ring-size sweep + calibrated strict-validity oracle),
  `fw_corruption_map.py` (per-chunk/burst corruption structure),
  `fw_align_copy.py` (word-aligned copy-source variants),
  `fw_snapshot_copy.py` (snapshot-copy variant + delta-class search),
  `fw_keydetect.py` (position-periodic write-transform detection),
  `fw_length_search.py` (size-constrained length-base-table search),
  `fw_checksum_oracle.py` (checksum anchor sweep).

### Documented (docs/firmware-analysis.md §15, docs/roadmap.md)
- **Q4 resolved for the driver**: payload plaintext at flash 0x2F6C;
  §4/§10.3 corrected accordingly.
- The 0x10000 `CED9D8CC` package structure: 12-byte header + 28-byte
  descriptors at 0x100010+idx*28 with the full field map (src, len, dst,
  len2, flags, driver arg, unused checksum) and the flags 0/1/2 dispatch.
- **flags 2 = software LZMA decoder** (0xA2002EF8 → 0xA200304C): props
  byte div-45/mod-45/div-9 = pb/lp/lc, u64 LE size at bytes 5..13 — the
  first software decompressor identified in the SDK image.
- Two checksum anchors recorded (driver payload 0xB56FB05D, eCos
  descriptor 0xB5EBF9FF); simple checksum families swept without a match
  (Q8).
- Engine window-usage scan: no engine access outside the already-decoded
  loader/driver/key-dispatcher regions — no hidden engine modes.
- Engine LZ value semantics: five new refutation families (ring sizes
  with a W=32768 ≡ RFC cross-check, word/halfword-aligned copy sources,
  snapshot copy, position-periodic write transforms, size-constrained
  length-base-table groups) plus the corruption-structure map (validity
  decays 67→45%, not chunk-periodic; match-class shares flat). Open
  questions Q7 (copy-path source fields) and Q8 (checksum algorithm)
  recorded.


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
