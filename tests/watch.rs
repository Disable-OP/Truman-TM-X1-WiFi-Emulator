//! Watchpoint subsystem tests (Milestone T, docs/firmware-analysis.md §22).
//!
//! The Q8 instrument needs trustworthy attribution: every hit must carry
//! the issuing (step, PC), the exact address, width and value, and kind
//! filtering must be exact. These tests exercise the AddressSpace-level
//! mechanics directly (no firmware required); the runtime evidence pins
//! live in tests/q8.rs.

use truman_emulator::memory::{AccessSize, AddressSpace, WatchKind, WatchKinds};

/// RAM at phys 0x0 (kseg0 0x80000000), ROM at phys 0x2000 (kseg1 0xA0002000).
fn bus() -> AddressSpace {
    let mut b = AddressSpace::new();
    b.add_ram("ram", 0x0000_0000, 0x1000);
    b.add_rom(
        "rom",
        0x0000_2000,
        (0u8..=0xFF).cycle().take(0x100).collect(),
    );
    b
}

#[test]
fn read_hit_records_value_region_and_size() {
    let mut b = bus();
    b.add_watch(0x0000_0000, 0x100, WatchKinds::READ);
    let v = b
        .load(0x8000_0010, AccessSize::Word)
        .expect("ram word read");
    assert_eq!(v, 0);
    let hits = b.take_watch_hits();
    assert_eq!(hits.len(), 1);
    let h = &hits[0];
    assert_eq!(h.kind, WatchKind::Read);
    assert_eq!(h.vaddr, 0x8000_0010);
    assert_eq!(h.phys, 0x10);
    assert_eq!(h.value, 0);
    assert_eq!(h.size, AccessSize::Word);
    assert_eq!(h.region, "ram");
}

#[test]
fn write_hit_records_stored_value() {
    let mut b = bus();
    b.add_watch(0x0000_0000, 0x100, WatchKinds::WRITE);
    b.store(0x8000_0020, AccessSize::Word, 0xDEAD_BEEF)
        .expect("ram store");
    b.store(0x8000_0024, AccessSize::Byte, 0x7F)
        .expect("ram byte store");
    let hits = b.take_watch_hits();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].value, 0xDEAD_BEEF);
    assert_eq!(hits[0].size, AccessSize::Word);
    assert_eq!(hits[1].value, 0x7F);
    assert_eq!(hits[1].size, AccessSize::Byte);
}

#[test]
fn kind_filtering_is_exact() {
    let mut b = bus();
    b.add_watch(0x0000_0000, 0x100, WatchKinds::READ);
    b.store(0x8000_0030, AccessSize::Word, 1).expect("store");
    assert_eq!(b.watch_hit_count(), 0, "READ watch must not fire on store");
    b.load(0x8000_0030, AccessSize::Word).expect("load");
    assert_eq!(b.watch_hit_count(), 1);

    let mut b2 = bus();
    b2.add_watch(0x0000_0000, 0x100, WatchKinds::WRITE);
    b2.load(0x8000_0030, AccessSize::Word).expect("load");
    assert_eq!(b2.watch_hit_count(), 0, "WRITE watch must not fire on load");
}

#[test]
fn range_boundaries_are_half_open() {
    let mut b = bus();
    // Watch phys 0x40..0x48 only.
    b.add_watch(0x40, 8, WatchKinds::READ_WRITE);
    // Below: no hit.
    b.load(0x8000_003C, AccessSize::Word).expect("below");
    // At base: hit.
    b.load(0x8000_0040, AccessSize::Word).expect("base");
    // Last covered byte: hit.
    b.load(0x8000_0047, AccessSize::Byte).expect("last");
    // Above: no hit.
    b.load(0x8000_0048, AccessSize::Word).expect("above");
    let hits = b.take_watch_hits();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].phys, 0x40);
    assert_eq!(hits[1].phys, 0x47);
}

#[test]
fn fetch_kind_watch_fires_on_fetch_only() {
    let mut b = bus();
    b.add_watch(0x0000_2000, 0x100, WatchKinds::FETCH);
    let w = b.fetch(0xA000_2000).expect("rom fetch");
    assert_ne!(w, 0);
    assert_eq!(b.watch_hit_count(), 1, "fetch must hit a FETCH watch");
    // A data read of the same address must NOT hit the FETCH watch.
    b.load(0xA000_2000, AccessSize::Word).expect("rom read");
    assert_eq!(b.watch_hit_count(), 1);
    let hits = b.take_watch_hits();
    assert_eq!(hits[0].kind, WatchKind::Fetch);
    assert_eq!(hits[0].value, w);
}

#[test]
fn cap_overflow_flag_and_drain() {
    let mut b = bus();
    b.set_watch_cap(2);
    b.add_watch(0x0000_0000, 0x100, WatchKinds::WRITE);
    for i in 0..4u32 {
        b.store(0x8000_0040 + i * 4, AccessSize::Word, i)
            .expect("store");
    }
    assert_eq!(b.watch_hit_count(), 2, "cap must bound recorded hits");
    assert!(b.watch_overflow());
    let hits = b.take_watch_hits();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].value, 0);
    assert_eq!(hits[1].value, 1);
    assert_eq!(b.watch_hit_count(), 0, "drain must empty the buffer");
    assert!(b.watch_overflow(), "overflow flag must persist until read");
}

#[test]
fn clear_watches_stops_recording() {
    let mut b = bus();
    b.add_watch(0x0000_0000, 0x100, WatchKinds::READ_WRITE);
    b.load(0x8000_0050, AccessSize::Word).expect("load");
    assert_eq!(b.watch_hit_count(), 1);
    b.clear_watches();
    b.load(0x8000_0050, AccessSize::Word).expect("load 2");
    assert_eq!(b.watch_hit_count(), 1, "no new hits after clear_watches");
}

#[test]
fn unwatched_bus_has_zero_overhead_path() {
    // Without watches, loads/stores still work and record nothing
    // (regression: the recording fast path must be inert by default).
    let mut b = bus();
    b.store(0x8000_0060, AccessSize::Word, 0x1234_5678)
        .expect("store");
    let v = b.load(0x8000_0060, AccessSize::Word).expect("load");
    assert_eq!(v, 0x1234_5678);
    assert_eq!(b.watch_hit_count(), 0);
    assert!(!b.has_watches());
}

#[test]
fn rom_store_faults_and_records_nothing() {
    let mut b = bus();
    b.add_watch(0x0000_2000, 0x100, WatchKinds::READ_WRITE);
    let err = b.store(0xA000_2000, AccessSize::Word, 0).unwrap_err();
    assert!(matches!(
        err,
        truman_emulator::memory::AccessFault {
            kind: truman_emulator::memory::FaultKind::Write,
            ..
        }
    ));
    assert_eq!(b.watch_hit_count(), 0, "failed access must not record");
}
