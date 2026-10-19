//! Integration tests: the hardware decompression engine (hypothesis H12)
//! against the real firmware's compressed payload.
//!
//! Evidence basis: docs/firmware-analysis.md §13. The "LD eCos" stage of the
//! main firmware (decompressed to 0xA2000000) reads the "SP" container at
//! flash file offset 0x1002C into DRAM (0xA2088000), uploads the Huffman
//! tables it parsed to the engine at 0xBFFE3800, starts the engine and
//! waits for bit 6 of 0xBFFE384C. The container is a raw DEFLATE stream:
//! the header parse the firmware performs (HLIT/HDIST/HCLEN + code lengths)
//! produces exactly the tables it uploads, which pins the format.

use truman_emulator::firmware;
use truman_emulator::inflate::inflate;

/// Locate the committed real firmware (if present).
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

/// The engine's decode of the real payload stream must terminate cleanly at
/// the stream's final block with a stable output size (zlib cross-checked
/// during the reverse-engineering session; the value is pinned here as a
/// regression guard for the pure-Rust decoder).
#[test]
fn real_sp_container_inflates_to_stable_size() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let image = firmware::load(&path).expect("load real firmware");

    // The loader copies 0x868C bytes from file offset 0x1002C into DRAM and
    // the engine inflates from image+8 (the firmware parses the 8-byte
    // "SP" header itself).
    let base = 0x1002Cusize;
    let container = &image.bytes[base..base + 0x868C];
    assert_eq!(&container[0..2], b"SP", "container magic");
    let be_len = u32::from_be_bytes([container[4], container[5], container[6], container[7]]);
    assert_eq!(be_len, 0x8684, "declared stream length");

    let stream = &container[8..];
    let mut out = Vec::new();
    let n = inflate(stream, 0x8000, &mut out, 0x8_0000).expect("inflate must succeed");
    assert_eq!(n, 28_256, "first deflate block output size");
}

/// The machine-level handshake: running the real firmware boots through
/// "LD eCos v1.16 1506f", the engine starts (0xBFFE3824 bit 0), completes
/// (0xBFFE384C bit 6 set), and the loader's wait loop exits — observed as
/// the loader reaching its result check and printing "LD eCos fail" (the
/// per-symbol continuation protocol is still open; see firmware-analysis
/// §13.4).
#[test]
fn real_firmware_engine_handshake_reaches_loader_result_check() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let image = firmware::load(&path).expect("load real firmware");
    let mut machine = truman_emulator::machine::Machine::new(
        truman_emulator::machine::MachineConfig::default(),
        &image,
    );
    truman_emulator::boot::reset_with_diagnostics(&mut machine, false);
    let result = machine.run(2_000_000);
    let console = machine.uart_console();
    assert!(console.contains("LD eCos v1.16 1506f"));
    assert!(console.contains("LD eCos fail"));
    // Engine state: one completed decode.
    let sh = machine.decomp.borrow();
    assert_eq!(
        sh.last_result,
        Some(Ok(28_256)),
        "engine completed exactly one decode"
    );
    assert!(!sh.busy);
    assert_eq!(sh.done.get(), 1);
    let _ = result; // step count asserted in tests/boot.rs
}
