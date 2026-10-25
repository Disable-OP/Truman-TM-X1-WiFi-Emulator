//! Determinism invariants of the machine (Milestone BE, §58).
//!
//! The emulator's reporting contracts (the CLI tail re-run, the capture
//! comparison workflows, every step pin in the acceptance tests) assume
//! that execution is a pure function of the start state — independent of
//! how the step budget is *chunked* into `Machine::run` calls, whether
//! the description hook is on (`run_traced`), and how many times the
//! machine was rebuilt. Those invariants were never pinned chain-wide on
//! the real firmware; a refactor that let hidden state leak across calls
//! (timer accumulation, bus tick order, log rings, HashMap iteration)
//! would have passed the existing per-step unit tests.
//!
//! All three tests execute the real boot chain deterministically and
//! compare full end states: stop reason, step count, final pc, the UART
//! console bytes, the register snapshot (incl. hi/lo/pc/next_pc) and the
//! instrumented-window touchmap.

use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig, Tracer};

fn firmware_image() -> firmware::Image {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("firmware");
    let mut entries = std::fs::read_dir(&p).expect("firmware dir");
    let bin = entries
        .find(|e| {
            e.as_ref()
                .ok()
                .and_then(|e| e.path().extension().map(|x| x == "bin"))
                .unwrap_or(false)
        })
        .expect("firmware image")
        .unwrap()
        .path();
    firmware::load(&bin).expect("firmware loads")
}

/// A machine built from the real firmware and reset.
fn booted(img: &firmware::Image) -> Machine {
    let mut m = Machine::new(MachineConfig::default(), img);
    m.reset();
    m
}

/// Run to a stop, in fixed-size chunks (the last chunk carries the rest).
fn run_chunked(m: &mut Machine, mut total: u64, chunk: u64) -> truman_emulator::machine::RunResult {
    let mut acc_steps: u64 = 0;
    let mut last: Option<truman_emulator::machine::RunResult> = None;
    while total > 0 {
        let n = total.min(chunk);
        let r = m.run(n);
        acc_steps += r.steps;
        total -= r.steps;
        let stopped = !matches!(r.stop, truman_emulator::machine::StopReason::StepLimit);
        last = Some(r);
        if stopped {
            break;
        }
    }
    let mut r = last.expect("at least one chunk");
    r.steps = acc_steps;
    r
}

#[test]
fn boot_chain_is_chunk_invariant() {
    let img = firmware_image();
    // Atomic: one run over the whole acceptance chain (to the trap).
    let mut a = booted(&img);
    let ra = a.run(1_400_000);
    // Chunked: the same budget in odd-sized slices.
    let mut b = booted(&img);
    let rb = run_chunked(&mut b, 1_400_000, 65_537);
    assert_eq!(ra.steps, rb.steps, "the trap step count is chunk-invariant");
    assert_eq!(
        ra.steps, 1_301_783,
        "the acceptance pin holds in both modes"
    );
    assert_eq!(ra.stop, rb.stop, "same stop reason");
    assert_eq!(ra.pc, rb.pc, "same final pc");
    assert_eq!(
        a.uart_console(),
        b.uart_console(),
        "same UART console bytes"
    );
    assert_eq!(
        a.snapshot(),
        b.snapshot(),
        "same register file, hi/lo, pc/next_pc"
    );
    assert_eq!(
        ra.touchmap, rb.touchmap,
        "same instrumented window touchmap"
    );
}

#[test]
fn traced_execution_matches_plain_execution() {
    let img = firmware_image();
    let mut a = booted(&img);
    let ra = a.run(2_000);
    let mut b = booted(&img);
    let mut tracer = Tracer::new(4_096);
    let rb = b.run_traced(2_000, &mut tracer);
    assert_eq!(ra.steps, rb.steps);
    assert_eq!(ra.stop, rb.stop);
    assert_eq!(ra.pc, rb.pc);
    assert_eq!(a.uart_console(), b.uart_console());
    assert_eq!(
        a.snapshot(),
        b.snapshot(),
        "describe-mode must not alter the chain"
    );
    assert_eq!(ra.touchmap, rb.touchmap);
    // And the tracer saw exactly the executed steps.
    assert_eq!(tracer.total(), 2_000);
}

#[test]
fn machine_rebuilds_are_identical_in_state() {
    let img = firmware_image();
    let mut a = booted(&img);
    let ra = a.run(1_000);
    let mut b = booted(&img);
    let rb = b.run(1_000);
    assert_eq!(ra.steps, rb.steps, "same step count");
    assert_eq!(ra.stop, rb.stop, "same stop reason");
    assert_eq!(ra.pc, rb.pc, "same final pc");
    assert_eq!(ra.touchmap, rb.touchmap, "same touchmap");
    assert_eq!(a.snapshot(), b.snapshot());
    assert_eq!(a.uart_console(), b.uart_console());
}
