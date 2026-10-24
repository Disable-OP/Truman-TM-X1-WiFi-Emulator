//! Hardware-session tests (Milestone Y): the pure comparison/residue
//! analysis behind docs/hardware-session.md, plus a real-firmware gated
//! test that proves the reference extraction feeds the comparator.

use truman_emulator::firmware;
use truman_emulator::machine::{adler32, classify_residue, compare_dumps, Machine, MachineConfig};

#[test]
fn adler32_matches_the_standard_zlib_vectors() {
    // RFC 1950 Adler-32, pinned against zlib's own published examples:
    // empty → 1, "abc" → 0x024d0127, "Wikipedia" → 0x11E60398.
    assert_eq!(adler32(b""), 0x0000_0001);
    assert_eq!(adler32(b"abc"), 0x024d_0127);
    assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    // Determinism and 16-bit-halves decomposition (the vendor probe
    // protocol reads the halves separately: addr 0 = high, 1 = low).
    let data: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
    let v = adler32(&data);
    assert_eq!(v, adler32(&data));
    let hi = (v >> 16) as u16;
    let lo = v as u16;
    assert_eq!(((hi as u32) << 16) | lo as u32, v);
}

#[test]
fn identical_camps_report_full_match_and_no_divergence() {
    let r = vec![0xA5u8; 4096];
    let c = compare_dumps(&r, &r.clone());
    assert_eq!(c.reference_len, 4096);
    assert_eq!(c.captured_len, 4096);
    assert_eq!(c.compared_len, 4096);
    assert_eq!(c.matching_bytes, 4096);
    assert_eq!(c.match_permille, 1000);
    assert_eq!(c.first_divergence, None);
    assert!(c.divergent_blocks.is_empty());
    assert_eq!(c.longest_match_run, 4096);
}

#[test]
fn single_byte_perturbation_is_localized_exactly() {
    let mut r = vec![0u8; 16 * 1024];
    for (i, b) in r.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
    let mut c = r.clone();
    c[12345] ^= 0xFF;
    let cmp = compare_dumps(&r, &c);
    assert_eq!(cmp.first_divergence, Some(12345));
    assert_eq!(cmp.divergent_blocks, vec![12]);
    assert_eq!(
        cmp.match_permille,
        (16 * 1024 - 1) as u64 * 1000 / (16 * 1024)
    );
    // longest match run: bytes 0..12345 all match
    assert_eq!(cmp.longest_match_run, 12345);
}

#[test]
fn divergent_blocks_are_deduplicated_across_runs() {
    let r = vec![0u8; 4 * 1024];
    let mut c = vec![0u8; 4 * 1024];
    // three separated mismatches inside block 1, one in block 2
    for &off in &[1100u32, 1500, 1900, 3000] {
        c[off as usize] = 0xFF;
    }
    let cmp = compare_dumps(&r, &c);
    assert_eq!(cmp.divergent_blocks, vec![1, 2]);
    assert_eq!(cmp.first_divergence, Some(1100));
}

#[test]
fn size_mismatch_compares_the_shorter_prefix_and_records_lengths() {
    let r = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
    let c = vec![1u8, 2, 3, 9];
    let cmp = compare_dumps(&r, &c);
    assert_eq!(cmp.compared_len, 4);
    assert_eq!(cmp.first_divergence, Some(3));
    assert_eq!(cmp.match_permille, 750);

    // longer capture: extra bytes are reported, not compared
    let c2 = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 0xAA, 0xBB];
    let cmp2 = compare_dumps(&r, &c2);
    assert_eq!(cmp2.compared_len, 8);
    assert_eq!(cmp2.first_divergence, None);
    assert_eq!(cmp2.captured_len, 10);
    assert_eq!(cmp2.reference_len, 8);
}

#[test]
fn empty_overlap_is_reported_not_panicked() {
    let r = vec![1u8, 2, 3];
    let c: Vec<u8> = Vec::new();
    let cmp = compare_dumps(&r, &c);
    assert_eq!(cmp.compared_len, 0);
    assert_eq!(cmp.match_permille, 0);
    assert_eq!(cmp.first_divergence, None);
    assert_eq!(cmp.longest_match_run, 0);
}

#[test]
fn comparison_is_deterministic() {
    let mut r = vec![0u8; 8192];
    let mut c = vec![0u8; 8192];
    for i in 0..8192 {
        r[i] = (i * 7 % 256) as u8;
        c[i] = if i % 997 == 0 { 0xFF } else { r[i] };
    }
    let a = compare_dumps(&r, &c);
    let b = compare_dumps(&r, &c);
    assert_eq!(a, b);
}

#[test]
fn all_zero_capture_classifies_as_no_residue() {
    let r = classify_residue(&vec![0u8; 8192]);
    assert_eq!(r.nonzero_bytes, 0);
    assert_eq!(r.nonzero_permille, 0);
    assert_eq!(r.first_nonzero, None);
    assert!(r.nonzero_blocks.is_empty());
    assert_eq!(r.longest_nonzero_run, 0);
}

#[test]
fn residue_runs_are_structured_by_block_and_length() {
    let mut c = vec![0u8; 3 * 1024];
    // run A: bytes 2000..2032 (block 1), run B: byte 2500 (block 2)
    for b in c.iter_mut().take(2032).skip(2000) {
        *b = 0x5A;
    }
    c[2500] = 0x11;
    let r = classify_residue(&c);
    assert_eq!(r.nonzero_bytes, 33);
    assert_eq!(r.first_nonzero, Some(2000));
    assert_eq!(r.longest_nonzero_run, 32);
    assert_eq!(r.nonzero_blocks, vec![1, 2]);
    assert_eq!(r.nonzero_permille, 33 * 1000 / (3 * 1024));
}

#[test]
fn residue_blocks_are_deduplicated_within_one_block() {
    let mut c = vec![0u8; 2 * 1024];
    for &off in &[10u32, 500, 1000] {
        c[off as usize] = 1;
    }
    let r = classify_residue(&c);
    assert_eq!(r.nonzero_blocks, vec![0]);
}

/// Locate the committed real firmware (if present), per the repo-wide
/// smoke-test convention (tests/boot.rs).
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
fn real_firmware_reference_feeds_the_comparator() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let img = firmware::load(&path).expect("load real firmware");

    let mut m = Machine::new(MachineConfig::default(), &img);
    m.reset();
    let res = m.run(2_000_000);
    let w = m
        .bus
        .peek_phys(0x1FFE_3828, truman_emulator::memory::AccessSize::Word)
        .unwrap_or(0x400);
    let h = m
        .bus
        .peek_phys(0x1FFE_382C, truman_emulator::memory::AccessSize::Word)
        .unwrap_or(78);
    let total = (w as usize) * (h as usize);
    let mut reference = vec![0u8; total];
    assert_eq!(m.bus.copy_out(0x0160_0000, &mut reference), total);

    // identical capture → identical verdict
    let same = compare_dumps(&reference, &reference.clone());
    assert_eq!(same.first_divergence, None);
    assert_eq!(same.match_permille, 1000);

    // the §19.1 head bytes: the reference's first word is what the loader
    // jumps into — the comparator must localize any perturbation exactly.
    let mut perturbed = reference.clone();
    perturbed[12345] ^= 0xFF;
    let cmp = compare_dumps(&reference, &perturbed);
    assert_eq!(cmp.first_divergence, Some(12345));
    assert_eq!(cmp.divergent_blocks, vec![12]);

    // a second boot must produce a byte-identical reference (determinism
    // under the comparator's extraction path, docs/architecture.md).
    let mut m2 = Machine::new(MachineConfig::default(), &img);
    m2.reset();
    let _ = m2.run(2_000_000);
    let mut reference2 = vec![0u8; total];
    m2.bus.copy_out(0x0160_0000, &mut reference2);
    let again = compare_dumps(&reference, &reference2);
    assert_eq!(again.first_divergence, None, "boot must be deterministic");
    assert_eq!(again.match_permille, 1000);

    // the boot's stop reason is the park after the engine handoff; the
    // report should record it for the session log.
    eprintln!(
        "boot: stop={:?} steps={} reference={} bytes",
        res.stop, res.steps, total
    );

    // Milestone AG (§34.2/P3): the emulator-side Adler-32 fingerprint of
    // the reference decode — the value a P3 probe-window read must match
    // for the emulator's decode to be the device's. Pinned here so a
    // future engine-semantics change that silently moves the decode
    // output is caught by this suite, not by a failed capture verdict.
    let fingerprint = adler32(&reference);
    assert_eq!(fingerprint, 0xcd8d_03bf, "reference decode fingerprint");
    eprintln!("reference adler-32: {fingerprint:#010x}");
}
