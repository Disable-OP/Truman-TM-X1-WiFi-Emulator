//! Milestone R (§20): the entry harness — the register-aware dynamic
//! oracle.
//!
//! The machine boots the real firmware with a watch at 0xA1600000 (the
//! loader's `jr v0` target, the decompressed eCos entry), captures the
//! post-jump register state and full DRAM, then re-enters candidate
//! decodes as real MIPS code. These tests pin:
//!
//! * the entry state (v0 = jump target, v1 = 0x13800 = 0x400 × 78, §19.1),
//! * the baseline engine-model head dying dynamically exactly where the
//!   §19.2 static oracle predicted (+0x50, 0x0000ffff reserved),
//! * full state restoration between candidates (deterministic replays),
//! * the step-limit path (a self-loop candidate runs clean to budget).

use truman_emulator::boot;
use truman_emulator::cpu::CpuError;
use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig, StopReason};
use truman_emulator::memory::AccessSize;

/// The eCos entry: the loader's `jr v0` target (§13.3, §19.1).
const ENTRY: u32 = 0xA160_0000;
/// PNG_PXL_EXT_ADDR (§17.1).
const PIXEL_BUF: u32 = 0x0160_0000;
const IMG_BYTES: usize = 79_872; // 1024 × 78, pinned by tests/png.rs
const DRAM_SIZE: usize = 0x0400_0000;

fn real_firmware_path() -> Option<std::path::PathBuf> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let fw_dir = std::path::Path::new(manifest).join("firmware");
    std::fs::read_dir(fw_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.extension().map(|x| x == "bin").unwrap_or(false)
                && p.file_name()
                    .map(|n| n.to_string_lossy().contains("1506HV"))
                    .unwrap_or(false)
        })
}

/// Boot the real firmware to the eCos entry; returns the machine with the
/// entry state live plus the captured register snapshot and DRAM image.
struct Entry {
    machine: Machine,
    snap: truman_emulator::cpu::CpuSnapshot,
    dram0: Vec<u8>,
    baseline: Vec<u8>,
}

fn boot_to_entry() -> Option<Entry> {
    let path = real_firmware_path()?;
    let image = firmware::load(&path).expect("load real firmware");
    let mut machine = Machine::new(MachineConfig::default(), &image);
    boot::reset_with_diagnostics(&mut machine, false);
    let res = machine.run_until_pc(4_000_000, ENTRY);
    if !matches!(res.stop, StopReason::EntryReached { .. }) {
        panic!("boot did not reach the eCos entry: {:?}", res.stop);
    }
    let snap = machine.snapshot();
    let mut dram0 = vec![0u8; DRAM_SIZE];
    assert_eq!(machine.bus.copy_out(0, &mut dram0), DRAM_SIZE);
    let mut baseline = vec![0u8; IMG_BYTES];
    machine.bus.copy_out(PIXEL_BUF, &mut baseline);
    Some(Entry {
        machine,
        snap,
        dram0,
        baseline,
    })
}

/// Re-enter `candidate` (zero-padded to the image size) from the captured
/// entry state, with a fresh full-DRAM restore.
fn evaluate(entry: &mut Entry, candidate: &[u8], budget: u64) -> (u64, u32, StopReason) {
    entry.machine.bus.copy_in(0, &entry.dram0);
    entry.machine.restore_cpu_state(&entry.snap);
    let mut padded = candidate.to_vec();
    padded.truncate(IMG_BYTES);
    padded.resize(IMG_BYTES, 0);
    entry.machine.bus.copy_in(PIXEL_BUF, &padded);
    let out = entry.machine.run(budget);
    (out.steps, out.pc, out.stop)
}

#[test]
fn boot_reaches_the_ecos_entry_with_the_loaders_return_state() {
    let Some(e) = boot_to_entry() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let snap = e.snap;
    assert_eq!(snap.pc, ENTRY, "watch stops with the PC at the entry");
    assert_eq!(snap.regs[2], ENTRY, "v0 = the jump target (jr v0)");
    assert_eq!(
        snap.regs[3], 0x13800,
        "v1 = 0x400 × 78 = the driver's success return (§19.1)"
    );
    // The engine wrote its decode into the pixel buffer before the jump:
    // the as-booted buffer is the baseline candidate and is non-trivial.
    let nz = e.baseline.iter().filter(|&&b| b != 0).count();
    assert!(
        nz > 10_000,
        "engine output should populate the image, got {nz} non-zero bytes"
    );
}

#[test]
fn engine_model_head_faults_where_the_static_oracle_predicted() {
    let Some(mut e) = boot_to_entry() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    // §19.2 (static): the m1/RFC head's first reserved word is +0x50 =
    // 0x0000ffff, built from literal bytes ff ff. The dynamic oracle must
    // confirm this under the loader's real register state.
    let base = e.baseline.clone();
    let (steps, pc, stop) = evaluate(&mut e, &base, 4096);
    match &stop {
        StopReason::Cpu(CpuError::UnknownInstruction { pc: fpc, word }) => {
            assert_eq!(
                *fpc, 0xA160_0050,
                "fault at the static oracle's reserved word"
            );
            assert_eq!(*word, 0x0000_ffff);
        }
        other => panic!("expected the +0x50 reserved fault, got {other:?}"),
    }
    assert_eq!(steps, 20, "words +0x00..+0x4c are all executed cleanly");
    assert_eq!(pc, 0xA160_0050);
}

#[test]
fn state_restoration_between_candidates_is_complete() {
    let Some(mut e) = boot_to_entry() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    // A candidate that stores 0xdeadbeef to 0xA1600100 and then hits a
    // reserved opcode — it dirties both registers and DRAM before dying.
    let words: [u32; 6] = [
        0x3C08_A160, // lui  t0, 0xa160
        0x2508_0100, // addiu t0, t0, 0x100
        0x3C09_DEAD, // lui  t1, 0xdead
        0x3529_BEEF, // ori  t1, t1, 0xbeef
        0xAD09_0000, // sw   t1, 0(t0)
        0xFC00_0000, // reserved (opcode 0x3f)
    ];
    let mut writer = Vec::new();
    for w in words {
        writer.extend_from_slice(&w.to_le_bytes());
    }
    let (steps, pc, stop) = evaluate(&mut e, &writer, 4096);
    assert_eq!(steps, 5, "the writer executes five instructions");
    assert_eq!(pc, 0xA160_0014);
    assert!(matches!(
        stop,
        StopReason::Cpu(CpuError::UnknownInstruction { .. })
    ));
    // The store landed (kseg1 alias 0xA1600100 -> phys 0x01600100).
    let saw = e
        .machine
        .bus
        .peek_phys(0x0160_0100, AccessSize::Word)
        .expect("dram peek");
    assert_eq!(saw, 0xdead_beef, "the candidate's store must be visible");

    // A plain restore (no candidate rewrite) must wipe the dirty byte —
    // the captured DRAM image is the boot-time state.
    e.machine.bus.copy_in(0, &e.dram0);
    let want = u32::from_le_bytes([
        e.dram0[0x0160_0100],
        e.dram0[0x0160_0101],
        e.dram0[0x0160_0102],
        e.dram0[0x0160_0103],
    ]);
    let wiped = e
        .machine
        .bus
        .peek_phys(0x0160_0100, AccessSize::Word)
        .expect("dram peek");
    assert_eq!(wiped, want, "copy_in must restore the boot-time DRAM");

    // A second, identical evaluation must observe the identical outcome —
    // only possible if registers AND memory are fully restored.
    let (steps2, pc2, stop2) = evaluate(&mut e, &writer, 4096);
    assert_eq!((steps, pc), (steps2, pc2));
    assert_eq!(
        format!("{stop:?}"),
        format!("{stop2:?}"),
        "replays must be bit-identical under full state restoration"
    );
}

#[test]
fn synthetic_self_loop_runs_clean_to_the_step_budget() {
    let Some(mut e) = boot_to_entry() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    // j 0xA1600000 (target index (0xA1600000>>2)&0x3FFFFFF = 0x00580000)
    // followed by nops: a tight self-loop that never faults. This pins
    // the oracle's step-limit path — the shape a correct eCos entry's
    // survival would take.
    let mut cand = Vec::with_capacity(IMG_BYTES);
    cand.extend_from_slice(&0x0858_0000u32.to_le_bytes());
    while cand.len() < IMG_BYTES {
        cand.extend_from_slice(&0u32.to_le_bytes());
    }
    let budget = 2_000;
    let (steps, pc, stop) = evaluate(&mut e, &cand, budget);
    assert_eq!(steps, budget, "self-loop must run to the budget");
    assert!(matches!(stop, StopReason::StepLimit), "got {stop:?}");
    assert_eq!(pc, ENTRY, "the loop returns to the entry");
}

#[test]
fn harness_evaluates_file_candidates_from_the_entry_state() {
    // End-to-end sanity of the example's core sequence: restore -> write
    // candidate -> run, using the baseline captured through the public
    // API only (no firmware-file dependence beyond boot_to_entry).
    let Some(mut e) = boot_to_entry() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let base = e.baseline.clone();
    let (steps, pc, stop) = evaluate(&mut e, &base, 4096);
    assert_eq!(pc, 0xA160_0050);
    assert_eq!(steps, 20);
    assert!(matches!(stop, StopReason::Cpu(_)));
}
