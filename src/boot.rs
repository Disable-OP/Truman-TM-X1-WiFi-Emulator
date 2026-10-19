//! Boot reset state and diagnostics.

use crate::machine::Machine;

/// Reset entry point under hypothesis A (flash XIP at the MIPS boot window).
/// See docs/firmware-analysis.md §8 Q5.
pub const RESET_PC: u32 = 0xBFC0_0000;

/// Initial stack pointer the firmware itself programs
/// (`lui $sp, 0xBFFC ; ori $sp, 0x0E00`) — used only as a cross-check
/// diagnostic, the firmware sets it during boot.
pub const FIRMWARE_STACK_TOP: u32 = 0xBFFC_0E00;

/// Reset the machine and emit a boot diagnostics report (text lines).
pub fn reset_with_diagnostics(machine: &mut Machine, verbose: bool) -> Vec<String> {
    machine.reset();
    let mut out = Vec::new();
    out.push(format!(
        "boot: reset pc=0x{RESET_PC:08x} (hypothesis A: flash XIP at MIPS boot window)"
    ));
    out.push(format!(
        "boot: firmware-provided stack top will be 0x{FIRMWARE_STACK_TOP:08x}"
    ));
    if verbose {
        out.push("boot: physical region map:".to_string());
        for (name, base, len, kind) in machine.region_map() {
            out.push(format!(
                "  {name:<16} phys 0x{base:08x}..0x{:08x} ({len:>9} bytes, {kind})",
                base + len - 1
            ));
        }
        if let Some(mips) = machine
            .cpu
            .as_any()
            .downcast_ref::<crate::cpu::mips32::Mips32>()
        {
            let c0 = mips.cop0();
            out.push(format!(
                "boot: COP0 Status=0x{:08x} Config=0x{:08x} Config1=0x{:08x} PRId=0x{:08x}",
                c0.status(),
                c0.config(),
                c0.config1(),
                c0.prid()
            ));
            out.push(
                "boot: Config1 cache geometry: I$ 16KiB 4-way 32B, D$ 16KiB 4-way 32B (synthetic, self-consistent)"
                    .to_string(),
            );
        }
    }
    out
}
