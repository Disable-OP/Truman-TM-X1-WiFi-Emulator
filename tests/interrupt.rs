//! Integration tests: end-to-end interrupt injection through the emulated
//! platform — program unmasks the interrupt controller over the bus, a
//! source is raised on the shared handle, the controller asserts its output
//! line, the CPU vectors to 0x80000180 (BEV=0), the handler runs and `eret`
//! returns to the interrupted loop.
//!
//! Device/CPU mechanisms are additionally covered at unit level
//! (devices::IntController tests, cpu::mips32 interrupt KATs). The real
//! firmware never enables interrupts (docs/firmware-analysis.md §11), so
//! these tests drive the mechanisms deliberately through the public
//! machine API — exactly the injection path a virtual device would use.

use truman_emulator::cpu;
use truman_emulator::firmware::{self, layout, Image};
use truman_emulator::machine::{Machine, MachineConfig, StopReason};
use truman_emulator::memory::AccessSize;

/// Synthetic 4 MiB image with the interrupt test program at offset 0x200
/// (same harness shape as tests/boot.rs).
fn synthetic_image(program: &[u32]) -> Vec<u8> {
    let mut v = vec![0xFF_u8; layout::IMAGE_SIZE as usize];
    v[0..4].copy_from_slice(&layout::PROLOGUE_WORD0.to_le_bytes());
    v[4..8].copy_from_slice(&layout::PROLOGUE_WORD1.to_le_bytes());
    let off1 = layout::STAGE1_BANNER_OFFSET as usize;
    v[off1..off1 + 23].copy_from_slice(b"zeroBoot v1.0 for 1506F");
    let off2 = layout::STAGE2_BANNER_OFFSET as usize;
    v[off2..off2 + 19].copy_from_slice(b"Boot form SPI v1.13");
    let off3 = layout::SPHE_DESC_OFFSET as usize;
    v[off3..off3 + 6].copy_from_slice(b"SPHE15");
    for (i, w) in program.iter().enumerate() {
        let o = 0x200 + i * 4;
        v[o..o + 4].copy_from_slice(&w.to_le_bytes());
    }
    // j 0xBFC00200 (nop delay) at 0x08/0x0C so the program at 0x200 runs.
    let target = 0xBFC0_0200u32;
    let jump: u32 = (0x02 << 26) | ((target & 0x0FFF_FFFF) >> 2);
    v[8..12].copy_from_slice(&jump.to_le_bytes());
    v[12..16].copy_from_slice(&0u32.to_le_bytes());
    v
}

fn img(bytes: Vec<u8>) -> Image {
    Image {
        path: std::path::PathBuf::from("synthetic-irq"),
        bytes,
        sha256: "0".repeat(64),
    }
}

// Program at 0xBFC00200 (registers: t0=8, t1=9, t2=10, t3=11):
//   mfc0  t0, Status            ; arm CPU: Status = IE | IM2 (BEV/ERL out)
//   ori   t0, t0, 0x0401
//   andi  t0, t0, 0xFFFB        ; 16-bit imm: clears the upper half (BEV, ERL)
//   mtc0  t0, Status
//   lui   t2, 0xBFFE            ; sysreg page
//   lui   t3, 0xFFFF
//   ori   t3, t3, 0xFFFF        ; t3 = 0xFFFFFFFF
//   sw    t3, 0x0480(t2)        ; intc enable word A (unmask all)
//   sw    t3, 0x0484(t2)        ; intc enable word B (unmask all)
//   lui   t0, 0x8000            ; marker base (DRAM)
//   addiu t1, zero, 0x55        ; marker value
//   j     self ; nop            ; idle loop at 0xBFC00230
fn irq_program() -> Vec<u32> {
    let mfc0_t0_status: u32 = (0x10 << 26) | (8 << 16) | (12 << 11);
    let mtc0_t0_status: u32 = (0x10 << 26) | (0x04 << 21) | (8 << 16) | (12 << 11);
    let mut p = vec![
        mfc0_t0_status,
        0x3508_0401, // ori t0, t0, 0x0401
        0x3108_FFFB, // andi t0, t0, 0xFFFB
        mtc0_t0_status,
        0x3C0A_BFFE, // lui t2, 0xBFFE
        0x3C0B_FFFF, // lui t3, 0xFFFF
        0x356B_FFFF, // ori t3, t3, 0xFFFF
        0xAD4B_0480, // sw t3, 0x0480(t2)
        0xAD4B_0484, // sw t3, 0x0484(t2)
        0x3C08_8000, // lui t0, 0x8000
        0x2409_0055, // addiu t1, zero, 0x55
    ];
    let self_pc = 0xBFC0_0200 + (p.len() as u32) * 4;
    let idx = (self_pc & 0x0FFF_FFFF) >> 2;
    p.push((0x02 << 26) | idx); // j self
    p.push(0); // delay slot
    p
}

// Handler at the BEV=0 interrupt vector 0x80000180 (DRAM):
//   sw    t1, 0x1000(t0)   ; marker: 0x55 at DRAM 0x80001000
//   eret
const HANDLER: [u32; 2] = [
    0xAD09_1000, // sw t1, 0x1000(t0)
    0x4200_0018, // eret
];

const LOOP_PC: u32 = 0xBFC0_0200 + 11 * 4;

#[test]
fn injected_interrupt_reaches_handler_and_returns() {
    let image = img(synthetic_image(&irq_program()));
    let mut m = Machine::new(MachineConfig::default(), &image);
    m.reset();

    // Install the handler at the interrupt vector (DRAM phys 0x180).
    for (i, w) in HANDLER.iter().enumerate() {
        m.bus
            .store(0x8000_0180 + (i as u32) * 4, AccessSize::Word, *w)
            .expect("write handler into DRAM");
    }

    // Arm the CPU and unmask the controller through the emulated bus.
    // (The synthetic prologue path takes ~12 steps before the program at
    // 0x200 starts; run well past that so the loop is parked.)
    let r = m.run(64);
    assert!(
        matches!(r.stop, StopReason::StepLimit),
        "program must idle in its loop: {}",
        r.stop
    );
    assert!(
        m.cpu.pc() == LOOP_PC || m.cpu.pc() == LOOP_PC + 4,
        "must be parked in the 2-word idle loop, pc=0x{:08x}",
        m.cpu.pc()
    );

    // Nothing raised: no line, no handler execution, no marker.
    assert_eq!(m.intc_output(), 0, "line must be low before any source");
    assert_eq!(
        m.bus.peek_phys(0x1000, AccessSize::Word),
        Some(0),
        "handler must not have run"
    );

    // Inject: raise source 0 through the machine API (the virtual-device
    // injection path). The controller's output line asserts on the next
    // tick, the CPU sees IP2, and the handler runs.
    m.raise_int_sources(0b1);
    let r = m.run(64);
    assert!(
        matches!(r.stop, StopReason::StepLimit),
        "handler + return must keep running: {}",
        r.stop
    );
    assert_eq!(m.intc_output(), 1, "line stays asserted while raised");
    assert_eq!(
        m.bus.peek_phys(0x1000, AccessSize::Word),
        Some(0x55),
        "handler store must have executed"
    );
    // The interrupt cycle is: loop -> deliver -> handler sw -> eret ->
    // loop. A bounded run can stop in any phase, so step until the CPU is
    // back in the interrupted loop to prove the eret return happened.
    let mut returned = m.cpu.pc() & 0xFFFF_FF00 == LOOP_PC & 0xFFFF_FF00;
    for _ in 0..12 {
        if returned {
            break;
        }
        m.run(1);
        returned = m.cpu.pc() & 0xFFFF_FF00 == LOOP_PC & 0xFFFF_FF00;
    }
    assert!(
        returned,
        "eret must return to the idle loop, pc=0x{:08x}",
        m.cpu.pc()
    );
    // With the CPU back in the loop, EXL must be clear and EPC must point
    // into the loop.
    if let Some(mips) = m
        .cpu
        .as_any()
        .downcast_ref::<truman_emulator::cpu::mips32::Mips32>()
    {
        let st = mips.cop0().status();
        assert_eq!(st & 0x2, 0, "EXL must be clear after eret");
        let epc = mips.cop0().read(14, 0);
        assert_eq!(
            epc & 0xFFFF_FF00,
            LOOP_PC & 0xFFFF_FF00,
            "EPC must point into the interrupted loop, epc=0x{epc:08x}"
        );
        let cause = mips.cop0().read(13, 0);
        assert_eq!(cause & 0x7C, 0, "ExcCode = 0 (interrupt)");
        assert_eq!(cause & (1 << 10), 1 << 10, "Cause.IP2 reflects the line");
    } else {
        panic!("expected the MIPS backend");
    }

    // Level semantics in the machine: clearing the source stops re-entry.
    m.clear_int_sources(0b1);
    let _ = m.run(16);
    assert_eq!(m.intc_output(), 0, "line drops when the source clears");
}

#[test]
fn interrupt_schedule_is_deterministic() {
    // Two identical machines with identical injection schedules must stay
    // in lockstep (interrupt state is part of the machine state).
    let image = img(synthetic_image(&irq_program()));
    let mut a = Machine::new(MachineConfig::default(), &image);
    let mut b = Machine::new(MachineConfig::default(), &image);
    a.reset();
    b.reset();
    for m in [&mut a, &mut b] {
        for (i, w) in HANDLER.iter().enumerate() {
            m.bus
                .store(0x8000_0180 + (i as u32) * 4, AccessSize::Word, *w)
                .expect("write handler");
        }
        let _ = m.run(64);
    }
    a.raise_int_sources(1);
    b.raise_int_sources(1);
    let ra = a.run(40);
    let rb = b.run(40);
    assert_eq!(ra.steps, rb.steps);
    assert_eq!(ra.pc, rb.pc, "identical schedules must stay in lockstep");
    assert_eq!(
        a.bus.peek_phys(0x1000, AccessSize::Word),
        b.bus.peek_phys(0x1000, AccessSize::Word)
    );
}

/// Guard: the REAL firmware boots identically with the interrupt machinery
/// wired in — it keeps Status.IE=0 for the whole observed horizon, so no
/// interrupt may ever fire (docs/firmware-analysis.md §11).
#[test]
fn real_firmware_boot_is_unaffected_by_irq_wiring() {
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
    let r = m.run(30_000_000);
    let console = m.uart_console();
    assert!(
        console.contains("LD eCos v1.16 1506f"),
        "boot chain must still reach the eCos milestone; console:\n{console}"
    );
    // Milestone H: the decompression engine completes, the loader's wait
    // loop exits, and the loader reaches its own result-check dead-end
    // ("LD eCos fail" — the per-symbol continuation protocol is still
    // open, docs/firmware-analysis.md §13.4). The stop is the firmware's
    // own data-execution dead-end, not an emulator fault.
    assert!(
        console.contains("LD eCos fail"),
        "engine handshake must let the loader reach its result check"
    );
    assert!(matches!(
        r.stop,
        StopReason::Cpu(cpu::CpuError::UnknownInstruction { .. })
    ));
    // The intc line must never have asserted during the real boot.
    assert_eq!(m.intc_output(), 0, "no interrupt may fire during real boot");
}

/// Virtual front-panel button press, end to end on the REAL firmware:
/// the key-scan dispatcher lives inside the decompression-engine wait
/// loop (docs/firmware-analysis.md §13), so the virtual button must be
/// held from before the loop starts. Set key-line bit 2 through the
/// machine API and verify the firmware's own dispatcher writes keycode
/// 0x400 into the key data port (docs/firmware-analysis.md §10.3/§12).
#[test]
fn real_firmware_reacts_to_injected_key_press() {
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

    // Hold bit 2 from reset: the engine-wait/key-scan loop processes the
    // pressed line while it waits for the decompression engine.
    m.press_key(0b0100);
    let _ = m.run(30_000_000);
    assert!(m.uart_console().contains("LD eCos v1.16 1506f"));

    // The firmware's own scan loop must have written keycode 0x400 into
    // the key data port — this is the firmware reacting to the virtual
    // button, not a fake response.
    let events = m.take_key_events();
    assert!(
        events.contains(&0x400),
        "keycode 0x400 must be produced by the firmware's key dispatcher; \
         events: {events:?}"
    );
    eprintln!("key events after injecting bit 2: {events:?}");

    // Release the key: the level drops and no NEW keycode arrives while
    // the line is low (drain first, then check emptiness).
    m.release_key(0b0100);
    let _ = m.run(20_000);
    let after = m.take_key_events();
    eprintln!("events observed after release (in-flight): {after:?}");
    // The dispatcher may emit at most in-flight repeats that raced with
    // the release; a fresh poll window must stay silent.
    let _ = m.run(20_000);
    assert!(
        m.take_key_events().is_empty(),
        "released key must stop producing events"
    );
}
