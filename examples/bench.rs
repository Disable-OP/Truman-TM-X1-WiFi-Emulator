//! Performance benchmark: untraced execution of the real firmware.
//!
//! Reproduce (roadmap item 5 — the decode-cache/perf milestone):
//!
//! ```sh
//! cargo build --release --example bench
//! ./target/release/examples/bench firmware/1506HV_*.bin [steps]
//! ```
//!
//! Reports instructions/second over an untraced `Machine::run` of the
//! real boot chain (identical semantics to `run --max-steps N`; the
//! harness stops at the eCos entry exactly like the acceptance test).
//! No numbers are claimed in docs without this command.

use std::time::Instant;

use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig, StopReason};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: bench <firmware.bin> [max_steps=2000000]");
        std::process::exit(1);
    }
    let path = &args[1];
    let max_steps: u64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2_000_000);

    let image = firmware::load(std::path::Path::new(path)).expect("load flash image");
    let mut machine = Machine::new(MachineConfig::default(), &image);
    machine.reset();

    let t0 = Instant::now();
    let result = machine.run(max_steps);
    let dt = t0.elapsed();

    let steps = result.steps;
    let secs = dt.as_secs_f64();
    println!(
        "steps executed : {steps} (stop: {})",
        match result.stop {
            StopReason::StepLimit => "step-limit".to_string(),
            StopReason::EntryReached { .. } => "entry".to_string(),
            StopReason::Cpu(e) => format!("cpu: {e}"),
        }
    );
    println!("wall time      : {secs:.3} s");
    println!(
        "throughput     : {:.2} M instr/s",
        steps as f64 / secs / 1e6
    );
    println!("final pc       : 0x{:08x}", result.pc);
}
