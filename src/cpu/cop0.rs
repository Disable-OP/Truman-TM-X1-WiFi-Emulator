//! Minimal COP0 (System Coprocessor 0) model for the SPHE15xx boot path.
//!
//! The register set covers exactly what the firmware's loader touches
//! (see docs/firmware-analysis.md §5) plus standard reset state:
//! Status, Config, Config1, PRId, EBase, Count, Compare, Cause, EPC,
//! ErrorEPC, TagLo/TagHi (sel 0 and sel 2), WatchLo/WatchHi, DEVID(22/23).
//!
//! Values that influence observable behavior (cache geometry etc.) are
//! chosen to be internally consistent; they are hypotheses where the real
//! SoC values are unknown and are logged at reset.

/// COP0 register file with per-selector storage.
#[derive(Debug, Clone)]
pub struct Cop0 {
    /// Indexed by [selector][register].
    regs: [[u32; 32]; 8],
}

/// COP0 register numbers used by the boot code.
pub mod regs {
    pub const COUNT: u8 = 9;
    pub const STATUS: u8 = 12;
    pub const CAUSE: u8 = 13;
    pub const EPC: u8 = 14;
    pub const PRID: u8 = 15;
    pub const CONFIG: u8 = 16;
    pub const WATCHLO: u8 = 18;
    pub const WATCHHI: u8 = 19;
    pub const TAGLO: u8 = 28;
    pub const TAGHI: u8 = 29;
    pub const ERROREPC: u8 = 30;
}

impl Cop0 {
    /// Reset-time state.
    ///
    /// - `Status = 0x0040_0004`: BEV=1, ERL=1 (standard MIPS reset), NMI=0.
    /// - `Config`: M=1 (Config1 present), AR=1 (Release 2), MT=1 (TLB),
    ///   BE=0 (little-endian), K0=2 (cacheable, non-coherent) until the
    ///   loader reprograms it.
    /// - `Config1`: I-cache 16 KiB 4-way 32B lines (IS=1, IL=4, IA=3),
    ///   D-cache 16 KiB 4-way 32B lines (DS=1, DL=4, DA=3). The loader
    ///   derives its cache-invalidate loop length from these fields, so
    ///   they only need to be self-consistent.
    /// - `PRId`: 24Kc-class value 0x0001_9300 (hypothesis; see Q6).
    pub fn new() -> Self {
        let mut c = Self { regs: [[0; 32]; 8] };
        c.regs[0][regs::STATUS as usize] = 0x0040_0004;
        // Config: M(31)=1 | AR(21:20)=1 | MT(15:13)=1 | K0(2:0)=2
        c.regs[0][regs::CONFIG as usize] = 0x8000_0000 | 0x0010_0000 | 0x0000_8000 | 0x2;
        // Config1: IS=1 IL=4 IA=3 DS=1 DL=4 DA=3  (16 KiB, 4-way, 32-byte)
        c.regs[1][regs::CONFIG as usize] =
            (1 << 22) | (4 << 19) | (3 << 16) | (1 << 13) | (4 << 10) | (3 << 7);
        c.regs[0][regs::PRID as usize] = 0x0001_9300;
        c
    }

    /// Read a COP0 register. Some registers are computed.
    pub fn read(&self, rd: u8, sel: u8) -> u32 {
        let s = (sel & 7) as usize;
        let r = (rd & 31) as usize;
        match (rd, sel) {
            (regs::CONFIG, 1) => self.regs[s][r], // Config1
            (15, 1) => self.regs[1][15],          // EBase (sel 1)
            _ => self.regs[s][r],
        }
    }

    /// Write a COP0 register. Writes are accepted raw; the boot path only
    /// programs fields our memory model ignores (K0, TagLo/TagHi, Count).
    pub fn write(&mut self, rd: u8, sel: u8, value: u32) {
        let s = (sel & 7) as usize;
        let r = (rd & 31) as usize;
        self.regs[s][r] = value;
    }

    /// Advance the Count register (called once per executed instruction).
    pub fn tick(&mut self) {
        self.regs[0][regs::COUNT as usize] = self.regs[0][regs::COUNT as usize].wrapping_add(2);
    }

    /// Status register (read helper for tracing/diagnostics).
    pub fn status(&self) -> u32 {
        self.regs[0][regs::STATUS as usize]
    }

    /// Config register (read helper for diagnostics).
    pub fn config(&self) -> u32 {
        self.regs[0][regs::CONFIG as usize]
    }

    /// Config1 register (read helper for diagnostics).
    pub fn config1(&self) -> u32 {
        self.regs[1][regs::CONFIG as usize]
    }

    /// PRId (read helper for diagnostics).
    pub fn prid(&self) -> u32 {
        self.regs[0][regs::PRID as usize]
    }
}

impl Default for Cop0 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_state_is_documented_shape() {
        let c = Cop0::new();
        assert_eq!(
            c.status() & 0x0040_0004,
            0x0040_0004,
            "BEV|ERL set at reset"
        );
        assert_eq!(c.status() & (1 << 19), 0, "NMI bit clear at reset");
        assert_eq!(c.config() & (1 << 31), 1 << 31, "Config.M set");
        assert_eq!(c.config() & (1 << 24), 0, "Config.BE clear (little-endian)");
        assert_eq!((c.config() >> 20) & 3, 1, "Config.AR = Release 2");
    }

    #[test]
    fn config1_cache_geometry_is_self_consistent() {
        let c = Cop0::new();
        let cfg1 = c.config1();
        let il = (cfg1 >> 19) & 7; // line size exponent: line = 2 << IL
        let is = (cfg1 >> 22) & 7; // sets: 0x40 << IS
        let ia = (cfg1 >> 16) & 7; // ways: IA + 1
        let line = 2u32 << il;
        let size = (0x40u32 << is) * (ia + 1) * line;
        assert_eq!(size, 16 * 1024, "I-cache geometry must compute to 16 KiB");
    }

    #[test]
    fn read_write_roundtrip_and_tick() {
        let mut c = Cop0::new();
        c.write(regs::TAGLO, 0, 0xDEAD_BEEF);
        assert_eq!(c.read(regs::TAGLO, 0), 0xDEAD_BEEF);
        c.write(regs::ERROREPC, 0, 0xBFC0_0100);
        assert_eq!(c.read(regs::ERROREPC, 0), 0xBFC0_0100);
        let before = c.read(regs::COUNT, 0);
        c.tick();
        assert_eq!(c.read(regs::COUNT, 0), before.wrapping_add(2));
    }
}
