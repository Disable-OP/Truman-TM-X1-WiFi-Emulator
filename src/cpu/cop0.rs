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
///
/// Milestone U added the architecture Count/Compare timer-interrupt state
/// (docs/firmware-analysis.md §23.2): a pending latch set when Count
/// transitions onto Compare, cleared by a Compare write, delivered as IP7
/// when not disabled (Cause.DC) and reported through the read-only Cause.TI
/// bit. The observed firmware never writes Compare and keeps IE=0, so the
/// real boot is unaffected (pinned by tests).
#[derive(Debug, Clone)]
pub struct Cop0 {
    /// Indexed by [selector][register].
    regs: [[u32; 32]; 8],
    /// Count/Compare timer interrupt pending latch (internal state; not a
    /// register). Set on the Count -> Compare match event, cleared only by
    /// a Compare write. Reset leaves it clear (no match event has run).
    timer_pending: bool,
}

/// COP0 register numbers used by the boot code.
pub mod regs {
    pub const COUNT: u8 = 9;
    pub const COMPARE: u8 = 11;
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
///
/// Bit assignments cross-checked against the architecture (MIPS32
/// Architecture for Programmers Vol. III, Cause register figure) and Linux's
/// `arch/mips/include/asm/mipsregs.h` (`CAUSEF_*`): BD=31, TI=30, CE=29:28,
/// DC=27, PCI=26, IV=23, WP=22, IP[7:2]=15:10, IP[1:0]=9:8, ExcCode=6:2.
/// Milestone U corrected the earlier `IV = 1 << 30` (that bit is TI; IV is
/// bit 23) — docs/firmware-analysis.md §23.2.
pub mod cause_bits {
    /// Exception code (bits 6:2); 0 = interrupt.
    pub const EXCCODE: u32 = 0x1F << 2;
    /// Branch-delay-slot flag (bit 31): EPC points at the branch.
    ///
    /// NOTE: this was previously (incorrectly) bit 22, which collided with
    /// the WP/watch-pending bit and shifted every exception taken in a
    /// delay slot. The architecture puts BD at bit 31; corrected in
    /// Milestone U (§23.2) alongside the IV fix.
    pub const BD: u32 = 1 << 31;
    /// Hardware interrupt inputs IP[7:2] (bits 15:10); read-only.
    pub const IP_HW: u32 = 0x3F << 10;
    /// Software interrupt inputs IP[1:0] (bits 9:8); writable.
    pub const IP_SW: u32 = 0x3 << 8;
    /// IV (bit 23): interrupts use the special vector (base + 0x200).
    /// Corrected from bit 30 in Milestone U (§23.2); bit 30 is TI.
    pub const IV: u32 = 1 << 23;
    /// DC (bit 27, r2): Count/Compare timer interrupt disable.
    pub const DC: u32 = 1 << 27;
    /// TI (bit 30, r2): timer-interrupt-pending indicator (read-only;
    /// composed on Cause reads from the internal pending latch, §23.2).
    pub const TI: u32 = 1 << 30;
    /// WP (bit 22): watch-pending (never set by this model).
    pub const WP: u32 = 1 << 22;
    /// Fields writable through a Cause `mtc0` (r2 write mask): IV, DC and
    /// the software interrupt inputs IP[1:0]. TI is read-only.
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
        let mut c = Self {
            regs: [[0; 32]; 8],
            timer_pending: false,
        };
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
            (regs::CAUSE, 0) => {
                // Two read-only fields are composed on every Cause read:
                // - TI (bit 30): the timer-pending indicator, gated by DC
                //   (documented falsifiable reading, §23.2);
                // - IP7 (bit 15): the Count/Compare timer drives the IP7
                //   hardware input on real cores, so it shows in IP[7:2]
                //   alongside the platform lines driven by set_ip_hw.
                let mut v = self.regs[s][r];
                v &= !(cause_bits::TI | 1 << 15);
                if self.timer_interrupt_active() {
                    v |= cause_bits::TI | 1 << 15;
                }
                v
            }
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
        if rd == regs::COMPARE && sel == 0 {
            // Architecture: writing the Compare register clears the
            // Count/Compare timer interrupt.
            self.timer_pending = false;
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
    /// `((Cause.IP | hw_lines) | timer) & Status.IM != 0`.
    ///
    /// The Count/Compare timer is input IP7 (bits 15:10 bit 7): pending
    /// when the internal latch is set and the timer is not disabled
    /// (Cause.DC = 0), and subject to the same IM7/IE gating as every
    /// other input (MIPS32r2 architecture; §23.2).
    pub fn irq_pending(&self) -> bool {
        let mut ip = self.regs[0][regs::CAUSE as usize] & (cause_bits::IP_HW | cause_bits::IP_SW);
        if self.timer_interrupt_active() {
            // The Count/Compare timer drives input IP7 (Cause bit 15;
            // IP[7:2] live at bits 15:10 with IP2 at 10).
            ip |= 1 << 15;
        }
        ip & (self.regs[0][regs::STATUS as usize] & status_bits::IM) != 0
    }

    /// Timer-interrupt activity as seen by Cause.TI and IP7: the pending
    /// latch set and not disabled by Cause.DC. (DC gating the readout and
    /// delivery while the latch persists is the model's documented reading;
    /// falsifiable, §23.2.)
    fn timer_interrupt_active(&self) -> bool {
        self.timer_pending && self.regs[0][regs::CAUSE as usize] & cause_bits::DC == 0
    }

    /// True when the Count/Compare match event has latched (regardless of
    /// DC). Diagnostic accessor for tests and the tracer.
    pub fn timer_latched(&self) -> bool {
        self.timer_pending
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
    ///
    /// Count/Compare timer (MIPS32r2): when Count *transitions onto* the
    /// Compare value the timer interrupt latches (Cause.TI / IP7 until a
    /// Compare write clears it). The match is an edge, not continuous
    /// equality — reset state (Count = Compare = 0) does not latch, and the
    /// first wrap-around match is 2^31 instructions away (§23.2).
    pub fn tick(&mut self) {
        let new_count = self.regs[0][regs::COUNT as usize].wrapping_add(2);
        self.regs[0][regs::COUNT as usize] = new_count;
        if !self.timer_pending && new_count == self.regs[0][regs::COMPARE as usize] {
            self.timer_pending = true;
        }
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

    // ---- Milestone U: Cause layout correction + Count/Compare timer ----

    /// Regression pin for the §23.2 corrections: the Cause register bit
    /// assignments must match the architecture (Linux `mipsregs.h`
    /// `CAUSEF_*`): BD=31, TI=30, DC=27, IV=23, WP=22.
    #[test]
    fn cause_bit_layout_matches_architecture() {
        assert_eq!(cause_bits::BD, 1 << 31, "BD is bit 31 (was wrongly 22)");
        assert_eq!(cause_bits::TI, 1 << 30, "TI is bit 30");
        assert_eq!(cause_bits::DC, 1 << 27, "DC is bit 27");
        assert_eq!(cause_bits::IV, 1 << 23, "IV is bit 23 (was wrongly 30)");
        assert_eq!(cause_bits::WP, 1 << 22, "WP is bit 22");
    }

    /// Bit 30 of a Cause write is TI (read-only), not IV: writing it must
    /// neither latch the writable storage nor select the special vector.
    #[test]
    fn cause_bit_30_is_ti_and_not_selectable_as_iv() {
        let mut c = Cop0::new();
        c.write(regs::STATUS, 0, status_bits::IE); // BEV=0 -> EBase vectors
        c.write(regs::CAUSE, 0, 1 << 30); // what the old code called IV
        assert_eq!(c.read(regs::CAUSE, 0) & (1 << 30), 0, "TI is read-only");
        assert_eq!(
            c.read(regs::CAUSE, 0) & cause_bits::IV,
            0,
            "bit 30 does not set IV"
        );
        assert_eq!(c.interrupt_vector(), 0x8000_0180, "general vector");
        // The real IV (bit 23) selects the special interrupt vector.
        c.write(regs::CAUSE, 0, cause_bits::IV);
        assert_eq!(c.interrupt_vector(), 0x8000_0200);
    }

    /// The Count -> Compare match event latches the timer interrupt:
    /// Cause.TI reads 1, IP7 shows asserted, and the pending check fires
    /// once IM7 + IE are armed.
    #[test]
    fn timer_interrupt_latches_on_count_compare_match() {
        let mut c = Cop0::new();
        c.write(regs::COMPARE, 0, 10);
        // Count ticks 2 per instruction: 2,4,6,8,10 -> match at 10.
        for _ in 0..5 {
            assert!(!c.timer_latched(), "no latch before the match");
            c.tick();
        }
        assert_eq!(c.read(regs::COUNT, 0), 10);
        assert!(c.timer_latched(), "match event latches the interrupt");
        let cause = c.read(regs::CAUSE, 0);
        assert_eq!(cause & cause_bits::TI, cause_bits::TI, "Cause.TI set");
        assert_eq!(cause & (1 << 15), 1 << 15, "IP7 input asserted");
        // Delivery requires IM7 + IE (IM7 = Status bit 15).
        assert!(!c.irq_pending(), "IE/IM7 not armed yet");
        c.write(regs::STATUS, 0, status_bits::IE | (0xFF << 8));
        assert!(c.irq_pending(), "timer delivers with IE+IM7 armed");
    }

    /// Architecture: writing the Compare register clears the timer
    /// interrupt (the standard on-device ack pattern).
    #[test]
    fn compare_write_clears_the_timer_interrupt() {
        let mut c = Cop0::new();
        c.write(regs::COMPARE, 0, 10);
        for _ in 0..5 {
            c.tick();
        }
        assert!(c.timer_latched());
        c.write(regs::COMPARE, 0, 0xFFFF_0000); // ack + re-arm far away
        assert!(!c.timer_latched(), "Compare write clears the interrupt");
        assert_eq!(c.read(regs::CAUSE, 0) & cause_bits::TI, 0);
        assert_eq!(c.read(regs::CAUSE, 0) & (1 << 15), 0, "IP7 deasserted");
        c.write(regs::STATUS, 0, status_bits::IE | (0xFF << 8));
        assert!(!c.irq_pending());
    }

    /// Cause.DC disables the timer interrupt: no delivery and TI reads 0,
    /// while the latch itself persists (documented falsifiable reading,
    /// §23.2); clearing DC re-activates the latched interrupt.
    #[test]
    fn dc_disables_timer_delivery_and_ti_readout() {
        let mut c = Cop0::new();
        c.write(regs::COMPARE, 0, 10);
        for _ in 0..5 {
            c.tick();
        }
        assert!(c.timer_latched());
        c.write(regs::CAUSE, 0, cause_bits::DC);
        assert_eq!(c.read(regs::CAUSE, 0) & cause_bits::TI, 0, "TI gated by DC");
        c.write(regs::STATUS, 0, status_bits::IE | (0xFF << 8));
        assert!(!c.irq_pending(), "DC blocks delivery");
        c.write(regs::CAUSE, 0, 0); // DC off
        assert!(c.irq_pending(), "latch persisted; delivering again");
    }

    /// Reset state (Count = Compare = 0) is not a match event: the first
    /// tick moves Count away from equality and nothing latches.
    #[test]
    fn reset_state_does_not_latch_timer() {
        let mut c = Cop0::new();
        assert_eq!(c.read(regs::COUNT, 0), 0);
        c.tick();
        assert_eq!(c.read(regs::COUNT, 0), 2);
        assert!(!c.timer_latched(), "reset equality is not a match");
        assert_eq!(c.read(regs::CAUSE, 0) & cause_bits::TI, 0);
    }

    /// A match on the wrapped Count still latches (equality is checked on
    /// every tick, 2^31 instructions apart in the default +2 model).
    #[test]
    fn timer_latches_on_wrapped_count_match() {
        let mut c = Cop0::new();
        c.write(regs::COUNT, 0, 0xFFFF_FFFE);
        c.write(regs::COMPARE, 0, 0); // next tick wraps onto Compare
        c.tick();
        assert_eq!(c.read(regs::COUNT, 0), 0);
        assert!(c.timer_latched(), "wrap-around match latches");
    }
}
