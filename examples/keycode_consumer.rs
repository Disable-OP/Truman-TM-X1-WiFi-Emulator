//! Milestone U: keycode-consumer hunt — dynamic reader/writer attribution
//! on the front-panel key port (docs/firmware-analysis.md §23.3).
//!
//! The static scan (`tools/fw_keycode_consumer.py`) finds all payload code
//! that touches 0xBFFE384C. This instrument adds the runtime half: boot the
//! REAL firmware with a READ|WRITE watchpoint over the key port (physical
//! 0x1FFE384C), inject virtual button presses through the machine API, and
//! attribute every port access to the issuing (step, PC). The dispatcher
//! (kseg0 0x82002548..0x82002630) must dominate; any OTHER reader PC would
//! be the keycode consumer.
//!
//! Usage: cargo run --release --example keycode_consumer -- firmware/<bin>

use std::collections::BTreeMap;

use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig};
use truman_emulator::memory::{WatchKind, WatchKinds};

const KEY_PORT_PHYS: u32 = 0x1FFE_384C;
/// Dispatcher setup + loop in payload-physical terms (kseg1 0xA2002500 =
/// phys 0x02002500; kseg0 alias 0x82002500 — same code, §12).
const DISPATCHER: (u32, u32) = (0x0200_2500, 0x0200_2640);

fn load_machine() -> Option<Machine> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("firmware")
        .read_dir()
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().map(|x| x == "bin").unwrap_or(false))?;
    let image = firmware::load(&path).expect("load real firmware");
    Some(Machine::new(MachineConfig::default(), &image))
}

/// Boot with the key port watched, holding `bits` pressed from reset.
/// Returns (steps, pc, hit table: (pc, kind) -> (count, distinct values)).
type PortTable = BTreeMap<(u32, u8), (usize, std::collections::BTreeSet<u32>)>;
type RunReport = (u64, u32, PortTable);

fn run_with_press(bits: u32) -> RunReport {
    let Some(mut m) = load_machine() else {
        eprintln!("skipping: real firmware not present in firmware/");
        std::process::exit(0);
    };
    m.add_watch(KEY_PORT_PHYS, 4, WatchKinds::READ_WRITE);
    m.press_key(bits);
    m.reset();
    let r = m.run(30_000_000);
    eprintln!("steps {} stop {:?} pc=0x{:08x}", r.steps, r.stop, r.pc);
    let hits = m.take_watch_hits();
    let overflow = m.bus.watch_overflow();
    eprintln!(
        "watch hits: {} (overflow: {overflow})",
        hits.len() + m.bus.watch_hit_count()
    );
    let mut table: BTreeMap<(u32, u8), (usize, std::collections::BTreeSet<u32>)> =
        Default::default();
    for h in &hits {
        let kind_u8 = match h.kind {
            WatchKind::Read => 0u8,
            WatchKind::Write => 1u8,
            WatchKind::Fetch => 2u8,
        };
        let e = table.entry((h.pc, kind_u8)).or_default();
        e.0 += 1;
        e.1.insert(h.value);
    }
    (r.steps, r.pc, table)
}

fn report(label: &str, table: &PortTable) {
    println!("== {label}: port access attribution ==");
    for ((pc, kind), (count, values)) in table {
        let who = if (DISPATCHER.0..DISPATCHER.1).contains(&(pc & 0x1FFF_FFFF)) {
            "dispatcher"
        } else {
            "CONSUMER?"
        };
        let kind_s = match kind {
            0 => "read ",
            1 => "write",
            _ => "fetch",
        };
        let vals: Vec<String> = values.iter().map(|v| format!("{v:#06x}")).collect();
        println!(
            "  pc={pc:#08x} [{who:<10}] {kind_s} x{count:<7} values: {}",
            if vals.len() > 8 {
                format!("{} ... ({} distinct)", vals[..8].join(","), vals.len())
            } else {
                vals.join(",")
            }
        );
    }
}

fn main() {
    // Phase 1: hold key-line bit 2 (the documented test key, keycode 0x400).
    let (_, _, t2) = run_with_press(0b0100);
    report("key line bit 2 held", &t2);

    // Phase 2: all four scan lines at once — the dispatcher polls them in
    // order 0/2/4/5 and emits each keycode in turn.
    let (_, _, all) = run_with_press(0b0101_0101);
    report("all scan lines held", &all);
}
