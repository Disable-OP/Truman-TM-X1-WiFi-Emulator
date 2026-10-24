//! Value-table file seam tests (Milestone AH, §35): the container format,
//! its validation, and the end-to-end wiring — a ladder loaded from its
//! byte encoding must reach the machine's engine decode and change (or
//! pin) the boot's decode output accordingly.

use truman_emulator::firmware;
use truman_emulator::inflate::{
    ValueTables, ValueTablesError, ENGINE_VALUE_TABLES, VALUE_TABLES_ENCODED_LEN,
    VALUE_TABLES_MAX_EXTRA, VALUE_TABLES_VERSION,
};
use truman_emulator::machine::{adler32, Machine, MachineConfig};
use truman_emulator::memory::AccessSize;

const PIXEL_BUF: u32 = 0x0160_0000;
const ENGINE_IMG_WIDTH_REG: u32 = 0x1FFE_3828;
const ENGINE_IMG_HEIGHT_REG: u32 = 0x1FFE_382C;

/// A candidate ladder in the m4 spirit (§17.5/§30.3): every extra-bit
/// width zeroed, the length bases shifted — a legal, non-RFC ladder the
/// offline analyses actually swept.
fn m4_style_ladder() -> ValueTables {
    let mut t = ValueTables::rfc1951();
    for slot in t.lextra.iter_mut() {
        *slot = 0;
    }
    for slot in t.dextra.iter_mut() {
        *slot = 0;
    }
    for (i, slot) in t.lbase.iter_mut().enumerate() {
        *slot = (i as u16) + 1;
    }
    t
}

#[test]
fn rfc_ladder_round_trips_through_the_container() {
    let t = ValueTables::rfc1951();
    let enc = t.to_bytes();
    assert_eq!(enc.len(), VALUE_TABLES_ENCODED_LEN);
    assert_eq!(ValueTables::from_bytes(&enc), Ok(t));
    // Idempotent: encode(parse(encode)) is byte-stable.
    let enc2 = ValueTables::from_bytes(&enc).unwrap().to_bytes();
    assert_eq!(enc, enc2);
}

#[test]
fn m4_style_ladder_round_trips_through_the_container() {
    let t = m4_style_ladder();
    assert_ne!(t, ValueTables::rfc1951(), "the candidate must differ");
    let enc = t.to_bytes();
    assert_eq!(ValueTables::from_bytes(&enc), Ok(t));
}

#[test]
fn rfc_container_encoding_is_pinned() {
    // Format drift guard: the RFC ladder's canonical encoding is pinned
    // byte-for-byte via its Adler-32 plus the structurally meaningful
    // offsets (magic, version, counts, first/last ladder entries).
    let enc = ValueTables::rfc1951().to_bytes();
    assert_eq!(adler32(&enc), 0x01c8_0db5, "RFC container fingerprint");
    assert_eq!(&enc[..8], b"SPHEVTBL");
    assert_eq!(enc[8], VALUE_TABLES_VERSION);
    assert_eq!(enc[9], 0, "reserved byte");
    assert_eq!(u16::from_le_bytes([enc[10], enc[11]]), 29);
    assert_eq!(u16::from_le_bytes([enc[12], enc[13]]), 30);
    // lbase[0] = 3 (RFC length code 257), dbase[0] = 1, dextra[29] = 13
    // (the widest extra field — the source of the container's own bound).
    assert_eq!(u16::from_le_bytes([enc[14], enc[15]]), 3);
    let dbase_off = 14 + 29 * 2 + 29;
    assert_eq!(u16::from_le_bytes([enc[dbase_off], enc[dbase_off + 1]]), 1);
    assert_eq!(enc[VALUE_TABLES_ENCODED_LEN - 1], 13);
}

#[test]
fn rejections_name_what_failed() {
    let good = ValueTables::rfc1951().to_bytes();

    // truncated
    assert_eq!(
        ValueTables::from_bytes(&good[..good.len() - 1]),
        Err(ValueTablesError::Truncated {
            needed: VALUE_TABLES_ENCODED_LEN,
            got: VALUE_TABLES_ENCODED_LEN - 1,
        })
    );
    // empty file
    assert_eq!(
        ValueTables::from_bytes(&[]),
        Err(ValueTablesError::Truncated {
            needed: VALUE_TABLES_ENCODED_LEN,
            got: 0,
        })
    );
    // too long
    let mut long = good.clone();
    long.push(0);
    assert_eq!(
        ValueTables::from_bytes(&long),
        Err(ValueTablesError::Overflow {
            expected: VALUE_TABLES_ENCODED_LEN,
            got: VALUE_TABLES_ENCODED_LEN + 1,
        })
    );
    // bad magic
    let mut bad = good.clone();
    bad[0] = b'X';
    assert_eq!(
        ValueTables::from_bytes(&bad),
        Err(ValueTablesError::BadMagic)
    );
    // wrong version
    let mut ver = good.clone();
    ver[8] = VALUE_TABLES_VERSION + 1;
    assert_eq!(
        ValueTables::from_bytes(&ver),
        Err(ValueTablesError::UnsupportedVersion {
            found: VALUE_TABLES_VERSION + 1,
        })
    );
    // wrong code counts (the RFC 1951 §3.2.5 space is fixed)
    let mut cnt = good.clone();
    cnt[10] = 28; // length-code count low byte
    assert_eq!(
        ValueTables::from_bytes(&cnt),
        Err(ValueTablesError::BadCounts {
            found_len: 28,
            found_dist: 30,
        })
    );
    // extra width above the bound: length ladder and distance ladder both
    let mut wide = good.clone();
    wide[14 + 29 * 2] = VALUE_TABLES_MAX_EXTRA + 1; // lextra[0]
    assert_eq!(
        ValueTables::from_bytes(&wide),
        Err(ValueTablesError::ExtraTooWide {
            table: "length",
            index: 0,
            width: VALUE_TABLES_MAX_EXTRA + 1,
        })
    );
    let mut wide_d = good;
    wide_d[VALUE_TABLES_ENCODED_LEN - 1] = 14; // dextra[29] = 13 is legal, 14 is not
    assert_eq!(
        ValueTables::from_bytes(&wide_d),
        Err(ValueTablesError::ExtraTooWide {
            table: "distance",
            index: 29,
            width: 14,
        })
    );
    // the bound itself is legal (distance code 29 carries 13 extras in RFC)
    let mut exact = ValueTables::rfc1951();
    exact.lextra[28] = VALUE_TABLES_MAX_EXTRA;
    assert_eq!(
        ValueTables::from_bytes(&exact.to_bytes()),
        Ok(exact),
        "a 13-bit extra is the legal maximum"
    );
    // errors display readably (session-log quality)
    let msg = ValueTablesError::BadMagic.to_string();
    assert!(msg.contains("magic"), "display names the problem: {msg}");
}

#[test]
fn the_config_default_is_the_pinned_engine_model() {
    // The machine's default ladder must stay field-identical to the
    // pinned static (this is what keeps every boot pin meaningful after
    // the decode path switched to the config-carried seam).
    assert_eq!(
        MachineConfig::default().value_tables,
        ValueTables::rfc1951()
    );
    assert_eq!(MachineConfig::default().value_tables, ENGINE_VALUE_TABLES);
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

fn boot_window(config: &MachineConfig, img: &firmware::Image) -> (Vec<u8>, usize, usize) {
    let mut m = Machine::new(config.clone(), img);
    m.reset();
    let _ = m.run(2_000_000);
    let w = m
        .bus
        .peek_phys(ENGINE_IMG_WIDTH_REG, AccessSize::Word)
        .unwrap_or(0x400);
    let h = m
        .bus
        .peek_phys(ENGINE_IMG_HEIGHT_REG, AccessSize::Word)
        .unwrap_or(78);
    let total = (w as usize) * (h as usize);
    let mut window = vec![0u8; total];
    assert_eq!(m.bus.copy_out(PIXEL_BUF, &mut window), total);
    (window, w as usize, h as usize)
}

/// The end-to-end seam: the SAME ladder, delivered via the file container
/// instead of the compiled default, must produce the pinned reference
/// decode — the config path is byte-equivalent to the static path.
#[test]
fn file_loaded_rfc_ladder_reproduces_the_pinned_reference() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let img = firmware::load(&path).expect("load real firmware");

    // Round-trip the RFC ladder through the container and boot with it.
    let enc = ValueTables::rfc1951().to_bytes();
    let loaded = ValueTables::from_bytes(&enc).expect("parse own encoding");
    let config = MachineConfig {
        value_tables: loaded,
        ..MachineConfig::default()
    };
    let (window, _, _) = boot_window(&config, &img);
    assert_eq!(
        adler32(&window),
        0xcd8d_03bf,
        "file-seam decode must equal the pinned reference decode (§34.4)"
    );
}

/// The differential half: a NON-default ladder delivered through the same
/// seam must actually change the machine's decode output — proving the
/// config-carried ladder reaches the engine decode end-to-end (the §31.4
/// parametricity, now through MachineConfig).
#[test]
fn file_loaded_candidate_ladder_changes_the_decode() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let img = firmware::load(&path).expect("load real firmware");

    let (reference, _, _) = boot_window(&MachineConfig::default(), &img);
    let candidate = m4_style_ladder();
    let config = MachineConfig {
        value_tables: candidate.clone(),
        ..MachineConfig::default()
    };
    let (window, _, _) = boot_window(&config, &img);
    assert_ne!(
        window, reference,
        "a changed ladder must change the engine decode output"
    );
    // and the candidate is reproducible: a second identical boot matches.
    let (again, _, _) = boot_window(&config, &img);
    assert_eq!(window, again, "candidate-ladder boot must be deterministic");
    // the candidate's fingerprint is its identity for the P3 elimination.
    eprintln!("candidate adler-32: 0x{:08x}", adler32(&window));
}
