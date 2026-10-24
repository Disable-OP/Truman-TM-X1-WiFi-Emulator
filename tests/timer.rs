//! Milestone U: the SoC timer block (+0x600..0x620) — model semantics and
//! the real firmware's observed register program (docs/firmware-analysis.md
//! §5, §23.4).
//!
//! The block is used by the firmware purely as a polled free-running
//! counter: `delay(N)` at mirror 0xB8008880 writes 0 to +0x600 (software
//! reset) and polls +0x600 until the count reaches 90×N. No IRQ line of
//! the timer block is observed anywhere in the boot horizon (the COP0
//! Count/Compare timer is separate and covered by tests/interrupt.rs).
//!
//! Real-boot register program (extracted with examples/timer_probe.rs over
//! the full boot; deterministic — pinned here):
//!   writes: +0x00 = 0 (one reset), +0x0C = 0x8095, +0x1C = 0
//!   reads:  +0x00 ×226 (the delay poll), max observed count 0x385 (901)

use std::cell::RefCell;
use std::rc::Rc;

use truman_emulator::devices::{Device, TimerCounter};
use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig};
use truman_emulator::memory::AccessSize;

type TimerLog = Rc<RefCell<Vec<(u32, AccessSize, u32, bool)>>>;

fn new_timer() -> (TimerCounter, TimerLog) {
    let log = Rc::new(RefCell::new(Vec::new()));
    let t = TimerCounter::new("timer-test", Rc::clone(&log));
    (t, log)
}

#[test]
fn timer_counts_per_tick_and_software_resets() {
    let (mut t, _log) = new_timer();
    assert_eq!(t.read(0x00, AccessSize::Word), 0);
    t.tick();
    t.tick();
    assert_eq!(t.read(0x00, AccessSize::Word), 2, "1 count per tick");
    // The delay() protocol: write 0 to +0x600 resets the counter.
    t.write(0x00, AccessSize::Word, 0);
    assert_eq!(t.read(0x00, AccessSize::Word), 0, "software reset");
    t.tick();
    assert_eq!(t.read(0x00, AccessSize::Word), 1);
}

#[test]
fn timer_config_registers_store_and_no_irq_semantics_are_claimed() {
    let (mut t, log) = new_timer();
    // The observed init program: +0x0C = 0x8095, +0x1C = 0. The model
    // stores config words (semantics unknown — logged, never faked).
    t.write(0x0C, AccessSize::Word, 0x8095);
    t.write(0x1C, AccessSize::Word, 0);
    assert_eq!(t.read(0x0C, AccessSize::Word), 0x8095);
    assert_eq!(t.read(0x1C, AccessSize::Word), 0);
    // Untouched registers read 0 (device fill semantics, visible in the
    // access log — never silent).
    assert_eq!(t.read(0x08, AccessSize::Word), 0);
    let entries = log.borrow();
    assert!(
        entries.iter().any(|e| e.0 == 0x08 && !e.3),
        "reads of unmodeled registers must be logged"
    );
}

/// Pin the real firmware's complete timer register program over the full
/// boot (extracted by examples/timer_probe.rs; §23.4). Any change to the
/// boot's use of the timer block must be a deliberate, documented change.
#[test]
fn real_firmware_timer_program_is_pinned() {
    let Some(path) = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("firmware")
        .read_dir()
        .ok()
        .and_then(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.path())
                .find(|p| p.extension().map(|x| x == "bin").unwrap_or(false))
        })
    else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let image = firmware::load(&path).expect("load real firmware");
    let mut m = Machine::new(MachineConfig::default(), &image);
    m.reset();
    let r = m.run(3_000_000);
    assert!(
        r.steps >= 1_300_000,
        "boot must reach the eCos stage horizon, stopped at {} ({:?})",
        r.steps,
        r.stop
    );

    let log = m.access_log("timer").expect("timer access log");
    let mut writes: std::collections::BTreeMap<u32, std::collections::BTreeSet<u32>> =
        Default::default();
    let mut counter_reads = 0usize;
    let mut max_count = 0u32;
    for (off, _size, val, wr) in log.borrow().iter() {
        if *wr {
            writes.entry(*off).or_default().insert(*val);
        } else if *off == 0x00 {
            counter_reads += 1;
            max_count = max_count.max(*val);
        }
    }

    // The complete write set: one counter reset (delay entry), the two
    // config words written once at init. Nothing else ever writes.
    let expected: Vec<(u32, u32)> = vec![(0x00, 0), (0x0C, 0x8095), (0x1C, 0)];
    for (off, val) in &expected {
        assert_eq!(
            writes.get(off).map(|s| s.contains(val)),
            Some(true),
            "timer +{off:#04x} must be written with {val:#x} exactly once; program: {writes:?}"
        );
        assert_eq!(
            writes.get(off).map(|s| s.len()),
            Some(1),
            "exactly one distinct write value at +{off:#04x}: {writes:?}"
        );
    }
    assert_eq!(
        writes.len(),
        3,
        "no other timer register is written: {writes:?}"
    );

    // The delay(10)-style poll: 226 counter reads, max 901 (90×10 + 1
    // overshoot of the polling loop).
    assert_eq!(
        counter_reads, 226,
        "the boot's single delay poll reads the counter exactly 226 times"
    );
    assert_eq!(
        max_count, 0x385,
        "max polled count = 901 = 90×10 + 1 (§5/§23.4)"
    );
}
