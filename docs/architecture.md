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
│   Machine::run/run_traced: step → bus.tick() → StopReason/touchmap      │
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
              │  DdrPhy · DdrCtrl · ParamRam · MirroredRam ·    │
              │  LoggingDevice                                  │
              └─────────────────────┬───────────────────────────┘
                                    │
              ┌─────────────────────▼───────────────────────────┐
              │ cpu/mod.rs — trait CpuBackend                   │
              │   step(bus) → StepReport { pc, word, desc, mem} │
              │ cpu/mips32.rs — the interpreter                 │
              │ cpu/cop0.rs  — COP0 subset (Status/Config1/…)   │
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
- **Memory map as data** (`MachineConfig`): all windows (DRAM 64 MiB,
  flash XIP at phys 0x1FC00000, IRAM overlay 0x1FFC0000, sysreg page
  0x1FFE0000, mirror window 0x18000000) are declared, documented fields —
  changing a hypothesis is editing a `Default` impl.
- **Determinism**: no wall-clock, no RNG; same image + same budget ⇒
  identical run (asserted by tests).
- **Errors are values**: every fault carries vaddr/phys/kind; the CLI
  distinguishes clean stops (exit 0) from CPU faults (exit 2).

## Testing strategy

- Unit tests per module: translation, endianness, COP0 geometry, every
  device's observed protocol (incl. the exact loader SPI transaction).
- Integration tests run the **real committed firmware**: stage-1 execution
  past the prologue, and the full boot chain to the eCos idle poll
  (`real_firmware_boots_through_ddr_training_into_main_firmware`).
- Quality gates: `cargo fmt --check`, `cargo clippy --all-targets
  --all-features` (0 warnings), `cargo test --release`.
