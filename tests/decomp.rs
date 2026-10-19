//! Integration tests: the hardware decompression engine against the real
//! firmware's compressed payload.
//!
//! Evidence basis: docs/firmware-analysis.md §13 (register program), §13.3
//! (the driver's success path: the poll's bit-5 branch acks with 0x2000,
//! counts the event against the limit mirrored from the container header's
//! u16@2, and routes to the block that returns 0x13800 — non-zero = success;
//! the bit-6 branch is the failure epilogue). The engine decode semantics
//! modeled here: dynamic-block tables re-derived from the stream header
//! (bit-for-bit equal to the uploaded tables), symbols from the programmed
//! start bit (R3818 = 1088 container bits = 1024 stream bits for this
//! container; the 16-bit gap after the 1008-bit header is zero padding,
//! dump-verified) and decode to the end of the input length register, with
//! end-of-block symbols as markers (the driver expects the full declared
//! stream). The exact LZ value semantics of the hardware engine are still
//! being pinned down — see §13.4 for the open items.

use truman_emulator::firmware;
use truman_emulator::inflate::{inflate, inflate_engine};

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

/// The first deflate block of the real payload stream decodes to a stable
/// size under standard RFC 1951 semantics (zlib cross-checked during the
/// reverse-engineering session; pinned as a regression guard).
#[test]
fn real_sp_container_first_block_inflates_to_stable_size() {
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

/// The engine-programmed decode of the real container: symbols from the
/// programmed start bit (R3818 = 1088 container bits = 1024 stream bits)
/// through the end of the input length register (0x8692 bytes from the
/// source; the stream here is the container minus the 8-byte header, so
/// the emulator reads 0x868A stream bytes), end-of-block ignored. The
/// output size is pinned as a regression guard for the decode path the
/// machine service loop performs.
#[test]
fn real_sp_container_engine_decode_is_deterministic() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let image = firmware::load(&path).expect("load real firmware");
    let base = 0x1002Cusize;
    let container = &image.bytes[base..base + 0x8692];
    let stream = &container[8..];
    assert_eq!(&container[0..2], b"SP", "container magic");

    let mut out = Vec::new();
    let n = inflate_engine(stream, 1024, 0x8000, &mut out, 0x4_0000)
        .expect("engine decode must run to input end");
    // 75,398 bytes from the declared 0x8684-byte stream; the emulator feeds
    // the stream up to the engine's length register (0x8692 from the
    // source = 0x868A stream bytes), and the 6 zero tail bytes decode to
    // 259 more bytes before input exhaustion — pinned as observed.
    assert_eq!(n, 75_657, "engine decode output size (start bit 1024)");
}

/// The machine-level handshake: running the real firmware boots through
/// "LD eCos v1.16 1506f", the engine completes, the wait loop takes the
/// bit-5 event branch, and the loader's success path prints "Go eCos" and
/// "Boot from SPI" and jumps to the decompressed eCos entry at 0xA1600000.
#[test]
fn real_firmware_engine_handshake_boots_to_ecos_entry() {
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
    let result = machine.run(1_500_000);
    let console = machine.uart_console();
    assert!(console.contains("LD eCos v1.16 1506f"));
    assert!(
        console.contains("Go eCos") && console.contains("Boot from SPI"),
        "loader success path must run; console:\n{console}"
    );
    assert!(
        !console.contains("LD eCos fail"),
        "failure path must not be taken anymore"
    );
    // The final PC must be inside the decompressed eCos image page
    // (0xA1600000 kseg1 = phys 0x01600000).
    assert_eq!(
        result.pc & 0xFFF0_0000,
        0xA160_0000,
        "boot must reach the eCos entry page, got pc={:#x}",
        result.pc
    );
    // Engine state: one completed decode.
    let sh = machine.decomp.borrow();
    assert!(
        matches!(sh.last_result, Some(Ok(n)) if n > 70_000),
        "engine completed one full decode, got {:?}",
        sh.last_result
    );
    assert!(!sh.busy);
    let _ = result; // step count asserted in tests/boot.rs
}
