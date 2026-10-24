//! Milestone U evidence: extract the complete SoC timer register program
//! (+0x600..0x620) from a full real-firmware boot, and pin the delay(N)
//! protocol's tick arithmetic (docs §5: write 0 to +0x600, poll count).
//!
//! Usage: cargo run --release --example timer_probe -- firmware/<flash.bin>

use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig};

fn main() {
    let fw = std::env::args()
        .nth(1)
        .expect("usage: timer_probe <flash.bin>");
    let image = firmware::load(fw.as_ref()).expect("load real firmware");
    let mut m = Machine::new(MachineConfig::default(), &image);
    m.reset();
    let r = m.run(3_000_000);
    eprintln!("steps {} stop {:?}", r.steps, r.stop);
    let log = m.access_log("timer").expect("timer log");
    let mut writes: std::collections::BTreeMap<u32, std::collections::BTreeSet<u32>> =
        Default::default();
    let mut reads: std::collections::BTreeMap<u32, usize> = Default::default();
    let mut count_read_max: u32 = 0;
    for (off, _size, val, wr) in log.borrow().iter() {
        if *wr {
            writes.entry(*off).or_default().insert(*val);
        } else {
            *reads.entry(*off).or_default() += 1;
            if *off == 0 {
                count_read_max = count_read_max.max(*val);
            }
        }
    }
    println!("== timer register program (offset -> write values) ==");
    for (off, vals) in &writes {
        let vs: Vec<String> = vals.iter().map(|v| format!("{v:#x}")).collect();
        println!(
            "  +{off:#04x}: {} distinct values: {}",
            vals.len(),
            vs.join(", ")
        );
    }
    println!("== timer reads ==");
    for (off, n) in &reads {
        println!("  +{off:#04x}: {n} reads");
    }
    println!("max counter value observed via reads: {count_read_max:#x}");
}
