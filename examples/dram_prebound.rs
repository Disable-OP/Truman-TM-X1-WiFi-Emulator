//! Milestone P probe 2 (§17.5 item 2): bound the pre-decode DRAM content
//! below the PNG pixel buffer.
//!
//! The engine's LZ window semantics determine what a match with distance
//! beyond the current output position copies. In the emulator's model the
//! window is the driver's zero-filled band buffer (deterministic zeros).
//! The alternative — the window being the output stream itself, so that
//! dist>output reads reach BELOW the pixel base (phys 0x01600000) into
//! 0x015Fxxxx — depends on the real device's pre-decode DRAM content
//! there, which software cannot observe directly. This probe BOUNDS it:
//!
//!   * canary pass — seed 0x01500000..0x01600000 with 0xA5 words, run the
//!     real boot to completion, dump: every byte != canary was written by
//!     the boot itself (deterministic, extractable content);
//!   * fresh pass — same run without canary: the emulator's pre-decode
//!     content (zero-init + boot writes);
//!   * the two passes must agree on the boot-written bytes (if the boot
//!     branched on the pre-existing content, the canary would have changed
//!     its behavior — the comparison catches that).
//!
//! If the region is fully boot-written, an alternative-semantics replay can
//! use the exact bytes; if partially untouched, the untouched sub-ranges
//! hold DDR-training leftovers on the real device (pattern unknowable from
//! software) and the alternative window placement is unfalsifiable there.
//!
//! Usage: cargo run --release --example dram_prebound
use truman_emulator::boot;
use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig};
use truman_emulator::memory::AccessSize;

const REGION: u32 = 0x0150_0000; // below-base region start (64 KiB span)
const REGION_END: u32 = 0x0160_0000; // pixel buffer base
const CANARY: u32 = 0xA5A5_A5A5;
const MAX_STEPS: u64 = 2_500_000;

fn find_firmware() -> Option<std::path::PathBuf> {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("firmware");
    let mut found = None;
    if let Ok(rd) = std::fs::read_dir(&path) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().map(|x| x == "bin").unwrap_or(false)
                && p.file_name()
                    .map(|n| n.to_string_lossy().contains("1506HV"))
                    .unwrap_or(false)
            {
                found = Some(p);
            }
        }
    }
    found
}

fn boot_machine(canary: bool) -> Machine {
    let path = find_firmware().expect("real firmware under firmware/");
    let image = firmware::load(&path).expect("load real firmware");
    let mut machine = Machine::new(MachineConfig::default(), &image);
    if canary {
        let mut a = REGION;
        while a < REGION_END {
            assert!(machine.bus.poke_phys(a, AccessSize::Word, CANARY).is_some());
            a += 4;
        }
    }
    boot::reset_with_diagnostics(&mut machine, false);
    machine
}

fn dump_region(machine: &mut Machine) -> Vec<u8> {
    let mut out = Vec::with_capacity((REGION_END - REGION) as usize);
    let mut a = REGION;
    while a < REGION_END {
        let v = machine
            .bus
            .peek_phys(a, AccessSize::Word)
            .expect("region inside DRAM");
        out.extend_from_slice(&v.to_le_bytes());
        a += 4;
    }
    out
}

/// Ranges of bytes that differ from `reference`, as (start, len).
fn diff_ranges(buf: &[u8], reference: &[u8]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        if buf[i] != reference[i] {
            let start = i;
            while i < buf.len() && buf[i] != reference[i] {
                i += 1;
            }
            ranges.push((start, i - start));
        } else {
            i += 1;
        }
    }
    ranges
}

fn main() {
    // ---- canary pass ----
    let mut m = boot_machine(true);
    let res = m.run(MAX_STEPS);
    eprintln!(
        "canary pass: steps={} stop={:?} pc=0x{:08x}",
        res.steps, res.stop, res.pc
    );
    let console = m.uart_console();
    for marker in ["Trim PASS", "1ST BOOT", "LD eCos", "Go eCos"] {
        eprintln!("  uart contains {:?}: {}", marker, console.contains(marker));
    }
    let canary_dump = dump_region(&mut m);

    // ---- fresh pass ----
    let mut m2 = boot_machine(false);
    let res2 = m2.run(MAX_STEPS);
    eprintln!(
        "fresh pass : steps={} stop={:?} pc=0x{:08x}",
        res2.steps, res2.stop, res2.pc
    );
    let fresh_dump = dump_region(&mut m2);

    // ---- analysis ----
    let zero_ref = vec![0u8; fresh_dump.len()];
    let canary_ref = {
        let mut v = Vec::with_capacity(canary_dump.len());
        for chunk in canary_dump.chunks(4) {
            let _ = chunk;
            v.extend_from_slice(&CANARY.to_le_bytes());
        }
        v
    };

    let boot_written = diff_ranges(&canary_dump, &canary_ref);
    let boot_written_bytes: usize = boot_written.iter().map(|(_, l)| l).sum();
    println!("== Milestone P probe 2: pre-decode DRAM bound (0x01500000..0x01600000) ==");
    println!(
        "canary pass: {} boot-written bytes in {} ranges (of {})",
        boot_written_bytes,
        boot_written.len(),
        canary_dump.len()
    );
    for (start, len) in boot_written.iter().take(12) {
        let addr = REGION + *start as u32;
        let end = start + len;
        let sample = &canary_dump[*start..end.min(start + 16)];
        println!(
            "  0x{addr:08X} len={len:6}  sample={}",
            sample
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
    }
    if boot_written.len() > 12 {
        println!("  ... {} more ranges", boot_written.len() - 12);
    }

    let fresh_nonzero = diff_ranges(&fresh_dump, &zero_ref);
    let fresh_nonzero_bytes: usize = fresh_nonzero.iter().map(|(_, l)| l).sum();
    println!(
        "fresh pass: {} non-zero bytes in {} ranges (emulator pre-decode content)",
        fresh_nonzero_bytes,
        fresh_nonzero.len()
    );

    // Determinism check: the boot-written bytes must be identical between
    // the canary and fresh passes (else the boot reads-before-write and the
    // canary perturbed it).
    let mut mismatches = 0usize;
    for (start, len) in &boot_written {
        if canary_dump[*start..*start + *len] != fresh_dump[*start..*start + *len] {
            mismatches += len;
        }
    }
    println!(
        "determinism: {} mismatching bytes between passes over the boot-written set{}",
        mismatches,
        if mismatches == 0 {
            " (boot never read the region's prior content)"
        } else {
            ""
        }
    );

    let untouched = canary_dump.len() - boot_written_bytes;
    println!(
        "bound: {}/{} bytes below the pixel base are NEVER written by the boot \
         (on the real device these hold DDR-training leftovers; the emulator \
         models them as zeros via the zero-preset window)",
        untouched,
        canary_dump.len()
    );
}
