//! Dump the pre-decode content around the PNG pixel buffer (phys
//! 0x01600000) — what the engine's copy-back window would see for
//! matches with distance beyond the current output position, and what
//! the driver zero-fill actually left in the band/scaling buffers.
//!
//! Run to a step count, then peek. Usage:
//!   cargo run --example pixel_precontent -- [steps]

use truman_emulator::boot;
use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig};

fn main() {
    let steps: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_280_000);

    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("firmware");
    let mut found = None;
    if let Ok(rd) = std::fs::read_dir(&path) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().map(|x| x == "bin").unwrap_or(false)
                && p.file_name()
                    .map(|n| n.to_string_lossy().contains("1506HV"))
                    .unwrap_or(false)
            {
                found = Some(p);
            }
        }
    }
    let Some(path) = found else {
        eprintln!("real firmware not found under firmware/");
        return;
    };
    let image = firmware::load(&path).expect("load real firmware");
    let mut machine = Machine::new(MachineConfig::default(), &image);
    boot::reset_with_diagnostics(&mut machine, false);
    let res = machine.run(steps as u64);
    eprintln!(
        "steps={}, stop={:?}, pc=0x{:08x}",
        res.steps, res.stop, res.pc
    );

    let bus = &mut machine.bus;
    println!("pre-decode content around the pixel buffer (phys 0x01600000):");
    for base in [0x015F_FC00u32, 0x0160_0000, 0x0160_0400] {
        print!("  0x{base:08X}:");
        for w in 0..8 {
            let v = bus
                .peek_phys(base + w * 4, truman_emulator::memory::AccessSize::Word)
                .unwrap_or(0);
            print!(" {v:08x}");
        }
        println!();
    }
    println!("band buffer (phys 0x02040000) after driver zero-fill:");
    let nz = (0..0x8000u32)
        .step_by(4)
        .filter(|&o| {
            bus.peek_phys(0x0204_0000 + o, truman_emulator::memory::AccessSize::Word)
                .unwrap_or(0)
                != 0
        })
        .count();
    println!("  nonzero words: {nz}");
}
