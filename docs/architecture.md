# Architecture

## Goals

1. Emulate the Truman TM X1 WiFi (Sunplus SPHE150x, MIPS32r2 LE) from the
   real flash image, driven by evidence rather than guesswork.
2. Keep every hypothesis swappable: a wrong device model is a configuration
   or small-model change, not a rewrite.
3. Stay portable and dependency-free (pure `std`, no unsafe).

## Module boundaries

```
┌───────────────────────────── bin/truman-emulator.rs (CLI) ─────────────┐
│  inspect / run / trace                                                  │
└────────────────────────────────┬───────────────────────────────────────┘
                                 │
┌────────────────────────────────▼───────────────────────────────────────┐
│ machine.rs — composition + run loop                                     │
│   MachineConfig: the documented memory map (hypothesis A')              │
│   Machine::run/run_traced: intc line → CPU → step → bus.tick()          │
│   raise_int_sources/clear_int_sources: virtual device injection         │
└───────┬──────────────────────────────┬──────────────────────────────────┘
        │                              │
┌───────▼──────────┐        ┌──────────▼──────────────────────────────────┐
│ boot.rs          │        │ memory.rs — AddressSpace                    │
│ reset PC 0xBFC0_ │        │  kseg0/kseg1 → phys translation             │
│ 0000 + diag      │        │  regions: Ram / Rom / Device(Box<dyn Dev>)  │
└──────────────────┘        │  tick() fan-out to time-dependent devices   │
                            └───────┬─────────────────────────────────────┘
                                    │
              ┌─────────────────────▼───────────────────────────┐
              │ devices.rs — trait Device { read/write/tick }   │
              │  Uart · TimerCounter · SpiController ·          │
              │  DdrPhy · DdrCtrl · IntController · ParamRam ·  │
              │  MirroredRam · LoggingDevice                    │
              └─────────────────────┬───────────────────────────┘
                                    │
              ┌─────────────────────▼───────────────────────────┐
              │ cpu/mod.rs — trait CpuBackend                   │
              │   step(bus) → StepReport { pc, word, desc, mem} │
              │   set_ext_irq_lines(lines): IP2..IP7 inputs     │
              │ cpu/mips32.rs — the interpreter                 │
              │ cpu/cop0.rs  — COP0 subset + interrupt delivery │
              └─────────────────────────────────────────────────┘
```

## Key decisions

- **CPU backend behind a trait** (`CpuBackend`): Unicorn 2 was evaluated and
  empirically rejected (cannot fetch from kseg segments — see
  `docs/firmware-analysis.md` §7). The interpreter is ~1200 lines, fully
  deterministic, and reports every step's PC/word/mem events for the tracer.
- **Devices receive region-relative offsets** and are wrapped by the bus;
  every device shares an access log (`AccessLog`) that feeds the run
  summary's "instrumented window access map". Stub behavior is always
  visible in that map — nothing silently pretends to work.
- **Time model**: `AddressSpace::tick()` runs once per executed instruction;
  `TimerCounter` increments per tick (1 count/instruction — the real tick
  rate is unknown; only flow semantics matter for boot).
- **Interrupts are wired, not guessed**: the CPU implements the
  architecture-specified delivery (Status/Cause/EPC/EXL/ERL/eret, vector
  selection) — see docs/firmware-analysis.md §11.4. The SoC interrupt
  controller is modeled as verified storage (the exact "IntCtrl" init
  program replays bit-identically) plus a deliberately minimal,
  level-triggered aggregation to CPU IP2 (hypothesis H11). The real
  firmware keeps `Status.IE = 0` for the entire observed boot, so the
  mechanisms are proven by dedicated tests that inject sources through
  `Machine::raise_int_sources` — not by pretending the firmware uses them.
- **Memory map as data** (`MachineConfig`): all windows (DRAM 64 MiB,
  flash XIP at phys 0x1FC00000, IRAM overlay 0x1FFC0000, sysreg page
  0x1FFE0000, mirror window 0x18000000) are declared, documented fields —
  changing a hypothesis is editing a `Default` impl.
- **Determinism**: no wall-clock, no RNG; same image + same budget ⇒
  identical run (asserted by tests).
- **Performance model** (Milestone V, §24): the run loop never pays for
  what it does not read — untraced loops disable per-step trace formatting
  (`CpuBackend::set_describe`), device access logs are O(1) `VecDeque`
  rings, watch recording is inert while no watch is installed, and region
  lookups go through a hashed hot-region memo. Measured effect: 1.45 →
  ~19.5 M instr/s on the deterministic boot chain (13.4×), semantics pinned
  unchanged by the full suite.
- **Errors are values**: every fault carries vaddr/phys/kind; the CLI
  distinguishes clean stops (exit 0) from CPU faults (exit 2).

## Testing strategy

- Unit tests per module: translation, endianness, COP0 geometry, every
  device's observed protocol (incl. the exact loader SPI transaction), and
  the interrupt mechanisms (COP0 vector selection, CPU delivery/eret/
  delay-slot KATs, IntController init replay + level output).
- Integration tests run the **real committed firmware**: stage-1 execution
  past the prologue, and the full boot chain to the eCos idle poll
  (`real_firmware_boots_through_ddr_training_into_main_firmware`), plus
  interrupt injection through the machine API
  (`tests/interrupt.rs`) and a guard that the real boot is unaffected by
  the interrupt wiring.
- Quality gates: `cargo fmt --check`, `cargo clippy --all-targets
  --all-features` (0 warnings), `cargo test --release`.
