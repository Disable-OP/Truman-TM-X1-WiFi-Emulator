//! Integration tests: boot flow, machine composition, real-firmware smoke.

use truman_emulator::firmware::{self, layout, Image};
use truman_emulator::machine::{Machine, MachineConfig, StopReason};
use truman_emulator::{boot, cpu};

/// Synthetic 4 MiB image with a tiny runnable program in the stage-1 area.
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
    // program right after the verified prologue region (offset 0x200)
    for (i, w) in program.iter().enumerate() {
        let o = 0x200 + i * 4;
        v[o..o + 4].copy_from_slice(&w.to_le_bytes());
    }
    // jump over the magic-prologue words so a program at 0x200 runs
    // (j at 0x08, nop delay slot at 0x0C). j target = (delay-slot pc &
    // 0xF0000000) | (index << 2), so the index must carry region bits.
    let target = 0xBFC0_0200u32;
    let jump: u32 = (0x03 << 26) | ((target & 0x0FFF_FFFF) >> 2);
    v[8..12].copy_from_slice(&jump.to_le_bytes());
    v[12..16].copy_from_slice(&0u32.to_le_bytes());
    v
}

fn img(bytes: Vec<u8>) -> Image {
    Image {
        path: std::path::PathBuf::from("synthetic"),
        bytes,
        sha256: "0".repeat(64),
    }
}

#[test]
fn machine_runs_synthetic_program_from_flash_window() {
    // Program at flash offset 0x200 (PC 0xBFC00200):
    //   lui  t0, 0xBFFC      ; IRAM overlay base
    //   ori  t0, t0, 0x0E00  ; +0x0E00
    //   lui  t1, 0xCAFE
    //   ori  t1, t1, 0xBABE
    //   sw   t1, -4(t0)      ; store into the overlay (stack area)
    //   lw   t2, -4(t0)      ; read back
    //   j    .               ; loop forever (branch to self)
    //   nop
    let j_target_addr = 0xBFC0_0000u32 + 0x200 + 6 * 4; // the j instruction itself
    let j_self: u32 = (0x03 << 26) | ((j_target_addr & 0x0FFF_FFFF) >> 2);
    let program = [
        0x3C08_BFFCu32,
        0x3508_0E00,
        0x3C09_CAFE,
        0x3529_BABE,
        0xAD09_FFFC, // sw t1, -4(t0)
        0x8D0A_FFFC, // lw t2, -4(t0)
        j_self,
        0,
    ];
    let image = img(synthetic_image(&program));
    let mut machine = Machine::new(MachineConfig::default(), &image);
    boot::reset_with_diagnostics(&mut machine, false);
    // Run; the self-loop must hit the step limit, not fault.
    let result = machine.run(1_000);
    assert!(
        matches!(result.stop, StopReason::StepLimit),
        "expected step limit, got {}",
        result.stop
    );
    assert_eq!(result.steps, 1_000);
    // Loaded word must be visible in DRAM? No — overlay is its own region;
    // verify via the register file: t2 must equal 0xCAFEBABE.
    let s = machine.snapshot();
    assert_eq!(s.regs[10], 0xCAFE_BABE, "lw must read back the stored word");
    assert_eq!(s.regs[8], 0xBFFC_0E00);
    // And the overlay bytes must physically contain the value.
    assert_eq!(
        machine
            .bus
            .peek_phys(0x1FFC_0DFC, truman_emulator::memory::AccessSize::Word),
        Some(0xCAFE_BABE)
    );
}

#[test]
fn machine_step_limit_and_termination_are_deterministic() {
    let program = [0x3C08_1234u32, 0x0000_0000]; // lui + nop, then falls off into FF data
    let image = img(synthetic_image(&program));
    let mut m1 = Machine::new(MachineConfig::default(), &image);
    let mut m2 = Machine::new(MachineConfig::default(), &image);
    boot::reset_with_diagnostics(&mut m1, false);
    boot::reset_with_diagnostics(&mut m2, false);
    let r1 = m1.run(10);
    let r2 = m2.run(10);
    assert_eq!(r1.steps, r2.steps);
    assert_eq!(r1.pc, r2.pc, "identical machines must stay in lockstep");
    // 0xFFFFFFFF is not a valid instruction → clean UnknownInstruction stop.
    assert!(
        matches!(
            r1.stop,
            StopReason::Cpu(cpu::CpuError::UnknownInstruction { .. })
        ),
        "got {}",
        r1.stop
    );
}

#[test]
fn sysreg_param_window_supports_read_modify_write() {
    // Reproduce the loader's first RMW: lw 0xBFFE0210; or 0x380; sw back.
    let program = [
        0x3C1A_BFFEu32, // lui k0, 0xBFFE
        0x8F5B_0210,    // lw  k1, 0x210(k0)
        0x3C01_0380,    // lui at, 0x380
        0x0361_D825,    // or  k1, k1, at
        0xAF5B_0210,    // sw  k1, 0x210(k0)
        0x0000_0000,    // nop
    ];
    let image = img(synthetic_image(&program));
    let mut machine = Machine::new(MachineConfig::default(), &image);
    boot::reset_with_diagnostics(&mut machine, false);
    // 4 boot-path steps (2 prologue ops + j + delay slot) + 6 program steps.
    // A run of exactly 10 hits the step limit only if every access succeeded:
    // a faulting lw/sw would stop early with StopReason::Cpu.
    let result = machine.run(10);
    assert!(
        matches!(result.stop, StopReason::StepLimit),
        "RMW over the param window must not fault: {}",
        result.stop
    );
    let s = machine.snapshot();
    assert_eq!(
        s.regs[27], 0x0380_0000,
        "k1 = 0 | 0x03800000 (sysreg resets to zero)"
    );
    // The store side must be physically visible in the sysreg-params region.
    assert_eq!(
        machine
            .bus
            .peek_phys(0x1FFE_0210, truman_emulator::memory::AccessSize::Word),
        Some(0x0380_0000),
        "sw must land in the sysreg param RAM"
    );
}

/// Locate the committed real firmware (if present) for the smoke test.
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

#[test]
fn real_firmware_stage1_executes_past_prologue() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let image = firmware::load(&path).expect("load real firmware");
    let id = firmware::identify(&image).expect("identify real firmware");
    assert!(id.notes.len() >= 3);

    let mut machine = Machine::new(MachineConfig::default(), &image);
    boot::reset_with_diagnostics(&mut machine, true);

    // Execute the first 50_000 instructions of the real stage-1 loader.
    // This is the Milestone C acceptance: the real MIPS32r2 boot code must
    // run without CPU-level faults (fetch/data) for a substantial stretch.
    let result = machine.run(50_000);
    eprintln!(
        "smoke: {} steps, stop: {}, final pc=0x{:08x}",
        result.steps, result.stop, result.pc
    );
    match &result.stop {
        StopReason::StepLimit => { /* ideal */ }
        StopReason::Cpu(e) => {
            // A clean stop is acceptable at this stage IF it happened after
            // real progress; require at least 1000 instructions executed.
            assert!(result.steps >= 1_000, "stopped too early ({e})");
        }
    }
    let s = machine.snapshot();
    // The loader re-programs sp during init; assert it stays inside the
    // IRAM overlay window (0xBFFC0000..0xBFFC1000) it was given.
    assert!(
        (0xBFFC_0000..0xBFFC_1000).contains(&s.regs[29]),
        "sp must stay in the IRAM overlay, got 0x{:08x}",
        s.regs[29]
    );
}

/// Milestone E acceptance: the complete boot chain — stage-1 zeroBoot,
/// stage-2 "Boot form SPI" loader, DDR training ("Trim PASS"), payload copy
/// ("Go!") — must reach the decompressed main firmware ("1ST BOOT" banner
/// printed over the emulated UART). The banner string is NOT present in the
/// flash file, so seeing it on the console proves the firmware's own code
/// decompressed the payload into DRAM under our interpreter.
#[test]
fn real_firmware_boots_through_ddr_training_into_main_firmware() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let image = firmware::load(&path).expect("load real firmware");
    let mut machine = Machine::new(MachineConfig::default(), &image);
    boot::reset_with_diagnostics(&mut machine, false);

    // 1,330,000 steps lands shortly after the "Go eCos" jump into the
    // decompressed eCos image (deterministic; see the step-count assert
    // below). A larger budget would just nop-slide through the zero-filled
    // DRAM beyond the decoded image region.
    let result = machine.run(1_330_000);
    let console = machine.uart_console();
    eprintln!(
        "boot-chain: {} steps, stop: {}, pc=0x{:08x}",
        result.steps, result.stop, result.pc
    );
    eprintln!("boot-chain console:\n{console}");

    // Ordered boot progress markers (each implies the previous ran).
    for marker in [
        "zeroBoot v1.0 for 1506F",
        "jump to second boot!!",
        "Boot form SPI v1.13",
        "Calc RAM Param",
        "DDR-V6.2.01.09-v",
        "Trim PASS",
        "copy fw to RAM",
        "Go!",
        "1ST BOOT",
        "Zero BSS",
        "IntCtrl",
        "LD eCos v1.16 1506f",
    ] {
        assert!(
            console.contains(marker),
            "boot chain must reach {marker:?}; console so far:\n{console}"
        );
    }

    // The main firmware executes from its documented load address
    // (kseg1 0xA2000000 = phys 0x02000000, DRAM offset 32 MiB); with the
    // engine event modeled the boot continues past it into the eCos
    // stage, so the final PC is asserted below instead.
    // Milestone I (§13.3): the decompression engine completes, the loader's
    // wait loop takes the bit-5 event branch (ack 0x2000, event counter
    // reaches the header-mirrored limit), the driver returns 0x13800 and
    // the loader's success path prints "Go eCos" / "Boot from SPI" and
    // jumps to the decompressed eCos entry at 0xA1600000 (phys
    // 0x01600000). The eCos image bytes themselves are the engine's
    // decode — its exact LZ value semantics are still being pinned down
    // (§13.4), so no eCos-internal behavior is asserted yet, only the
    // real control flow of the unmodified boot chain.
    for marker in ["LD eCos v1.16 1506f", "Go eCos", "Boot from SPI"] {
        assert!(
            console.contains(marker),
            "boot chain must reach {marker:?}; console so far:\n{console}"
        );
    }
    assert!(
        !console.contains("LD eCos fail"),
        "loader failure path must not be taken"
    );
    // The boot must be executing inside the eCos image page.
    assert_eq!(
        result.pc & 0xFFF0_0000,
        0xA160_0000,
        "boot must reach the eCos entry page (0xA1600000), got {:#x}",
        result.pc
    );
    assert!(
        (1_290_000..1_400_000).contains(&result.steps),
        "deterministic step count into the eCos stage, got {}",
        result.steps
    );

    // NOTE: "1ST BOOT" is NOT stored in memory — the main firmware prints
    // it via unrolled immediate stores (observed at 0xA2000340..0xA2000448),
    // so a memory scan cannot find it. The console output above IS the proof
    // that the decompressed main-firmware code runs from DRAM.
}
