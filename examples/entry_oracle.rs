//! Entry harness — the register-aware dynamic oracle (Milestone R,
//! docs/firmware-analysis.md §20).
//!
//! Milestone Q ended with a static paradox: the boot provably executes the
//! engine's LZ output from byte 0 (`jr v0` -> 0xA1600000 with the driver's
//! return value live in v1), yet every tested bit-accounting produced a
//! head that reserved-faults within +0x74 on literal bytes (§19). Static
//! word classification cannot follow control flow; this harness can.
//!
//! The harness boots the REAL firmware with a watch at 0xA1600000 (the
//! loader's `jr v0` target), captures the exact post-jump register state
//! and the full DRAM, then re-enters each candidate decode as MIPS code:
//!
//!   1. restore DRAM + registers to the captured entry state,
//!   2. write the candidate bytes into the PNG pixel buffer (phys
//!      0x01600000, zero-padded to the programmed image size — the
//!      band-buffer padding semantics of §17.2),
//!   3. execute from 0xA1600000 with a bounded step budget and record
//!      instructions survived + the first fault.
//!
//! A correct decode is the eCos entry: real firmware that runs to the
//! budget (10k+ instructions). A wrong decode faults on a reserved word,
//! a bad access or an unsupported instruction — the faulting PC localizes
//! the first wrong bytes *dynamically*, under the loader's true register
//! state (v1 = 0x13800 live, §19.1), which is exactly the oracle the
//! §19.4 head-branch DFS lacked.
//!
//! Candidates come from files (raw bytes, e.g. produced by the Python
//! analysis tools). The as-booted engine-model output (RFC 1951 value
//! semantics) is always evaluated first as the baseline.
//!
//! Usage:
//!   cargo run --release --example entry_oracle <flashfile.bin> \
//!       [--budget N] [--trace K] [candidate0.bin candidate1.bin ...]
use std::path::PathBuf;

use truman_emulator::cpu::CpuSnapshot;
use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig, StopReason};
use truman_emulator::memory::AccessSize;

/// The eCos entry: the loader's `jr v0` target (§13.3, §19.1).
const ENTRY: u32 = 0xA160_0000;
/// PNG_PXL_EXT_ADDR (§17.1): the pixel output the engine writes and the
/// boot jumps into.
const PIXEL_BUF: u32 = 0x0160_0000;
const DRAM_BASE: u32 = 0x0000_0000;
const DRAM_SIZE: usize = 0x0400_0000; // 64 MiB, MachineConfig::default
/// Default per-candidate instruction budget. A real eCos entry survives
/// far beyond this; any head fault happens within a few dozen steps.
const DEFAULT_BUDGET: u64 = 65_536;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: entry_oracle <flashfile.bin> [--budget N] [--trace K] [candidates...]");
        std::process::exit(1);
    }
    let mut budget = DEFAULT_BUDGET;
    let mut trace_k: usize = 0;
    let mut cands: Vec<(String, Vec<u8>)> = Vec::new();
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--budget" => {
                i += 1;
                budget = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(DEFAULT_BUDGET);
            }
            "--trace" => {
                i += 1;
                trace_k = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            p => {
                let data = std::fs::read(p).unwrap_or_else(|e| panic!("read {p}: {e}"));
                cands.push((p.to_string(), data));
            }
        }
        i += 1;
    }

    let img = firmware::load(&PathBuf::from(&args[1])).expect("load firmware");
    let mut m = Machine::new(MachineConfig::default(), &img);
    truman_emulator::boot::reset_with_diagnostics(&mut m, false);

    // 1. Boot the real firmware to the eCos entry.
    let res = m.run_until_pc(4_000_000, ENTRY);
    println!("boot: {} steps, stop: {}", res.steps, res.stop);
    let StopReason::EntryReached { pc } = res.stop else {
        eprintln!("boot did not reach the entry watch; aborting");
        std::process::exit(2);
    };
    assert_eq!(pc, ENTRY);

    // 2. Capture the entry state: registers + full DRAM.
    let entry_snap: CpuSnapshot = m.snapshot();
    let mut dram0 = vec![0u8; DRAM_SIZE];
    let n = m.bus.copy_out(DRAM_BASE, &mut dram0);
    println!(
        "entry: pc=0x{:08x} v0=0x{:08x} v1=0x{:08x} sp=0x{:08x} ra=0x{:08x} (DRAM snapshotted: {n} bytes)",
        entry_snap.pc, entry_snap.regs[2], entry_snap.regs[3], entry_snap.regs[29], entry_snap.regs[31]
    );
    let mut nonzero = Vec::new();
    for (r, &v) in entry_snap.regs.iter().enumerate() {
        if v != 0 && r != 0 {
            nonzero.push(format!("{}=0x{:08x}", reg_name(r), v));
        }
    }
    println!("entry: registers {}", nonzero.join(" "));

    // Image geometry from the engine's own registers.
    let w = m
        .bus
        .peek_phys(0x1FFE_3828, AccessSize::Word)
        .unwrap_or(0x400);
    let h = m.bus.peek_phys(0x1FFE_382C, AccessSize::Word).unwrap_or(78);
    let img_bytes = (w as usize) * (h as usize);
    println!("image: {w}x{h} = {img_bytes} bytes at phys 0x{PIXEL_BUF:08x}");

    // 3. Baseline candidate: the engine-model output as booted.
    let mut baseline = vec![0u8; img_bytes];
    m.bus.copy_out(PIXEL_BUF, &mut baseline);
    let mut all = vec![("engine-model (RFC 1951, as booted)".to_string(), baseline)];
    all.extend(cands);

    // 4. Evaluate every candidate from the identical entry state.
    println!(
        "\n{:<40} {:>8} {:>9}  outcome",
        "candidate", "bytes", "survived"
    );
    for (name, data) in &all {
        let mut padded = data.clone();
        padded.truncate(img_bytes);
        padded.resize(img_bytes, 0);

        m.bus.copy_in(DRAM_BASE, &dram0);
        m.restore_cpu_state(&entry_snap);
        let wrote = m.bus.copy_in(PIXEL_BUF, &padded);

        let out = m.run(budget);
        let outcome = match &out.stop {
            StopReason::StepLimit => "STEP LIMIT (ran clean to budget)".to_string(),
            StopReason::EntryReached { .. } => "re-entry watch hit (unexpected)".to_string(),
            StopReason::Cpu(e) => format!("{e}"),
        };
        println!(
            "{:<40} {:>8} {:>9}  pc=0x{:08x}  {}",
            truncate_name(name, 40),
            wrote,
            out.steps,
            out.pc,
            outcome
        );

        // A clean run to the budget is necessary but NOT sufficient: the
        // zero-filled DRAM below the pixel buffer is a giant NOP sled, so
        // any candidate that once branches into it survives for free.
        // Discriminate survivors: instruction diversity, zero-word fetch
        // share, and UART output (real firmware talks).
        if matches!(out.stop, StopReason::StepLimit) {
            m.bus.copy_in(DRAM_BASE, &dram0);
            m.restore_cpu_state(&entry_snap);
            m.bus.copy_in(PIXEL_BUF, &padded);
            let uart_before = m.uart_bytes().len();
            let mut tracer = truman_emulator::machine::Tracer::new(budget.min(1 << 20) as usize);
            let out2 = m.run_traced(budget, &mut tracer);
            let trace = tracer.drain();
            let total = trace.len();
            let mut zero_words = 0usize;
            let mut pcs = std::collections::HashSet::new();
            for r in &trace {
                if r.word == 0 {
                    zero_words += 1;
                }
                pcs.insert(r.pc);
            }
            let uart_new = m.uart_bytes().len() - uart_before;
            let zero_pct = zero_words
                .checked_mul(100)
                .and_then(|n| n.checked_div(total))
                .unwrap_or(0);
            let sled = zero_pct > 90;
            println!(
                "    metrics: steps={} distinct-pcs={} zero-fetch={} ({}%) uart+{} bytes {}",
                out2.steps,
                pcs.len(),
                zero_words,
                zero_pct,
                uart_new,
                if sled {
                    "-> NOP-sled artifact, NOT eCos survival"
                } else {
                    "-> diverse execution, worth deep inspection"
                }
            );
            if uart_new > 0 && uart_new <= 400 {
                let u = m.uart_bytes();
                println!(
                    "    uart: {:?}",
                    String::from_utf8_lossy(&u[u.len() - uart_new..])
                );
            }
        }

        if trace_k > 0 {
            let mut tracer = truman_emulator::machine::Tracer::new(trace_k);
            m.bus.copy_in(DRAM_BASE, &dram0);
            m.restore_cpu_state(&entry_snap);
            m.bus.copy_in(PIXEL_BUF, &padded);
            let _ = m.run_traced(trace_k as u64, &mut tracer);
            for r in tracer.drain() {
                println!(
                    "    +0x{:04x}: {:08x}  {}",
                    r.pc.wrapping_sub(ENTRY),
                    r.word,
                    r.desc
                );
            }
        }
    }
}

fn truncate_name(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n - 1).collect::<String>() + "…"
    }
}

/// MIPS ABI register names for the diagnostics line.
fn reg_name(r: usize) -> &'static str {
    const NAMES: [&str; 32] = [
        "zero", "at", "v0", "v1", "a0", "a1", "a2", "a3", "t0", "t1", "t2", "t3", "t4", "t5", "t6",
        "t7", "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7", "t8", "t9", "k0", "k1", "gp", "sp",
        "fp", "ra",
    ];
    NAMES.get(r).copied().unwrap_or("?")
}
