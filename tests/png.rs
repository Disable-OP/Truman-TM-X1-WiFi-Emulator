//! Milestone N (§17): the "decompression engine" at 0xBFFE3800 is a PNG
//! decoder. The SPHE15xx platform register header (regfile.h of the
//! linux-sunplus-15xx tree) names the whole block PNG_DEC_*; every value
//! the real firmware programs fits the PNG semantics:
//!
//!   0xBFFE3828 PNG_IMG_WIDTH  = 0x400 (driver constant 1024)
//!   0xBFFE382C PNG_IMG_HEIGHT = container u16@2 = 78
//!   0xBFFE3830 PNG_IMG_TYPE   = 0x68 (driver constant)
//!   0xBFFE383C PNG_BLK_INFO   = (btype<<1)|bfinal = 5
//!   0xBFFE3874 PNG_PXL_LINE   = row countdown preload (77)
//!
//! The pixel output (PNG_PXL_EXT_ADDR = 0x01600000) is the boot's jump
//! target: the 1024x78 8-bpp image IS the loaded eCos image. Completion
//! accounting (PNG_BS_USED_CNT x 128) equals the pixel count; the real
//! device's post-boot DRAM stores 624 x 128 = 79,872 = 78 x 1024 (§13.6),
//! which the emulator's engine now reports as well.

use truman_emulator::boot;
use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig};

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
fn engine_is_programmed_as_png_decoder_and_pads_rows() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let image = firmware::load(&path).expect("load real firmware");
    let mut machine = Machine::new(MachineConfig::default(), &image);
    boot::reset_with_diagnostics(&mut machine, false);

    // Through "LD eCos", the engine handshake, decode and completion
    // (the boot test pins the same window deterministically).
    let result = machine.run(1_320_000);
    eprintln!(
        "png-engine: {} steps, stop: {:?}, pc=0x{:08x}",
        result.steps, result.stop, result.pc
    );

    // The firmware's engine program (recorded in the register file).
    assert_eq!(
        machine.decomp_reg(0x28),
        0x400,
        "PNG_IMG_WIDTH must be the driver's 0x400 constant"
    );
    assert_eq!(
        machine.decomp_reg(0x2C),
        78,
        "PNG_IMG_HEIGHT must be the container's u16@2"
    );
    assert_eq!(
        machine.decomp_reg(0x30),
        0x68,
        "PNG_IMG_TYPE must be the driver's 0x68 constant"
    );
    assert_eq!(
        machine.decomp_reg(0x3C),
        5,
        "PNG_BLK_INFO = (btype<<1)|bfinal for the SP container's header"
    );
    assert_eq!(
        machine.decomp_reg(0x74),
        77,
        "PNG_PXL_LINE = row countdown preload (limit-1)"
    );

    // Completion: the engine reports the full pixel image (row padding
    // from the zero-filled band buffer when the bitstream ends early).
    assert_eq!(
        machine.decomp_last_result(),
        Some(Ok(79_872)),
        "engine must complete 78x1024 pixel bytes"
    );
    assert_eq!(
        machine.decomp_reg(0x1C),
        624,
        "PNG_BS_USED_CNT = 78 rows x 8 units of 128 px"
    );

    // The boot must be past the engine handshake (into the eCos page).
    // The eCos image CONTENT still decodes with the engine's open LZ
    // value semantics (§17), so the stage still stops on its first
    // bad instruction at 0xA1600050 — the engine-side facts above are
    // what this test pins.
    assert_eq!(
        result.pc & 0xFFF0_0000,
        0xA160_0000,
        "boot must reach the eCos entry page (0xA1600000)"
    );
}
