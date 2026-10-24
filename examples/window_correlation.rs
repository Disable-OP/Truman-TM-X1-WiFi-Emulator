//! Window-access correlation with the boot's UART milestone sequence
//! (roadmap Q2: name the 0x98010000 / 0xB0000000 / 0xF0000000 windows).
//!
//! Runs the real firmware boot in fixed step segments. After each segment
//! it snapshots (a) the UART console length and (b) every instrumented
//! window's access log (cleared per segment, so deltas are exact). Console
//! lines then act as phase boundaries: for each phase we report which
//! windows were touched, how often, and where (offset ranges), so each
//! unknown window can be attributed to a boot phase by *evidence*.
//!
//! Usage: cargo run --release --example window_correlation [-- <firmware>]
//! (default: the committed 4 MiB internal-flash image).

use std::path::PathBuf;

use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig};

const DEFAULT_FW: &str =
    "firmware/1506HV_512M_4M_SVT2_TRUMAN_TM_X1_WIFI_Internal_flashfile_20220902.bin";
const SEGMENT_STEPS: u64 = 10_000;
const MAX_STEPS: u64 = 1_500_000;

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_FW));
    let img = firmware::load(&path).expect("firmware load");
    let mut machine = Machine::new(MachineConfig::default(), &img);
    machine.reset();

    let windows: Vec<String> = machine.access_log_names().to_vec();
    let mut console_len = 0usize;
    let mut step = 0u64;
    let mut phase_first_step = 0u64;
    let mut phase_line = String::from("(before first console output)");
    // cumulative first-touch step per window
    let mut first_touch: Vec<(String, Option<u64>)> =
        windows.iter().map(|w| (w.clone(), None)).collect();
    // cumulative distinct offsets per window (the real touchmap)
    let mut cum_offsets: Vec<(String, std::collections::BTreeSet<u32>)> = windows
        .iter()
        .map(|w| (w.clone(), std::collections::BTreeSet::new()))
        .collect();

    println!("# window/UART correlation over the real boot (segments of {SEGMENT_STEPS} steps)");
    while step < MAX_STEPS {
        // clear all logs so the next segment records exactly its own accesses
        for w in &windows {
            if let Some(log) = machine.access_log(w) {
                log.borrow_mut().clear();
            }
        }
        let stop = machine.run(SEGMENT_STEPS);
        step += SEGMENT_STEPS;

        let console = machine.uart_console();
        let new_console = &console.as_bytes()[console_len.min(console.len())..];
        console_len = console.len();

        // per-window deltas for this segment
        let mut phase_report: Vec<(String, usize, Vec<u32>, usize, usize)> = Vec::new();
        for (i, w) in windows.iter().enumerate() {
            let Some(log) = machine.access_log(w) else {
                continue;
            };
            let entries = log.borrow();
            if entries.is_empty() {
                continue;
            }
            let mut offs = std::collections::BTreeSet::new();
            let (mut r, mut wr) = (0usize, 0usize);
            for (off, _size, _val, was_write) in entries.iter() {
                offs.insert(*off);
                if *was_write {
                    wr += 1;
                } else {
                    r += 1;
                }
            }
            if first_touch[i].1.is_none() {
                first_touch[i].1 = Some(step);
            }
            cum_offsets[i].1.extend(offs.iter().copied());
            let distinct: Vec<u32> = offs.into_iter().take(6).collect();
            phase_report.push((w.clone(), entries.len(), distinct, r, wr));
        }

        // A phase ends when the console grew by a full line (or on stop).
        let got_line = new_console.contains(&b'\n');
        let stopped = !matches!(stop.stop, truman_emulator::machine::StopReason::StepLimit);
        if got_line || stopped || step >= MAX_STEPS {
            let text: String = String::from_utf8_lossy(new_console)
                .lines()
                .last()
                .unwrap_or("")
                .to_string();
            if !text.trim().is_empty() {
                phase_line = text.trim().to_string();
            }
            println!(
                "phase steps {:>8}..{:<8} after \"{}\"",
                phase_first_step, step, phase_line
            );
            for (w, n, offs, r, wr) in &phase_report {
                let offs_s: Vec<String> = offs.iter().map(|o| format!("0x{o:04x}")).collect();
                println!(
                    "    {w:<22} {n:>6} acc ({r}/{wr} r/w) at {}",
                    offs_s.join(" ")
                );
            }
            phase_first_step = step;
            if stopped {
                println!(
                    "# boot stopped at step {}: {}",
                    stop.steps,
                    match &stop.stop {
                        truman_emulator::machine::StopReason::StepLimit => {
                            "step limit".to_string()
                        }
                        truman_emulator::machine::StopReason::Cpu(e) => {
                            format!("{e}")
                        }
                        truman_emulator::machine::StopReason::EntryReached { pc } => {
                            format!("entry watch at 0x{pc:08x}")
                        }
                    }
                );
                break;
            }
        }
    }

    println!("\n# first-touch summary (window: step first accessed)");
    for (w, t) in &first_touch {
        let total = cum_offsets
            .iter()
            .find(|(n, _)| n == w)
            .map(|(_, s)| s.len())
            .unwrap_or(0);
        println!(
            "  {w:<22} first={} distinct_offsets_total={}",
            t.map(|v| v.to_string()).unwrap_or_else(|| "never".into()),
            total
        );
    }
    // offset-range detail for the three Q2 windows
    println!("\n# Q2 windows: cumulative offset structure");
    for w in ["win-0x18000000", "win-0x10000000", "win-0x30000000"] {
        let Some((_, set)) = cum_offsets.iter().find(|(n, _)| n == w) else {
            continue;
        };
        if set.is_empty() {
            println!("  {w}: (no accesses in the whole boot)");
            continue;
        }
        let (min, max) = (*set.first().unwrap(), *set.last().unwrap());
        let above = set.iter().filter(|o| **o >= 0x10000).count();
        println!(
            "  {w}: {} distinct offsets, range 0x{min:04x}..0x{max:04x}, >=0x10000: {above}",
            set.len()
        );
    }
}
