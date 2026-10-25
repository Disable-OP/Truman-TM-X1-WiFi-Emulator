//! Loop-overhead attribution probe (Milestone Z, §28): where does the
//! per-instruction loop spend time beyond the raw CPU step?
//!
//! Reproduce:
//! ```sh
//! cargo build --release --example loop_probe
//! ./target/release/examples/loop_probe firmware/1506HV_*.bin
//! ```
//!
//! Measured-first method (the §24.1 pattern, re-run post-V): the full
//! boot run anchors the end-to-end rate; microbenches then measure each
//! per-step loop component in isolation, using the public API only:
//! bus.tick() fan-out (whole / intc-only / timer-only buses), the engine
//! request poll, the watch/IRQ checks, and the raw-core ceiling on a
//! synthetic self-loop. Numbers from this harness are the only ones
//! quoted in docs/firmware-analysis.md §28.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use truman_emulator::cpu::{mips32::Mips32, CpuBackend};
use truman_emulator::devices::{EngineShared, IntController, LoggingDevice, TimerCounter};
use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig, StopReason};
use truman_emulator::memory::AddressSpace;

fn ns_per_iter(iters: u64, t: std::time::Duration) -> f64 {
    t.as_nanos() as f64 / iters as f64
}

/// Raw-core ceiling: a synthetic addiu/bne self-loop stepped directly
/// (no machine loop, no devices beyond the backing RAM).
fn raw_core_rate() -> f64 {
    // addiu $t0,$t0,1 ; lui $t1,0x0010 ; ori $t1,$t1,0 ; bne $t0,$t1,-3 ; nop
    // (loop 0x10_0000 iterations x 4 instr = 4 Mi instructions)
    let code: [u32; 8] = [
        0x2108_0001, // addiu $t0, $t0, 1
        0x3C09_0010, // lui  $t1, 0x0010
        0x3529_0000, // ori  $t1, $t1, 0
        0x1509_FFFD, // bne  $t0, $t1, -3 (to the addiu)
        0x0000_0000, // nop (delay slot)
        0x0000_0000, // (pad)
        0x0000_0000,
        0x0000_0000,
    ];
    let mut bus = AddressSpace::new();
    bus.add_ram("probe-ram", 0x0, 1 << 20);
    for (i, w) in code.iter().enumerate() {
        let o = 0x100 + i * 4;
        bus.poke_phys(o as u32, truman_emulator::memory::AccessSize::Word, *w);
    }
    let mut cpu = Mips32::new();
    cpu.reset(0xA000_0100);
    // Direct step users default to descriptions ON (CpuBackend contract);
    // the machine run loops disable them. Disable here too or every step
    // pays a String build (the Milestone V §24.3 finding).
    cpu.set_describe(false);
    let iters: u64 = 4_000_000 / 4 * 4;
    let t0 = Instant::now();
    for _ in 0..iters {
        let _ = cpu.step(&mut bus);
    }
    let secs = t0.elapsed().as_secs_f64();
    iters as f64 / secs / 1e6
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: loop_probe <firmware.bin>");
        std::process::exit(1);
    }
    let image = firmware::load(std::path::Path::new(&args[1])).expect("load flash image");

    // 1. Full-machine baseline (identical to examples/bench).
    let mut machine = Machine::new(MachineConfig::default(), &image);
    machine.reset();
    let t0 = Instant::now();
    let res = machine.run(2_000_000);
    let full_secs = t0.elapsed().as_secs_f64();
    let full_mips = res.steps as f64 / full_secs / 1e6;
    println!(
        "full machine.run       : {:>8.2} M instr/s ({} steps, stop {})",
        full_mips,
        res.steps,
        match res.stop {
            StopReason::StepLimit => "step-limit",
            StopReason::EntryReached { .. } => "entry",
            StopReason::Cpu(_) => "cpu",
        }
    );

    // 2. bus.tick() fan-out on the real machine's bus (timer + intc).
    const ITERS: u64 = 20_000_000;
    let t0 = Instant::now();
    for _ in 0..ITERS {
        machine.bus.tick();
    }
    println!(
        "bus.tick (timer+intc)  : {:>8.2} ns/call",
        ns_per_iter(ITERS, t0.elapsed())
    );

    // 3. intc-only tickable bus.
    let mut intc_bus = AddressSpace::new();
    let sources = Rc::new(Cell::new(0u32));
    let line = Rc::new(Cell::new(0u8));
    let log = LoggingDevice::new("p", 0).log_handle();
    let intc = IntController::new("intc", sources, line, log);
    intc_bus.add_tickable_device("intc", 0x480, 0x40, Box::new(intc));
    let t0 = Instant::now();
    for _ in 0..ITERS {
        intc_bus.tick();
    }
    println!(
        "intc tick only         : {:>8.2} ns/call",
        ns_per_iter(ITERS, t0.elapsed())
    );

    // 4. timer-only tickable bus.
    let mut timer_bus = AddressSpace::new();
    let tlog = LoggingDevice::new("p", 0).log_handle();
    let timer = TimerCounter::new("timer", tlog);
    timer_bus.add_tickable_device("timer", 0x600, 0x20, Box::new(timer));
    let t0 = Instant::now();
    for _ in 0..ITERS {
        timer_bus.tick();
    }
    println!(
        "timer tick only        : {:>8.2} ns/call",
        ns_per_iter(ITERS, t0.elapsed())
    );

    // 5. Engine request poll (borrow_mut + take_request, idle engine).
    let sh = Rc::new(std::cell::RefCell::new(EngineShared {
        regs: vec![0; 0x78],
        request: None,
        busy: false,
        done: Rc::new(Cell::new(0)),
        last_result: None,
        last_stream: None,
        last_start_bit: None,
        last_decoded_len: None,
        last_preset: None,
        input_len: 0,
        pending: Rc::new(Cell::new(false)),
    }));
    // Also measure the borrow-free hint check the machine now uses.
    let pending_hint = sh.borrow().pending.clone();
    let t0 = Instant::now();
    let mut taken: u64 = 0;
    for _ in 0..ITERS {
        if std::hint::black_box(sh.borrow_mut().take_request()).is_some() {
            taken += 1;
        }
    }
    println!(
        "engine poll (idle)     : {:>8.2} ns/call (taken={taken})",
        ns_per_iter(ITERS, t0.elapsed())
    );
    let t0 = Instant::now();
    let mut ph: u64 = 0;
    for _ in 0..ITERS {
        if pending_hint.get() {
            ph += 1;
        }
    }
    std::hint::black_box(ph);
    println!(
        "pending hint check     : {:>8.2} ns/call",
        ns_per_iter(ITERS, t0.elapsed())
    );

    // 6. Watch/IRQ per-step checks on the live machine.
    let t0 = Instant::now();
    let mut hw: u64 = 0;
    for _ in 0..ITERS {
        if std::hint::black_box(machine.bus.has_watches()) {
            hw += 1;
        }
    }
    println!(
        "has_watches check      : {:>8.2} ns/call (hits={hw})",
        ns_per_iter(ITERS, t0.elapsed())
    );

    // 7. Raw-core ceiling on the synthetic self-loop.
    println!(
        "raw core (self-loop)   : {:>8.2} M instr/s",
        raw_core_rate()
    );

    println!("\nattribution (per instruction): tick fan-out + engine poll + checks + cpu step");
}
