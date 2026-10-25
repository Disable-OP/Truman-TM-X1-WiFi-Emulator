//! Hardware-session tests (Milestone Y): the pure comparison/residue
//! analysis behind docs/hardware-session.md, plus a real-firmware gated
//! test that proves the reference extraction feeds the comparator.

use truman_emulator::firmware;
use truman_emulator::machine::{
    adler32, adler32_probe_halves, classify_residue, compare_dumps, Machine, MachineConfig,
    ENGINE_MAX_OUT, ENGINE_PRESET_BYTES,
};

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
fn probe_halves_pin_the_342_readout_order_and_round_trip() {
    // Milestone AP: the §34.2/P3 half order in code. The vendor drivers'
    // end-of-image branch (and the Truman loader's handler, §12) reads
    // the decode's Adler-32 as two 16-bit probe values — probe address 0
    // returns the HIGH half, probe address 1 the LOW half. The RFC
    // reference decode's pinned fingerprint therefore reads out as
    // (0xcd8d, 0x03bf) — the pair the session's two `mdw 0xBFFE387C`
    // operations should produce when the emulator's decode IS the
    // device's decode.
    assert_eq!(adler32_probe_halves(0xcd8d_03bf), (0xcd8d, 0x03bf));
    // The zlib test vectors split the same way, high-first.
    assert_eq!(adler32_probe_halves(0x024d_0127), (0x024d, 0x0127));
    assert_eq!(adler32_probe_halves(0x11e6_0398), (0x11e6, 0x0398));
    assert_eq!(adler32_probe_halves(0x0000_0001), (0x0000, 0x0001));
    // Round trip: the inverse assembly is the `--fingerprint` operand —
    // for every adler the pair reassembles to it, and for every slice
    // the assembled pair of the computed adler equals the computed adler.
    let data: Vec<u8> = (0..=255u8).cycle().take(79_872).collect();
    let v = adler32(&data);
    let (hi, lo) = adler32_probe_halves(v);
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

/// Milestone AQ (§44): the band-window preset seam, machine-level.
///
/// The §13/§18.3 model zero-fills the 32 KiB band window before the
/// decode; far-back matches (458 sites, 3.98% of block-1 per §18.3
/// consequence 4) copy from it. The seam makes the window CONTENT a
/// config value, so the §21.6 item-2 experiment is a file, not a
/// recompile. Pinned here:
/// * an all-zero preset file is byte-transparent — the boot (stop,
///   steps), the decode length and the reference fingerprint are the
///   default boot's;
/// * an 0xA5 preset changes exactly the far-back copies (the §19.1
///   head's preset match brings 0xA5A5 instead of 0x0000), leaves the
///   decode length and the literal bytes untouched, and is
///   deterministic across boots;
/// * `EngineDecodeInputs` carries the preset content and the §36
///   replay under `inflate_engine_with_traced_preset` is byte-exact.
#[test]
fn band_window_preset_seam_is_transparent_at_zero_and_effective_under_a5() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let img = firmware::load(&path).expect("load real firmware");

    // The default boot: reference stop, fingerprint 0xcd8d03bf, decode
    // length 75_408 (all pinned by the existing suite).
    let mut def = Machine::new(MachineConfig::default(), &img);
    def.reset();
    let def_res = def.run(2_000_000);

    // (a) An all-zero preset FILE: the seam is transparent.
    let cfg0 = MachineConfig {
        preset_window: Some(vec![0u8; ENGINE_PRESET_BYTES]),
        ..Default::default()
    };
    let mut m0 = Machine::new(cfg0, &img);
    m0.reset();
    let res0 = m0.run(2_000_000);
    assert_eq!(res0.stop, def_res.stop, "zero preset: same stop");
    assert_eq!(res0.steps, def_res.steps, "zero preset: same steps");
    let (total, ref0) = window_of(&mut m0);
    let mut win_def = vec![0u8; total];
    def.bus.copy_out(0x0160_0000, &mut win_def);
    assert_eq!(adler32(&ref0), 0xcd8d_03bf, "zero preset: same fingerprint");
    assert_eq!(ref0, win_def, "zero preset: byte-identical window");

    // (b) An 0xA5 preset: the far-back copies change, the rest does not.
    let cfg5 = MachineConfig {
        preset_window: Some(vec![0xA5u8; ENGINE_PRESET_BYTES]),
        ..Default::default()
    };
    let mut m5 = Machine::new(cfg5.clone(), &img);
    m5.reset();
    let _ = m5.run(2_000_000);
    let (_, ref5) = window_of(&mut m5);
    let inputs = m5.engine_decode_inputs().expect("engine ran");
    assert_eq!(
        inputs.decoded_len, 75_408,
        "the symbol sequence is preset-independent (same decode length)"
    );
    assert_eq!(
        &ref5[..2],
        &[0x60, 0x81],
        "the two head literals are preset-independent"
    );
    assert_eq!(
        &ref5[2..4],
        &[0xA5, 0xA5],
        "the §19.1 head's preset match now copies 0xA5"
    );
    assert_ne!(
        adler32(&ref5),
        0xcd8d_03bf,
        "the A5 window is not the zero-window decode"
    );

    // Determinism: a second A5 boot is byte-identical.
    let mut m5b = Machine::new(cfg5.clone(), &img);
    m5b.reset();
    let _ = m5b.run(2_000_000);
    let (_, ref5b) = window_of(&mut m5b);
    assert_eq!(ref5, ref5b, "the A5-preset boot is deterministic");

    // (c) The §36 replay under the seam: the carried preset replays
    // byte-exact; the zero-preset replay must NOT match (it would
    // silently reproduce the default decode).
    let mut replay = Vec::new();
    let n = truman_emulator::inflate::inflate_engine_with_traced_preset(
        &inputs.stream,
        inputs.start_bit,
        &inputs.preset_window,
        &mut replay,
        ENGINE_MAX_OUT,
        &cfg5.value_tables,
        None,
    )
    .expect("A5 replay succeeds");
    assert_eq!(n, inputs.decoded_len, "replay length == boot decode length");
    assert_eq!(
        &replay[..],
        &ref5[..inputs.decoded_len],
        "replay under the carried preset is byte-identical to the boot"
    );
    assert_eq!(inputs.preset_window.len(), ENGINE_PRESET_BYTES);
    assert!(inputs.preset_window.iter().all(|&b| b == 0xA5));
    let mut replay0 = Vec::new();
    let n0 = truman_emulator::inflate::inflate_engine_with_traced(
        &inputs.stream,
        inputs.start_bit,
        ENGINE_PRESET_BYTES,
        &mut replay0,
        ENGINE_MAX_OUT,
        &cfg5.value_tables,
        None,
    )
    .expect("zero replay succeeds");
    assert_eq!(n0, n, "same length under either preset");
    assert_ne!(
        &replay0[..],
        &ref5[..inputs.decoded_len],
        "the zero-preset replay differs (the seam is effective)"
    );
}

/// The engine pixel window (W×H from the engine's registers) of a
/// booted machine, per the dram_compare --post extraction.
fn window_of(m: &mut Machine) -> (usize, Vec<u8>) {
    let w = m
        .bus
        .peek_phys(0x1FFE_3828, truman_emulator::memory::AccessSize::Word)
        .unwrap_or(0x400);
    let h = m
        .bus
        .peek_phys(0x1FFE_382C, truman_emulator::memory::AccessSize::Word)
        .unwrap_or(78);
    let total = (w as usize) * (h as usize);
    let mut buf = vec![0u8; total];
    assert_eq!(m.bus.copy_out(0x0160_0000, &mut buf), total);
    (total, buf)
}

/// Milestone AR (§45): the P2 residue re-boot experiment, machine-level.
///
/// The decision tree's P2 RESIDUE PRESENT row ("load the residue via
/// `AddressSpace::copy_in` before the engine start in a fresh emulator
/// boot and re-run the decode compare") was library-only prose;
/// `dram_compare --pre --reboot-residue` is now its one-command form.
/// These tests pin the model statements the command reports, on the
/// real firmware, per window class:
/// * residue BELOW the pixel base (the P2 wide window's lower part,
///   outside the band preset and the output window) is inert — the
///   model's decode does not read the never-written region (the
///   read-side complement of §18.3), and the boot pins hold under the
///   injection (handoff step, fingerprint);
/// * residue INSIDE the output window is fully overwritten by the
///   engine's row semantics (§17: every programmed row is completed —
///   decode output plus the zero-filled band tail), so the P2 primary
///   window cannot hold surviving residue;
/// * the driver's own chunk-size formula leaves a 6-byte input overhang
///   past the loader's container copy — the one genuine bus-side
///   pre-decode channel — and it is ACTIVE: the decode's final 10
///   bytes are a function of those 6 bytes.
#[test]
fn residue_below_the_pixel_base_is_inert_and_the_boot_pins_hold() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let img = firmware::load(&path).expect("load real firmware");

    // Clean boot: the pinned reference (0xcd8d03bf, §34-era pin).
    let mut a = Machine::new(MachineConfig::default(), &img);
    a.reset();
    let res_a = a.run(2_000_000);
    let (_, ref_a) = window_of(&mut a);

    // Residue boot: 4096 bytes of 0xA5 one MiB below the pixel base —
    // inside the §21.6 wide P2 window (the 1 MiB below 0x01600000),
    // outside the band preset [0x015F8000,0x01600000) and the output
    // window, and far from the container staging (0x02088000).
    let mut b = Machine::new(MachineConfig::default(), &img);
    b.reset();
    assert_eq!(b.bus.copy_in(0x0150_0000, &[0xA5u8; 4096]), 4096);
    let res_b = b.run(2_000_000);
    let (_, ref_b) = window_of(&mut b);

    assert_eq!(ref_a, ref_b, "below-base residue cannot reach the decode");
    assert_eq!(
        adler32(&ref_b),
        0xcd8d_03bf,
        "fingerprint holds under residue"
    );
    assert_eq!(
        res_b.steps, res_a.steps,
        "the handoff pin holds (1,301,783)"
    );
    assert_eq!(res_a.steps, 1_301_783);
}

#[test]
fn residue_inside_the_output_window_is_overwritten_by_the_row_semantics() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let img = firmware::load(&path).expect("load real firmware");

    let mut a = Machine::new(MachineConfig::default(), &img);
    a.reset();
    let _ = a.run(2_000_000);
    let (total, ref_a) = window_of(&mut a);

    // Residue boot: the WHOLE output window (the engine's programmed
    // W×H = 79,872) is 0xA5 before the boot runs. §17's row semantics
    // write every programmed row — the decode output plus the
    // zero-filled band tail — so nothing survives.
    let mut b = Machine::new(MachineConfig::default(), &img);
    b.reset();
    assert_eq!(b.bus.copy_in(0x0160_0000, &vec![0xA5u8; total]), total);
    let _ = b.run(2_000_000);
    let (_, ref_b) = window_of(&mut b);

    assert_eq!(
        ref_a, ref_b,
        "the row semantics overwrite the full programmed window"
    );
    assert_eq!(adler32(&ref_b), 0xcd8d_03bf);
    // The zero-filled band tail of the row semantics is visible in the
    // clean window: everything past the decode's own output length is
    // zeros (the machine's PNG-row resize, §17).
    let decoded_len = a.engine_decode_inputs().expect("engine ran").decoded_len;
    assert!(ref_a[decoded_len..].iter().all(|&b| b == 0));
}

#[test]
fn the_container_tail_overhang_is_the_one_active_residue_channel() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let img = firmware::load(&path).expect("load real firmware");

    // Clean boot, plus the driver's engine program pinned from the
    // registers themselves (external, verifiable): the bitstream base
    // is the copied container (0x02088000) and the chunk size the
    // driver programs (stream_len + 8 + (len&1) + 6, §17) reads 6
    // bytes PAST the loader's copy (0x868C bytes, §13.1).
    let mut a = Machine::new(MachineConfig::default(), &img);
    a.reset();
    let _ = a.run(2_000_000);
    let (total, ref_a) = window_of(&mut a);
    let src = ((a.decomp_reg(0x04) << 16) | a.decomp_reg(0x08)) & 0x1FFF_FFFF;
    let ilen = a.decomp_reg(0x38) as usize;
    assert_eq!(src, 0x0208_8000, "the bitstream base is the staging copy");
    assert_eq!(ilen, 0x8692, "the driver's chunk-size program");
    let container_len = 0x868Cusize;
    let overhang = (src as usize + ilen) - (src as usize + container_len);
    assert_eq!(
        overhang, 6,
        "the input window overhangs the copy by 6 bytes"
    );

    // Residue boot: 0xA5 in exactly the overhang bytes — content the
    // loader never writes and the driver's zero-fill does not cover
    // (§13.1 fills the band window and scratch only). The engine reads
    // them as stream tail, and the decode's final bytes move.
    let mut b = Machine::new(MachineConfig::default(), &img);
    b.reset();
    let tail_at = src as usize + container_len;
    assert_eq!(b.bus.copy_in(tail_at as u32, &[0xA5u8; 6]), 6);
    let _ = b.run(2_000_000);
    let (_, ref_b) = window_of(&mut b);

    assert_ne!(ref_a, ref_b, "the overhang channel is ACTIVE");
    assert_ne!(adler32(&ref_b), 0xcd8d_03bf);
    // The sensitivity is exactly the decode tail: block-1's content
    // (0..75,398, §17.2) is untouched; the final bytes move with the
    // tail input. Deterministic across a second injected boot.
    let c = compare_dumps(&ref_a, &ref_b);
    assert_eq!(c.first_divergence, Some(75_398));
    assert_eq!(total - c.matching_bytes, 10);
    let mut b2 = Machine::new(MachineConfig::default(), &img);
    b2.reset();
    assert_eq!(b2.bus.copy_in(tail_at as u32, &[0xA5u8; 6]), 6);
    let _ = b2.run(2_000_000);
    let (_, ref_b2) = window_of(&mut b2);
    assert_eq!(ref_b, ref_b2, "the tail channel is deterministic");
}
