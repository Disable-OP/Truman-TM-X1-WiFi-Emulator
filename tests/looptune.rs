//! Milestone Z regression pins (§28): the per-step loop optimizations
//! are semantics-preserving — these tests pin the three mechanisms
//! behind the 19.5 → 25.4 M instr/s boot-throughput gain.

use std::cell::Cell;
use std::rc::Rc;

use truman_emulator::devices::{Device, EngineShared, IntController, LoggingDevice, TimerCounter};
use truman_emulator::memory::AccessSize;

/// The interrupt controller's change-detection tick (§28): a sources
/// change between steps asserts the line on the next tick, exactly like
/// the old unconditional recompute; enable-register writes assert
/// immediately without any tick.
#[test]
fn intc_change_detection_tick_matches_unconditional_recompute() {
    let sources = Rc::new(Cell::new(0u32));
    let line = Rc::new(Cell::new(0u8));
    let log = LoggingDevice::new("t", 0).log_handle();
    let mut intc = IntController::new("intc", sources.clone(), line.clone(), log);

    // Enable all sources via the verified write path (asserts the line
    // immediately — no tick needed).
    Device::write(&mut intc, 0x00, AccessSize::Word, 0xFFFF_FFFF);
    assert_eq!(line.get(), 0, "no sources raised yet");

    // Raise a source through the shared latch: the change must be seen
    // on the next tick (this is Machine::raise_int_sources's contract).
    sources.set(0b100);
    assert_eq!(
        line.get(),
        0,
        "line changes on the step boundary, not mid-step"
    );
    Device::tick(&mut intc);
    assert_eq!(line.get(), 1, "raised source visible after one tick");

    // Repeated ticks without a change keep the line (idempotent).
    Device::tick(&mut intc);
    assert_eq!(line.get(), 1);

    // Clearing drops it again on the next tick.
    sources.set(0);
    Device::tick(&mut intc);
    assert_eq!(line.get(), 0);
}

/// The engine request hint (§28): set by a post, cleared by
/// take_request; take_request stays authoritative.
#[test]
fn engine_pending_hint_tracks_request_lifecycle() {
    let mut sh = EngineShared {
        regs: vec![0; 0x78],
        request: None,
        busy: false,
        done: Rc::new(Cell::new(0)),
        last_result: None,
        last_stream: None,
        last_start_bit: None,
        last_decoded_len: None,
        input_len: 0,
        pending: Rc::new(Cell::new(false)),
    };
    assert!(sh.take_request().is_none());
    assert!(!sh.pending.get(), "idle engine: no hint");

    // A start write posts (src, dst) and raises the hint (this is the
    // DecompEngineLo start-bit path).
    sh.busy = true;
    sh.request = Some((0x0208_8000, 0x0160_0000));
    sh.pending.set(true);

    assert_eq!(sh.take_request(), Some((0x0208_8000, 0x0160_0000)));
    assert!(!sh.pending.get(), "hint cleared once the request is taken");
    assert!(sh.take_request().is_none(), "take stays authoritative");
}

/// The shared timer counter (§28): `with_count` shares the cell with
/// the machine run loop; Device::tick still increments it so standalone
/// buses behave identically to pre-Z.
#[test]
fn timer_shared_count_cell_increments_and_reads_back() {
    let count = Rc::new(Cell::new(0u32));
    let log = LoggingDevice::new("t", 0).log_handle();
    let mut timer = TimerCounter::with_count("timer", count.clone(), log);

    Device::tick(&mut timer);
    Device::tick(&mut timer);
    assert_eq!(count.get(), 2);
    assert_eq!(Device::read(&mut timer, 0, AccessSize::Word), 2);

    // The firmware's software reset path: write 0 to +0x600 (§9).
    Device::write(&mut timer, 0, AccessSize::Word, 0);
    assert_eq!(count.get(), 0);
    assert_eq!(Device::read(&mut timer, 0, AccessSize::Word), 0);
}
