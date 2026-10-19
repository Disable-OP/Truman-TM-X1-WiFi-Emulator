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

/// Status register field masks (MIPS32r2).
pub mod status_bits {
    /// Global interrupt enable.
    pub const IE: u32 = 1 << 0;
    /// Exception level (blocks further exceptions).
    pub const EXL: u32 = 1 << 1;
    /// Error level (blocks further exceptions; reset value).
    pub const ERL: u32 = 1 << 2;
    /// Interrupt masks IM[7:0] (bits 15:8).
    pub const IM: u32 = 0xFF << 8;
    /// Boot exception vectors (1 = ROM vectors 0xBFC0_0200).
    pub const BEV: u32 = 1 << 22;
    /// NMI flag (read-only input from reset logic).
    pub const NMI: u32 = 1 << 19;
}

/// Cause register field masks (MIPS32r2).
pub mod cause_bits {
    /// Exception code (bits 6:2); 0 = interrupt.
    pub const EXCCODE: u32 = 0x1F << 2;
    /// Branch-delay-slot flag (bit 22): EPC points at the branch.
    pub const BD: u32 = 1 << 22;
    /// Hardware interrupt inputs IP[7:2] (bits 15:10); read-only.
    pub const IP_HW: u32 = 0x3F << 10;
    /// Software interrupt inputs IP[1:0] (bits 9:8); writable.
    pub const IP_SW: u32 = 0x3 << 8;
    /// IV (bit 30): interrupts use the special vector (base + 0x200).
    pub const IV: u32 = 1 << 30;
    /// DC (bit 27, r2): Count/Compare timer interrupt disable.
    pub const DC: u32 = 1 << 27;
    /// WP (bit 22... overlaps BD on r2 — watch-pending, unused here).
    pub const WRITABLE: u32 = IV | DC | IP_SW;
}

/// Exception vector offsets from the vector base (MIPS32r2).
pub mod vectors {
    /// General exception vector offset (also interrupts when Cause.IV=0).
    pub const GENERAL: u32 = 0x180;
    /// Special interrupt vector offset (used when Cause.IV=1).
    pub const INTERRUPT: u32 = 0x200;
    /// BEV=1 base in the boot ROM window.
    pub const ROM_BASE: u32 = 0xBFC0_0200;
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
    /// - `EBase (15,1)`: 0x8000_0000 (kseg0 base; the main firmware never
    ///   writes it, so with BEV=0 vectors land at 0x8000_0180 — see
    ///   docs/firmware-analysis.md §11).
    pub fn new() -> Self {
        let mut c = Self { regs: [[0; 32]; 8] };
        c.regs[0][regs::STATUS as usize] = 0x0040_0004;
        // Config: M(31)=1 | AR(21:20)=1 | MT(15:13)=1 | K0(2:0)=2
        c.regs[0][regs::CONFIG as usize] = 0x8000_0000 | 0x0010_0000 | 0x0000_8000 | 0x2;
        // Config1: IS=1 IL=4 IA=3 DS=1 DL=4 DA=3  (16 KiB, 4-way, 32-byte)
        c.regs[1][regs::CONFIG as usize] =
            (1 << 22) | (4 << 19) | (3 << 16) | (1 << 13) | (4 << 10) | (3 << 7);
        c.regs[0][regs::PRID as usize] = 0x0001_9300;
        c.regs[1][regs::PRID as usize] = 0x8000_0000; // EBase (15,1)
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

    /// Write a COP0 register.
    ///
    /// Most registers are raw storage (the boot path only programs fields
    /// our memory model ignores). Two registers have architectural write
    /// masks that matter for interrupt delivery:
    ///
    /// - **Cause (13,0)**: only IV, DC and the software interrupt inputs
    ///   IP[1:0] are writable; hardware fields (BD, ExcCode, IP[7:2]) are
    ///   preserved. (MIPS32r2 manual, Cause register write behavior.)
    /// - everything else: raw write.
    pub fn write(&mut self, rd: u8, sel: u8, value: u32) {
        let s = (sel & 7) as usize;
        let r = (rd & 31) as usize;
        if rd == regs::CAUSE && sel == 0 {
            let old = self.regs[s][r];
            self.regs[s][r] = (old & !cause_bits::WRITABLE) | (value & cause_bits::WRITABLE);
            return;
        }
        self.regs[s][r] = value;
    }

    /// Set the hardware interrupt inputs IP[7:2] (driven by the platform
    /// interrupt controller). These bits are read-only to software.
    pub fn set_ip_hw(&mut self, lines: u8) {
        let old = self.regs[0][regs::CAUSE as usize];
        self.regs[0][regs::CAUSE as usize] =
            (old & !cause_bits::IP_HW) | (u32::from(lines & 0x3F) << 10);
    }

    /// Whether an interrupt is pending and not masked:
    /// `(Cause.IP | hw_lines) & Status.IM != 0`.
    pub fn irq_pending(&self) -> bool {
        let ip = self.regs[0][regs::CAUSE as usize] & (cause_bits::IP_HW | cause_bits::IP_SW);
        ip & (self.regs[0][regs::STATUS as usize] & status_bits::IM) != 0
    }

    /// Exception vector for an interrupt, per MIPS32r2 vector setup:
    /// BEV=1 → ROM base + offset; BEV=0 → EBase + offset. `Cause.IV`
    /// selects the special interrupt vector (0x200) over the general one
    /// (0x180).
    pub fn interrupt_vector(&self) -> u32 {
        let base = if self.status() & status_bits::BEV != 0 {
            vectors::ROM_BASE
        } else {
            // EBase[31:12] is the base; the CPUNum field (bits 9:0) is not
            // part of the vector address.
            self.regs[1][regs::PRID as usize] & !0xFFF
        };
        let off = if self.regs[0][regs::CAUSE as usize] & cause_bits::IV != 0 {
            vectors::INTERRUPT
        } else {
            vectors::GENERAL
        };
        base + off
    }

    /// Set Exception Code and BD in Cause and latch `epc` (interrupt or
    /// general exception entry). Returns the vector address for the caller.
    pub fn enter_exception(&mut self, exccode: u32, epc: u32, in_delay_slot: bool) -> u32 {
        let old = self.regs[0][regs::CAUSE as usize];
        self.regs[0][regs::CAUSE as usize] = (old & !(cause_bits::EXCCODE | cause_bits::BD))
            | ((exccode & 0x1F) << 2)
            | if in_delay_slot { cause_bits::BD } else { 0 };
        self.regs[0][regs::EPC as usize] = epc;
        self.regs[0][regs::STATUS as usize] |= status_bits::EXL;
        self.interrupt_vector()
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

    #[test]
    fn ebase_reset_value_selects_kseg0_vectors() {
        let c = Cop0::new();
        assert_eq!(c.read(15, 1), 0x8000_0000, "EBase reset = 0x80000000");
    }

    #[test]
    fn interrupt_vector_selection() {
        // Reset state: BEV=1 → ROM interrupt vector 0xBFC0_0380.
        let mut c = Cop0::new();
        assert_eq!(c.interrupt_vector(), 0xBFC0_0380);
        // BEV=0 → EBase + 0x180 (general vector; Cause.IV=0).
        c.write(
            regs::STATUS,
            0,
            (c.status() & !status_bits::BEV) | status_bits::IE,
        );
        assert_eq!(c.interrupt_vector(), 0x8000_0180);
        // Cause.IV=1 → special interrupt vector EBase + 0x200.
        c.write(regs::CAUSE, 0, cause_bits::IV);
        assert_eq!(c.interrupt_vector(), 0x8000_0200);
        // A written EBase moves the base (bits 11:0 masked out).
        c.write(15, 1, 0x8000_1234);
        assert_eq!(c.interrupt_vector(), 0x8000_1200);
    }

    #[test]
    fn irq_pending_needs_im_and_hw_or_sw_input() {
        let mut c = Cop0::new();
        c.write(
            regs::STATUS,
            0,
            status_bits::IE | status_bits::IM, // armed, all masks on
        );
        assert!(!c.irq_pending(), "no inputs yet");
        // Hardware input IP2 pending + IM2 set → pending.
        c.set_ip_hw(0b00_0001);
        assert!(c.irq_pending());
        // Mask IM2 off → not pending.
        c.write(
            regs::STATUS,
            0,
            status_bits::IE | (status_bits::IM & !(1 << 10)),
        );
        assert!(!c.irq_pending(), "IM2 cleared: masked out");
        // Software interrupt IP0 via a Cause write (IP1:0 writable).
        c.write(regs::STATUS, 0, status_bits::IE | status_bits::IM);
        c.write(regs::CAUSE, 0, 0x100);
        assert!(c.irq_pending(), "software IP0 with IM0 set");
    }

    #[test]
    fn enter_exception_sets_fields_and_exl() {
        let mut c = Cop0::new();
        c.write(regs::STATUS, 0, status_bits::IE); // ERL cleared
        let v = c.enter_exception(0, 0x8000_1234, false);
        assert_eq!(v, 0x8000_0180);
        assert_eq!(c.read(regs::EPC, 0), 0x8000_1234);
        assert_eq!(c.status() & status_bits::EXL, status_bits::EXL);
        assert_eq!(c.read(regs::CAUSE, 0) & cause_bits::EXCCODE, 0);
        assert_eq!(c.read(regs::CAUSE, 0) & cause_bits::BD, 0);
        // Delay-slot variant sets BD and relatches EPC.
        let _ = c.enter_exception(0, 0x8000_1238, true);
        assert_eq!(c.read(regs::CAUSE, 0) & cause_bits::BD, cause_bits::BD);
        assert_eq!(c.read(regs::EPC, 0), 0x8000_1238);
    }
}
