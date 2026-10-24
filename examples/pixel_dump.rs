//! Analysis harness: runs the real firmware boot and dumps the engine's
//! pixel output (0x01600000, PNG_IMG_WIDTH x PNG_IMG_HEIGHT bytes) plus the
//! band window, for first-corruption analysis (docs/firmware-analysis.md
//! §18, Milestone Q tooling).
//!
//! Usage: cargo run --release --example pixel_dump <flashfile.bin> <out.bin>
use std::path::PathBuf;

use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig};
use truman_emulator::memory::AccessSize;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: pixel_dump <flashfile.bin> <out.bin>");
        std::process::exit(1);
    }
    let img = firmware::load(&PathBuf::from(&args[1])).expect("load");
    let mut m = Machine::new(MachineConfig::default(), &img);
    m.reset();
    let res = m.run(2_000_000);
    println!("stop={:?} steps={} pc={:08x}", res.stop, res.steps, res.pc);

    // PNG image geometry from the engine's own registers (phys 0x1FFE38xx).
    let w = m
        .bus
        .peek_phys(0x1FFE_3828, AccessSize::Word)
        .unwrap_or(0x400);
    let h = m.bus.peek_phys(0x1FFE_382C, AccessSize::Word).unwrap_or(78);
    let total = (w as usize) * (h as usize);
    let mut out = Vec::with_capacity(total);
    for i in 0..total {
        out.push(
            m.bus
                .peek_phys(0x0160_0000 + i as u32, AccessSize::Byte)
                .unwrap_or(0) as u8,
        );
    }
    std::fs::write(&args[2], &out).expect("write");
    println!("dumped {} bytes ({}x{}) to {}", out.len(), w, h, args[2]);
    let nz = out.iter().filter(|&&b| b != 0).count();
    println!("non-zero bytes: {nz} ({}%)", nz * 100 / out.len().max(1));
}
