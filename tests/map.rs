//! Integration tests: the post-handoff map (Milestones AD+AE, §32–§33).
//!
//! [`truman_emulator::machine::Machine::map_handoff`] boots the chain up to
//! the loader's `jr v0` entry and records what executes after it: the
//! executed-page histogram, every MMIO access over the modeled device
//! windows with `(step, PC)` attribution, the DRAM data-write map (written
//! pages, total stores, bounded first-store sample), the console bytes
//! emitted after the handoff, and an optional per-instruction head trace.
//!
//! The synthetic tests pin the instrument's mechanics on a machine whose
//! handoff target is a DRAM stage installed through `copy_in`; the
//! real-firmware test pins the *current* post-handoff horizon exactly as
//! observed (evidence, not aspiration): 20 instructions, one page, no MMIO,
//! no console — the stage dies on its first bad instruction (§32.2).

use truman_emulator::cpu::CpuError;
use truman_emulator::firmware::{self, layout, Image};
use truman_emulator::machine::{
    Machine, MachineConfig, PageHistogram, StopReason, DEFAULT_HANDOFF_ENTRY,
};
use truman_emulator::memory::{AccessSize, WatchKind};

/// Synthetic 4 MiB image with the handoff launcher at offset 0x200 (same
/// harness shape as tests/boot.rs / tests/interrupt.rs).
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
        path: std::path::PathBuf::from("synthetic-map"),
        bytes,
        sha256: "0".repeat(64),
    }
}

// Launcher at 0xBFC00200: jump to the DRAM stage at kseg0 0x80002000.
//   lui   t0, 0x8000
//   ori   t0, t0, 0x2000
//   jr    t0
//   nop            (delay slot)
const LAUNCHER: [u32; 4] = [0x3C08_8000, 0x3508_2000, 0x0100_0008, 0x0000_0000];

// Stage at phys 0x2000 (kseg0 0x80002000), installed with copy_in:
//   lui   t2, 0xBFFE       ; sysreg page (kseg1)
//   ori   t3, t2, 0x0900   ; UART TX register (console output)
//   addiu t1, zero, 'M'
//   sb    t1, 0(t3)        ; emit 'M' -> instrumented MMIO write
//   lui   t4, 0x8000       ; DRAM (kseg0) — NOT a device window
//   sw    t1, 0x3000(t4)   ; marker store — a DRAM data write: belongs in
//                          ; the Milestone AE write map, never in MMIO
//   j     self ; nop       ; park: the post window ends on its step budget
const STAGE: [u32; 8] = [
    0x3C0A_BFFE, // lui   t2, 0xBFFE
    0x354B_0900, // ori   t3, t2, 0x0900
    0x2409_004D, // addiu t1, zero, 0x4D ('M')
    0xA169_0000, // sb    t1, 0(t3)
    0x3C0C_8000, // lui   t4, 0x8000
    0xAD89_3000, // sw    t1, 0x3000(t4)
    0x0800_0806, // j 0x80002018 (self)
    0x0000_0000, // nop (delay slot)
];

const STAGE_ENTRY: u32 = 0x8000_2000;
const SELF_PC: u32 = 0x8000_2018;

#[test]
fn synthetic_handoff_map_records_pages_mmio_and_console() {
    let image = img(synthetic_image(&LAUNCHER));
    let cfg = MachineConfig {
        handoff_entry: STAGE_ENTRY,
        ..MachineConfig::default()
    };
    let mut m = Machine::new(cfg, &image);
    // Install the stage before reset; reset() only resets CPU state.
    let bytes: Vec<u8> = STAGE.iter().flat_map(|w| w.to_le_bytes()).collect();
    assert_eq!(
        m.bus.copy_in(0x2000, &bytes),
        bytes.len(),
        "stage must land in DRAM"
    );
    m.reset();

    const POST: u64 = 200;
    let map = m.map_handoff(1_000, POST, 8);

    // Handoff: reached at the launcher's jr, with its target register live.
    assert!(map.handoff_reached, "handoff must be reached");
    assert_eq!(map.handoff_pc, STAGE_ENTRY);
    assert!(map.pre_steps < 64, "pre steps: {}", map.pre_steps);
    let regs = map.handoff_regs.as_ref().expect("snapshot at handoff");
    assert_eq!(regs.regs[8], STAGE_ENTRY, "t0 must hold the stage entry");

    // Post window: the stage parks in its self-loop, so the budget is the
    // only stop — and every one of the budget's instructions is recorded.
    assert_eq!(map.post_stop, Some(StopReason::StepLimit));
    assert_eq!(map.post_steps, POST);
    assert_eq!(map.final_pc & 0xFFFF_FFFC, SELF_PC);
    assert_eq!(map.exec_pages, vec![(STAGE_ENTRY & 0xFFFF_F000, POST)]);
    assert_eq!(map.head.len(), 8, "head trace capped at the request");
    assert_eq!(map.head[0].pc, STAGE_ENTRY);

    // MMIO: exactly the console store, attributed to the stage's sb; the
    // DRAM marker store is not a device access and must not appear.
    assert_eq!(map.mmio.len(), 1, "mmio: {:?}", map.mmio);
    let (step, pc, phys, kind, size, value) = map.mmio[0];
    assert_eq!(step, 3, "the sb is the stage's 4th instruction");
    assert_eq!(pc, STAGE_ENTRY + 0x0C);
    assert_eq!(phys, 0x1FFE_0900);
    assert_eq!(kind, WatchKind::Write);
    assert_eq!(size, AccessSize::Byte);
    assert_eq!(value, 0x4D);
    assert!(!map.mmio_overflow);

    // Written-memory map (Milestone AE, §33): exactly the marker store —
    // counted, histogrammed and sampled, with full attribution; and it is
    // a DRAM write, so it never leaks into the MMIO record above.
    assert_eq!(map.writes_total, 1, "writes: {:?}", map.write_sample);
    assert_eq!(map.write_pages, vec![(0x3000, 1)]);
    assert_eq!(map.write_sample.len(), 1);
    let w = &map.write_sample[0];
    assert_eq!(w.step, 5, "the sw is the stage's 6th instruction");
    assert_eq!(w.pc, STAGE_ENTRY + 0x14);
    assert_eq!(w.vaddr, 0x8000_3000, "kseg0 view of the same DRAM byte");
    assert_eq!(w.phys, 0x3000);
    assert_eq!(w.size, AccessSize::Word);
    assert_eq!(w.value, 0x4D);

    // Console: exactly the byte emitted after the handoff.
    assert_eq!(map.console_after, "M");
}

#[test]
fn unreached_handoff_is_reported_without_post_data() {
    // A program that parks forever before the handoff entry
    // (j 0xBFC00200 = self; nop).
    let park = [0x0BF0_0080_u32, 0x0000_0000];
    let image = img(synthetic_image(&park));
    let mut m = Machine::new(MachineConfig::default(), &image);
    m.reset();
    let map = m.map_handoff(1_000, 1_000, 8);
    assert!(!map.handoff_reached);
    assert_eq!(map.pre_steps, 1_000, "the pre budget is fully consumed");
    assert_eq!(map.post_steps, 0);
    assert_eq!(map.post_stop, None);
    assert!(map.exec_pages.is_empty());
    assert!(map.mmio.is_empty());
    assert_eq!(map.writes_total, 0);
    assert!(map.write_pages.is_empty());
    assert!(map.write_sample.is_empty());
    assert!(map.console_after.is_empty());
    assert!(map.head.is_empty());
    assert_eq!(map.handoff_pc, DEFAULT_HANDOFF_ENTRY);
}

#[test]
fn page_histogram_orders_and_counts() {
    let mut h = PageHistogram::new();
    for _ in 0..7 {
        h.record(0x100);
    }
    for _ in 0..3 {
        h.record(0x9_0200);
    }
    assert_eq!(h.total(), 10);
    assert_eq!(h.page_count(), 2);
    assert_eq!(h.count_at(0x100), Some(7));
    assert_eq!(
        h.count_at(0x9_0204),
        Some(3),
        "page base is masked to 4 KiB"
    );
    assert_eq!(h.count_at(0x1234_5678), None, "untouched page");
    // Descending by count, ties ascending by base. Entries are page bases.
    assert_eq!(h.top_pages(8), vec![(0x0, 7), (0x9_0000, 3)]);
    assert_eq!(h.top_pages(1), vec![(0x0, 7)]);
}

// A stage that renders before parking — 100 word stores to one DRAM page
// (0x80005000), then a self-loop:
//   lui   t4, 0x8000       ; DRAM (kseg0)
//   addiu t1, zero, 0x40   ; pixel value
//   addiu t2, zero, 100    ; loop bound
// loop:
//   sw    t1, 0x5000(t4)
//   addiu t1, t1, 1
//   addiu t2, t2, -1
//   bne   t2, zero, loop   ; offset -4
//   nop                    ; (delay slot)
// park:
//   j     park ; nop
const RENDER_STAGE: [u32; 12] = [
    0x3C0C_8000, // 0x00 lui   t4, 0x8000
    0x2409_0040, // 0x04 addiu t1, zero, 0x40
    0x240A_0064, // 0x08 addiu t2, zero, 100
    0xAD89_5000, // 0x0C sw    t1, 0x5000(t4)
    0x2529_0001, // 0x10 addiu t1, t1, 1
    0x214A_FFFF, // 0x14 addiu t2, t2, -1
    0x1540_FFFC, // 0x18 bne   t2, zero, loop
    0x0000_0000, // 0x1C nop (delay slot)
    0x0800_0808, // 0x20 j 0x80002020 (park self)
    0x0000_0000, // 0x24 nop (delay slot)
    0x0000_0000,
    0x0000_0000,
];

/// The write map under sustained rendering (Milestone AE, §33): every store
/// counted in the histogram and total, the sample capped at the *first*
/// [`truman_emulator::machine::WRITE_SAMPLE_CAP`] events with intact
/// attribution, MMIO untouched, no cap overflow. This is the shape a
/// framebuffer discovery takes on the real post-capture path: one page,
/// one stride, values advancing per row.
#[test]
fn render_loop_write_map_counts_all_and_caps_the_sample() {
    let image = img(synthetic_image(&LAUNCHER));
    let cfg = MachineConfig {
        handoff_entry: STAGE_ENTRY,
        ..MachineConfig::default()
    };
    let mut m = Machine::new(cfg, &image);
    let bytes: Vec<u8> = RENDER_STAGE.iter().flat_map(|w| w.to_le_bytes()).collect();
    m.bus.copy_in(0x2000, &bytes);
    m.reset();

    let map = m.map_handoff(1_000, 700, 0);

    assert_eq!(map.post_stop, Some(StopReason::StepLimit));
    assert_eq!(map.writes_total, 100, "every store counted");
    assert_eq!(map.write_pages, vec![(0x5000, 100)]);
    assert_eq!(
        map.write_sample.len(),
        truman_emulator::machine::WRITE_SAMPLE_CAP,
        "sample capped, not total"
    );
    // The sample is the *first* stores: sw at step 3, then every 5 steps
    // (sw, addiu, addiu, bne, nop per iteration).
    for (k, w) in map.write_sample.iter().enumerate() {
        assert_eq!(w.step, 3 + 5 * k as u64, "sample ordering, item {k}");
        assert_eq!(w.pc, STAGE_ENTRY + 0x0C, "the loop's sw");
        assert_eq!(w.vaddr, 0x8000_5000);
        assert_eq!(w.phys, 0x5000);
        assert_eq!(w.size, AccessSize::Word);
        assert_eq!(w.value, 0x40 + k as u32, "the counter advances");
    }
    assert_eq!(map.write_sample[63].value, 0x40 + 63);
    assert!(map.mmio.is_empty(), "no device access: {:?}", map.mmio);
    assert!(!map.mmio_overflow, "100 stores are far below the cap");
    assert!(map.console_after.is_empty());
}

/// The shared watch-hit cap flows through the AE classifier correctly
/// (Milestone AF): with the cap at 1, the first post-handoff hit (the
/// stage's device write, step 3) is recorded and classified into the MMIO
/// record, the second hit (the DRAM marker store, step 5) is dropped *by
/// the cap* — not misrouted — `mmio_overflow` is set, and the write map
/// degrades to empty rather than to a wrong partial count. The classifier
/// itself never loses a recorded hit; the cap can, and the overflow flag
/// is the instrument's honesty about it. The console byte is unaffected:
/// watches observe accesses, they do not gate them.
#[test]
fn map_watch_cap_overflow_is_flagged_through_the_classifier() {
    let image = img(synthetic_image(&LAUNCHER));
    let cfg = MachineConfig {
        handoff_entry: STAGE_ENTRY,
        ..MachineConfig::default()
    };
    let mut m = Machine::new(cfg, &image);
    let bytes: Vec<u8> = STAGE.iter().flat_map(|w| w.to_le_bytes()).collect();
    m.bus.copy_in(0x2000, &bytes);
    m.bus.set_watch_cap(1);
    m.reset();

    let map = m.map_handoff(1_000, 200, 8);

    assert_eq!(map.post_stop, Some(StopReason::StepLimit));
    assert_eq!(map.mmio.len(), 1, "mmio: {:?}", map.mmio);
    let (step, _pc, phys, kind, _size, _value) = map.mmio[0];
    assert_eq!(step, 3, "the recorded hit is the device write");
    assert_eq!(phys, 0x1FFE_0900);
    assert_eq!(kind, WatchKind::Write);
    assert_eq!(map.writes_total, 0, "the DRAM store hit the cap");
    assert!(map.write_pages.is_empty());
    assert!(map.write_sample.is_empty());
    assert!(map.mmio_overflow, "the shared cap gates both records");
    assert_eq!(map.console_after, "M", "watches observe, not gate");
}

fn real_firmware_path() -> Option<std::path::PathBuf> {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("firmware")
        .read_dir()
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().map(|x| x == "bin").unwrap_or(false))
}

/// The current post-handoff horizon of the real firmware, pinned exactly as
/// observed (§32.2, extended by §33): the loader hands off at step
/// 1,301,763 with the driver's return value (v1 = 0x13800 = 78×1024
/// pixels) and the PNG width (a0 = 0x400) still live; the RFC-decoded
/// stage executes 20 instructions — all in its entry page, touching no
/// device, storing nothing to DRAM, emitting no console output — and dies
/// on the 0xFFFF word at 0xA1600050. This is the §19.1 head paradox in
/// dynamic terms. When the capture-recovered value tables land, this same
/// instrument maps the real menu path instead; the pin is the honest
/// record of what the RFC tables produce.
#[test]
fn real_firmware_handoff_map_pins_the_current_horizon() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let image = firmware::load(&path).expect("load real firmware");
    let mut m = Machine::new(MachineConfig::default(), &image);
    m.reset();
    let map = m.map_handoff(2_000_000, 1_000_000, 64);

    assert!(map.handoff_reached, "boot must reach the handoff");
    assert_eq!(map.handoff_pc, 0xA160_0000);
    assert_eq!(map.pre_steps, 1_301_763);

    let regs = map.handoff_regs.as_ref().expect("snapshot at handoff");
    assert_eq!(regs.regs[2], 0xA160_0000, "v0 = the jump target");
    assert_eq!(regs.regs[3], 0x13_800, "v1 = the driver's 79,872 return");
    assert_eq!(regs.regs[4], 0x400, "a0 = the PNG width constant");
    assert_eq!(regs.regs[29], 0x8200_4B80, "sp = the loader stack");

    // The horizon: 20 instructions, one page, no MMIO, no console.
    assert_eq!(map.post_steps, 20);
    assert_eq!(
        map.post_stop,
        Some(StopReason::Cpu(CpuError::UnknownInstruction {
            pc: 0xA160_0050,
            word: 0x0000_FFFF,
        }))
    );
    assert_eq!(map.final_pc, 0xA160_0050);
    assert_eq!(map.exec_pages, vec![(0xA160_0000, 20)]);
    assert!(map.mmio.is_empty(), "no post-handoff MMIO: {:?}", map.mmio);
    assert!(!map.mmio_overflow);
    assert_eq!(map.writes_total, 0, "the stage stores nothing (§33)");
    assert!(map.write_pages.is_empty());
    assert!(map.write_sample.is_empty());
    assert!(map.console_after.is_empty());
    assert_eq!(map.head.len(), 20);
    assert_eq!(map.head[0].pc, 0xA160_0000);
    assert_eq!(map.head[19].pc, 0xA160_004C);
}

/// The instrument itself must be deterministic: two fresh machines produce
/// identical maps (field-wise; the map's own types are not PartialEq).
#[test]
fn real_firmware_handoff_map_is_deterministic() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let image = firmware::load(&path).expect("load real firmware");
    let mut a = Machine::new(MachineConfig::default(), &image);
    a.reset();
    let ma = a.map_handoff(2_000_000, 100_000, 32);
    let mut b = Machine::new(MachineConfig::default(), &image);
    b.reset();
    let mb = b.map_handoff(2_000_000, 100_000, 32);

    assert_eq!(ma.pre_steps, mb.pre_steps);
    assert_eq!(ma.post_steps, mb.post_steps);
    assert_eq!(ma.post_stop, mb.post_stop);
    assert_eq!(ma.exec_pages, mb.exec_pages);
    assert_eq!(ma.mmio, mb.mmio);
    assert_eq!(ma.writes_total, mb.writes_total);
    assert_eq!(ma.write_pages, mb.write_pages);
    assert_eq!(ma.write_sample, mb.write_sample);
    assert_eq!(ma.console_after, mb.console_after);
    let pa: Vec<u32> = ma.head.iter().map(|r| r.pc).collect();
    let pb: Vec<u32> = mb.head.iter().map(|r| r.pc).collect();
    assert_eq!(pa, pb);
}
