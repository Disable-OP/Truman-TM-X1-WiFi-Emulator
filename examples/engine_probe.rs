//! Analysis harness: runs the real firmware boot, prints the decompression
//! engine's complete register-access log (the driver's engine program,
//! docs/firmware-analysis.md §13.2) and dumps the window/scratch/output
//! DRAM regions after the decode.
//!
//! Usage: cargo run --release --example engine_probe <flashfile.bin>
use std::path::PathBuf;

use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig};
use truman_emulator::memory::AccessSize;

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            eprintln!("usage: engine_probe <flashfile.bin>");
            std::process::exit(1);
        });
    let img = firmware::load(&path).expect("load");
    let cfg = MachineConfig::default();
    let mut m = Machine::new(cfg, &img);
    m.reset();
    let res = m.run(2_000_000);
    println!("stop={:?} steps={} pc={:08x}", res.stop, res.steps, res.pc);

    if let Some(log) = m.access_log("decomp-engine") {
        let entries = log.borrow();
        println!("-- decomp-engine ({} entries) --", entries.len());
        for (off, size, val, wr) in entries.iter() {
            let kind = if *wr { "WR" } else { "RD" };
            let sz = match size {
                AccessSize::Byte => "b",
                AccessSize::Half => "h",
                AccessSize::Word => "w",
            };
            println!("  {} {} 0x{:04x} = 0x{:08x}", kind, sz, off, val);
        }
    } else {
        println!("no decomp-engine log");
    }

    let regions: [(u32, u32, &str); 5] = [
        (0x0204_0000, 0x8000, "window"),
        (0x0204_8000, 0x4000, "scratch-head"),
        (0x0209_0600, 0x200, "container-end+"),
        (0x0160_0000, 0x20, "out-head"),
        (0x0200_3AF0, 0x40, "driver-gp-state"),
    ];
    for (phys, len, tag) in regions {
        let mut nz = 0usize;
        let mut head = Vec::new();
        for i in 0..len {
            let b = m.bus.peek_phys(phys + i, AccessSize::Byte).unwrap_or(0) as u8;
            if b != 0 {
                nz += 1;
            }
            if i < 32 {
                head.push(b);
            }
        }
        println!(
            "{}: 0x{:08x}+0x{:x} nonzero={} head={}",
            tag,
            phys,
            len,
            nz,
            head.iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>()
        );
    }
}
