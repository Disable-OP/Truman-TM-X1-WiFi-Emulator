//! Truman TM X1 WiFi machine model: hardware composition and run loop.
//!
//! The default configuration encodes **hypothesis A'** (refined during
//! Milestone B by the kseg1-mask insight) from docs/firmware-analysis.md §8:
//!
//! | Window (kseg1)  | Physical       | Model | Confidence |
//! |-----------------|----------------|-------|------------|
//! | 0xBFC00000+     | 0x1FC00000+    | flash XIP (visible 0x3C0000 bytes) | probable |
//! | 0xBFFC0000+     | 0x1FFC0000+    | IRAM overlay 64 KiB (loader stack) | hypothesis |
//! | 0xBFFE0000+     | 0x1FFE0000+    | param/register page 64 KiB | hypothesis |
//! | 0x80000000+     | 0x00000000+    | DRAM 64 MiB | probable |
//! | 0x98000000+     | 0x18000000+    | flash execution/data mirror (Q2, §16.4) | verified-usage |
//! | 0xB0000000+     | 0x10000000+    | never accessed by the observed boot (the 0xB0000000/0x90000000 lui constants are address-classification masks, not peripheral bases — §16.4) | verified-nonuse |
//! | 0xF0000000+     | 0x30000000+    | never accessed; kseg2 (TLB-mapped on stock MIPS32, faults here) — kept as a visible logging stub | verified-nonuse |
//!
//! Note the kseg1 arithmetic: `0xBFFC0000 & 0x1FFFFFFF = 0x1FFC0000`, i.e.
//! the overlay windows sit in the **top 256 KiB of the 4 MiB flash window**
//! — exactly where the image contains a high-entropy blob (file 0x3C0000)
//! and an erased parameter page (file 0x3E0000). The overlays replace those
//! flash ranges at runtime; the file content below 0x3C0000 stays XIP.
//!
//! Every window can be overridden through [`MachineConfig`] so a wrong
//! hypothesis is a configuration change, not a rewrite.

use crate::cpu::{CpuBackend, CpuError, StepReport};
use crate::devices::{
    DdrCtrl, DdrPhy, DecompEngineHi, DecompEngineLo, EngineShared, IntController, KeyPort,
    LoggingDevice, MirroredRam, ParamRam, SpiController, TimerCounter, Uart,
};
use crate::firmware::Image;
use crate::memory::{AddressSpace, WatchCtx, WatchHit, WatchKinds};

/// Shared access log entry: (offset, size, value, was_write). Backed by a
/// capped `VecDeque` ring (see `devices::AccessLog`, Milestone V §24).
pub type AccessLog = std::rc::Rc<
    std::cell::RefCell<std::collections::VecDeque<(u32, crate::memory::AccessSize, u32, bool)>>,
>;

/// The loader's `jr v0` handoff entry (§13.3): the PNG_DEC pixel output
/// address (PNG_PXL_EXT_ADDR = 0x01600000) executed under the kseg1
/// alias — the decompressed stage's documented load address. The
/// [`MachineConfig::handoff_entry`] default; see
/// [`Machine::map_handoff`].
pub const DEFAULT_HANDOFF_ENTRY: u32 = 0xA160_0000;

/// Machine hardware configuration.
#[derive(Debug, Clone)]
pub struct MachineConfig {
    /// DRAM physical base (kseg0 alias 0x80000000).
    pub dram_base: u32,
    /// DRAM size — "512M" in the filename = 512 Mbit = 64 MiB.
    pub dram_size: u32,
    /// Flash XIP window physical base (kseg1 alias 0xBFC00000).
    pub flash_base: u32,
    /// Visible flash length below the overlay windows.
    pub flash_visible_len: u32,
    /// On-chip RAM overlay used by the loader (stack at +0x0E00).
    pub iram_base: u32,
    pub iram_size: u32,
    /// Loader parameter/register page (loader RMWs +0x210/+0x284/+0x2AC).
    pub sysreg_base: u32,
    pub sysreg_size: u32,
    /// Reset fill byte of the sysreg window.
    /// 0x00 = registers with zeroed reset values (default);
    /// 0xFF = erased-flash-backed page. Selecting this is part of
    /// hypothesis A' (docs/firmware-analysis.md Q1).
    pub sysreg_fill: u8,
    /// Interrupt controller block inside the sysreg page (offset +0x480,
    /// 0x40 bytes — verified init program; hypothesis H11 for behavior).
    pub intc_offset: u32,
    pub intc_size: u32,
    /// Front-panel key data port inside the sysreg page (offset +0x384C,
    /// 4 bytes — verified read/write semantics from the key-scan loop).
    pub keyport_offset: u32,
    pub keyport_size: u32,
    /// Hardware decompression engine register block (hypothesis H12):
    /// +0x3800..0x3878 split into a low region (+0x3800..0x384C: start /
    /// FIFO) and a high region (+0x3850..0x3878: window + output address).
    pub decomp_offset: u32,
    pub decomp_size: u32,
    /// Flash execution/data mirror (Q2 resolved, §16.4): the 0x98000000
    /// (code fetch, stage-1/2 execution) and 0x98010000 (data reads, e.g.
    /// the 0x10000-package descriptors during "LD eCos") accesses both
    /// land here — one linear mirror of the SPI flash at phys 0x18000000.
    pub win_180_base: u32,
    pub win_180_size: u32,
    /// If true, the 0x18000000 window is prefilled with the flash image
    /// contents (linear mirror hypothesis). Writes persist.
    pub win_180_mirror_flash: bool,
    /// Vendor window at phys 0x10000000 (kseg1 0xB0000000 / kseg0
    /// 0x90000000). NEVER accessed by the observed boot (§16.4); the lui
    /// constants are address-classification masks. Kept as a visible
    /// logging stub.
    pub win_100_base: u32,
    pub win_100_size: u32,
    /// Vendor window at phys 0x30000000 (kseg2 0xF0000000). NEVER accessed
    /// by the observed boot; kseg2 is TLB-mapped on stock MIPS32 and
    /// faults in this model — the stub exists so the map stays explicit.
    pub win_300_base: u32,
    pub win_300_size: u32,
    /// The loader's `jr v0` handoff entry (§13.3): the PNG_DEC pixel
    /// output address (PNG_PXL_EXT_ADDR = 0x01600000) executed under the
    /// kseg1 alias — the decompressed stage's documented load address.
    /// [`Machine::map_handoff`] stops here before executing the stage's
    /// first instruction, leaving the loader's register state live.
    pub handoff_entry: u32,
}

impl Default for MachineConfig {
    fn default() -> Self {
        Self {
            dram_base: 0x0000_0000,
            dram_size: 0x0400_0000, // 64 MiB
            flash_base: 0x1FC0_0000,
            // NOTE: the flash ROM region declared from flash_base covers only
            // the visible 0x3C0000 bytes; the top 256 KiB of the 4 MiB window
            // (phys 0x1FFC0000-0x1FFFFFFF) is claimed by the IRAM overlay and
            // the loader parameter page (standard kseg1 masking puts them
            // there), matching the file's own region boundaries.
            flash_visible_len: 0x003C_0000,
            iram_base: 0x1FFC_0000,
            iram_size: 0x0001_0000, // 64 KiB
            sysreg_base: 0x1FFE_0000,
            sysreg_size: 0x0001_0000, // 64 KiB
            sysreg_fill: 0x00,
            intc_offset: 0x480,
            intc_size: 0x40,
            keyport_offset: 0x384C,
            keyport_size: 0x4,
            decomp_offset: 0x3800,
            decomp_size: 0x78,
            win_180_base: 0x1800_0000,
            win_180_size: 0x0040_0000, // 4 MiB linear mirror of the flash
            win_180_mirror_flash: true,
            win_100_base: 0x1000_0000,
            win_100_size: 0x0010_0000,
            win_300_base: 0x3000_0000,
            win_300_size: 0x0010_0000,
            handoff_entry: DEFAULT_HANDOFF_ENTRY,
        }
    }
}

/// Why a run stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// Executed the requested number of instructions.
    StepLimit,
    /// The CPU backend stopped with an error.
    Cpu(CpuError),
    /// The watch PC passed to [`Machine::run_until_pc`] was reached before
    /// executing the instruction there (entry-harness support, §20).
    EntryReached { pc: u32 },
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StopReason::StepLimit => write!(f, "step limit reached"),
            StopReason::Cpu(e) => write!(f, "{e}"),
            StopReason::EntryReached { pc } => {
                write!(f, "entry watch reached at pc=0x{pc:08x}")
            }
        }
    }
}

/// Result of a completed run.
#[derive(Debug)]
pub struct RunResult {
    pub steps: u64,
    pub stop: StopReason,
    /// Final PC.
    pub pc: u32,
    /// Distinct offsets accessed per instrumented window, for diagnostics.
    pub touchmap: Vec<(String, Vec<u32>)>,
}

/// The composed Truman TM X1 machine.
pub struct Machine {
    pub cpu: Box<dyn CpuBackend>,
    pub bus: AddressSpace,
    pub config: MachineConfig,
    /// Shared UART output buffer (the emulated serial console).
    pub uart_output: std::rc::Rc<std::cell::RefCell<Vec<u8>>>,
    /// External interrupt sources (bit i = source i), shared with the
    /// [`IntController`]. Raise/clear these to inject device interrupts.
    pub intc_sources: std::rc::Rc<std::cell::Cell<u32>>,
    /// Aggregate interrupt-controller output line (0/1, wired to CPU IP2).
    pub intc_line: std::rc::Rc<std::cell::Cell<u8>>,
    /// Front-panel key-line level (bit 0/2/4/5/6 = keys; see [`KeyPort`]).
    pub key_pressed: std::rc::Rc<std::cell::Cell<u32>>,
    /// Key event queue: the keycodes the firmware's scan loop writes to
    /// the key port (bit 0→0x100, 2→0x400, 4→0x1000, 5→0x2000).
    pub key_events: std::rc::Rc<std::cell::RefCell<Vec<u32>>>,
    /// Decompression engine shared state (hypothesis H12). The run loop
    /// polls `take_request()` and services the decode with the bus.
    pub decomp: std::rc::Rc<std::cell::RefCell<EngineShared>>,
    /// Borrow-free engine-request hint (Milestone Z, §28): shared with
    /// [`EngineShared::pending`], checked instead of a per-step
    /// `RefCell::borrow_mut`.
    engine_pending: std::rc::Rc<std::cell::Cell<bool>>,
    /// SoC timer counter cell, shared with the [`TimerCounter`] device
    /// (Milestone Z, §28): the run loops increment it directly — one
    /// count per executed instruction, as before.
    timer_count: std::rc::Rc<std::cell::Cell<u32>>,
    /// Access logs of instrumented windows (name, handle).
    logs: Vec<(String, AccessLog)>,
}

impl Machine {
    /// Compose the machine with the default (hypothesis A) configuration.
    pub fn new(cfg: MachineConfig, firmware: &Image) -> Self {
        let mut bus = AddressSpace::new();
        let mut logs: Vec<(String, AccessLog)> = Vec::new();

        bus.add_ram("dram", cfg.dram_base, cfg.dram_size);
        bus.add_rom("flash", cfg.flash_base, {
            let mut v = firmware.bytes.clone();
            v.truncate(cfg.flash_visible_len as usize);
            v
        });

        let iram = ParamRam::new("iram-overlay", cfg.iram_size, 0x00);
        logs.push(("iram-overlay".to_string(), iram.log_handle()));
        bus.add_device("iram-overlay", cfg.iram_base, cfg.iram_size, Box::new(iram));

        // Interrupt controller at sysreg+0x480 (verified init program;
        // hypothesis H11 behavior — see devices::IntController). Carved out
        // of the low param RAM, which is therefore split in two again.
        let intc_sources = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let intc_line = std::rc::Rc::new(std::cell::Cell::new(0u8));
        let intc_log = LoggingDevice::new("intc-log", 0).log_handle();
        let intc = IntController::new(
            "intc",
            std::rc::Rc::clone(&intc_sources),
            std::rc::Rc::clone(&intc_line),
            std::rc::Rc::clone(&intc_log),
        );
        logs.push(("intc".to_string(), intc_log));
        bus.add_tickable_device(
            "intc",
            cfg.sysreg_base + cfg.intc_offset,
            cfg.intc_size,
            Box::new(intc),
        );

        // Timer block at +0x600..+0x620 (delay() polls the counter at
        // +0x600; see devices::TimerCounter). Carved out of the low param
        // RAM, which is therefore split in two. The counter cell is
        // shared with the run loops, which increment it directly (§28) —
        // the device is registered non-tickable.
        let timer_count = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let timer_log = LoggingDevice::new("timer-log", 0).log_handle();
        let timer = TimerCounter::with_count(
            "timer",
            std::rc::Rc::clone(&timer_count),
            std::rc::Rc::clone(&timer_log),
        );
        logs.push(("timer".to_string(), timer_log));
        bus.add_device("timer", cfg.sysreg_base + 0x600, 0x20, Box::new(timer));
        let param = ParamRam::new("sysreg-params", cfg.intc_offset, cfg.sysreg_fill);
        logs.push(("sysreg-params".to_string(), param.log_handle()));
        bus.add_device(
            "sysreg-params",
            cfg.sysreg_base,
            cfg.intc_offset,
            Box::new(param),
        );
        let param_after_intc = ParamRam::new(
            "sysreg-params-mid0",
            0x600 - cfg.intc_offset - cfg.intc_size,
            cfg.sysreg_fill,
        );
        logs.push((
            "sysreg-params-mid0".to_string(),
            param_after_intc.log_handle(),
        ));
        bus.add_device(
            "sysreg-params-mid0",
            cfg.sysreg_base + cfg.intc_offset + cfg.intc_size,
            0x600 - cfg.intc_offset - cfg.intc_size,
            Box::new(param_after_intc),
        );
        let param2 = ParamRam::new("sysreg-params-mid", 0x900 - 0x620, cfg.sysreg_fill);
        logs.push(("sysreg-params-mid".to_string(), param2.log_handle()));
        bus.add_device(
            "sysreg-params-mid",
            cfg.sysreg_base + 0x620,
            0x900 - 0x620,
            Box::new(param2),
        );

        // System page split: UART block sits at +0x900 (verified via the
        // firmware's putchar), the rest is parameter RAM.
        let uart_output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let uart_log = LoggingDevice::new("uart-log", 0).log_handle();
        let uart = Uart::new(
            "uart",
            std::rc::Rc::clone(&uart_output),
            std::rc::Rc::clone(&uart_log),
        );
        logs.push(("uart".to_string(), uart_log));
        bus.add_device("uart", cfg.sysreg_base + 0x900, 0x20, Box::new(uart));
        let param_hi = ParamRam::new("sysreg-params-hi", 0x1500 - 0x920, cfg.sysreg_fill);
        logs.push(("sysreg-params-hi".to_string(), param_hi.log_handle()));
        bus.add_device(
            "sysreg-params-hi",
            cfg.sysreg_base + 0x920,
            0x1500 - 0x920,
            Box::new(param_hi),
        );

        // DDR PHY training engine at +0x1580..+0x15E0 (DDR-V6.2 gate/eye
        // training; see devices::DdrPhy).
        let ddr_log = LoggingDevice::new("ddrphy-log", 0).log_handle();
        let ddrphy = DdrPhy::new("ddrphy", std::rc::Rc::clone(&ddr_log));
        logs.push(("ddrphy".to_string(), ddr_log));
        bus.add_device("ddrphy", cfg.sysreg_base + 0x1580, 0x60, Box::new(ddrphy));
        // DDR controller command/status block at +0x1500..+0x16A0 (see
        // devices::DdrCtrl). Carved out of the upper param RAM, and split
        // around the PHY window into two regions sharing one register file
        // (§31: the hot-region memo requires non-overlapping regions).
        let ddc_log = LoggingDevice::new("ddrctrl-log", 0).log_handle();
        let (ddrctrl, ddr_shared) = DdrCtrl::new("ddrctrl", std::rc::Rc::clone(&ddc_log));
        logs.push(("ddrctrl".to_string(), ddc_log.clone()));
        bus.add_device("ddrctrl", cfg.sysreg_base + 0x1500, 0x80, Box::new(ddrctrl));
        let ddrctrl_hi =
            DdrCtrl::with_shared("ddrctrl-hi", 0xE0, ddr_shared, std::rc::Rc::clone(&ddc_log));
        bus.add_device(
            "ddrctrl-hi",
            cfg.sysreg_base + 0x15E0,
            0xC0,
            Box::new(ddrctrl_hi),
        );
        // Front-panel key data port at +0x384C (verified read/write
        // semantics; see devices::KeyPort). Carved out of the upper param
        // RAM, which is split around it. Bit 6 of reads ORs the hardware
        // decompression engine's done flag (hypothesis H12).
        let key_pressed = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let key_events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let engine_done = std::rc::Rc::new(std::cell::Cell::new(0u8));
        let key_log = LoggingDevice::new("keyport-log", 0).log_handle();
        let keyport = KeyPort::new(
            "keyport",
            std::rc::Rc::clone(&key_pressed),
            std::rc::Rc::clone(&key_events),
            std::rc::Rc::clone(&engine_done),
            std::rc::Rc::clone(&key_log),
        );
        logs.push(("keyport".to_string(), key_log));
        bus.add_device(
            "keyport",
            cfg.sysreg_base + cfg.keyport_offset,
            cfg.keyport_size,
            Box::new(keyport),
        );

        // Hardware decompression engine at +0x3800..+0x3878 (hypothesis
        // H12; see devices::DecompEngineLo). Two register regions share one
        // EngineShared state; the run loop performs the DEFLATE decode and
        // raises the event line the loader polls (0xBFFE384C bit 5, §13.3).
        let decomp_log = LoggingDevice::new("decomp-log", 0).log_handle();
        let (decomp_lo, decomp_shared) = DecompEngineLo::new(
            "decomp-engine",
            std::rc::Rc::clone(&engine_done),
            std::rc::Rc::clone(&decomp_log),
        );
        let decomp_hi = DecompEngineHi::with_shared(
            "decomp-engine-hi",
            std::rc::Rc::clone(&decomp_shared),
            std::rc::Rc::clone(&decomp_log),
        );
        let engine_pending = decomp_shared.borrow().pending.clone();
        logs.push(("decomp-engine".to_string(), decomp_log));
        bus.add_device(
            "decomp-engine",
            cfg.sysreg_base + cfg.decomp_offset,
            0x4C,
            Box::new(decomp_lo),
        );
        bus.add_device(
            "decomp-engine-hi",
            cfg.sysreg_base + cfg.decomp_offset + 0x50,
            cfg.decomp_size - 0x50,
            Box::new(decomp_hi),
        );
        let hi2_low_len = cfg.decomp_offset - 0x16A0;
        let param_hi2 = ParamRam::new("sysreg-params-hi2", hi2_low_len, cfg.sysreg_fill);
        logs.push(("sysreg-params-hi2".to_string(), param_hi2.log_handle()));
        bus.add_device(
            "sysreg-params-hi2",
            cfg.sysreg_base + 0x16A0,
            hi2_low_len,
            Box::new(param_hi2),
        );
        let hi2_high_len = 0x3E00 - cfg.decomp_offset - cfg.decomp_size;
        let param_hi2b = ParamRam::new("sysreg-params-hi2b", hi2_high_len, cfg.sysreg_fill);
        logs.push(("sysreg-params-hi2b".to_string(), param_hi2b.log_handle()));
        bus.add_device(
            "sysreg-params-hi2b",
            cfg.sysreg_base + cfg.decomp_offset + cfg.decomp_size,
            hi2_high_len,
            Box::new(param_hi2b),
        );

        // SPI control register block at +0x3E00..+0x3E40 (verified via the
        // stage-2 loader's SCS flow; see devices::SpiController doc). It is
        // carved out of the sysreg-params-hi range, so the region above it
        // shrinks and is re-declared after the controller block.
        let spi_log = LoggingDevice::new("spi-log", 0).log_handle();
        let spi = SpiController::new("spi", std::rc::Rc::clone(&spi_log));
        logs.push(("spi".to_string(), spi_log));
        bus.add_device("spi", cfg.sysreg_base + 0x3E00, 0x40, Box::new(spi));
        let param_top_len = cfg.sysreg_size - 0x3E40;
        let param_top = ParamRam::new("sysreg-params-top", param_top_len, cfg.sysreg_fill);
        logs.push(("sysreg-params-top".to_string(), param_top.log_handle()));
        bus.add_device(
            "sysreg-params-top",
            cfg.sysreg_base + 0x3E40,
            param_top_len,
            Box::new(param_top),
        );

        for (name, base, size, mirror) in [
            (
                "win-0x18000000",
                cfg.win_180_base,
                cfg.win_180_size,
                cfg.win_180_mirror_flash,
            ),
            ("win-0x10000000", cfg.win_100_base, cfg.win_100_size, false),
            ("win-0x30000000", cfg.win_300_base, cfg.win_300_size, false),
        ] {
            // Unknown windows: modeled as logging RAM so fetch works and
            // every access is recorded. When `mirror` is set, the window is
            // prefilled with the firmware image (linear flash mirror).
            let dev = LoggingDevice::new(name, 0).with_log_capacity(16_384);
            logs.push((name.to_string(), dev.log_handle()));
            if mirror {
                let filled = MirroredRam::new(name, size, &firmware.bytes, dev.log_handle());
                bus.add_device(name, base, size, Box::new(filled));
            } else {
                bus.add_device(name, base, size, Box::new(dev));
            }
        }

        Self {
            cpu: Box::new(crate::cpu::mips32::Mips32::new()),
            bus,
            config: cfg,
            uart_output,
            intc_sources,
            intc_line,
            key_pressed,
            key_events,
            decomp: decomp_shared,
            engine_pending,
            timer_count,
            logs,
        }
    }

    /// Decompression-engine service (§13): when the firmware starts the
    /// engine (write of 0xBFFE3824 with bit 0 set), the run loop performs
    /// the decode the way the engine is programmed: the stream after the
    /// "SP" header, symbols beginning at the start-bit register
    /// (0xBFFE3818, container-relative, minus the 64 header bits) and
    /// running to the end of the input length register (0xBFFE3838), with
    /// end-of-block symbols as markers. Output goes to the engine's
    /// output register pair through real bus stores. Completion raises
    /// the event line the loader polls at 0xBFFE384C bit 5 (§13.3: the
    /// driver's success path counts bit-5 events; its bit-6 path returns
    /// 0 and the boot prints "LD eCos fail").
    fn service_decomp_engine(&mut self) {
        // Borrow-free fast path (§28): the pending hint lives outside the
        // RefCell; take_request stays authoritative.
        if !self.engine_pending.get() {
            return;
        }
        let Some((src, dst)) = self.decomp.borrow_mut().take_request() else {
            return;
        };
        let (start_bit_raw, ilen) = {
            let sh = self.decomp.borrow();
            (sh.reg(0x18), sh.reg(0x38))
        };
        // Input length register (0xBFFE3838) counts bytes from the source
        // (observed 0x8692 for a 0x868C container); clamp to what is
        // plausibly mapped. The stream starts after the 8-byte "SP"
        // header, and the engine's start bit is container-relative
        // (dump evidence: header byte counter 134 includes those 8).
        let ilen = (ilen as usize).clamp(0x100, 0x2_0000);
        let start_bit = start_bit_raw.wrapping_sub(64) as usize;
        let mut src_bytes = Vec::with_capacity(ilen);
        for i in 8..ilen {
            let b = self
                .bus
                .peek_phys(src.wrapping_add(i as u32), crate::memory::AccessSize::Byte)
                .unwrap_or(0);
            src_bytes.push(b as u8);
        }
        let mut out = Vec::new();
        // The value ladder is the named seam for the hardware session
        // (src/inflate.rs `ENGINE_VALUE_TABLES`, docs/firmware-analysis.md
        // §31.4): replacing that constant with the recovered ROM values is
        // the whole post-capture edit.
        let res = crate::inflate::inflate_engine_with(
            &src_bytes,
            start_bit.min(src_bytes.len() * 8),
            0x8000,
            &mut out,
            0x4_0000,
            &crate::inflate::ENGINE_VALUE_TABLES,
        );
        match res {
            Ok(n) => {
                // PNG row semantics (§17): the engine is a PNG decoder
                // (PNG_DEC_* registers, docs/firmware-analysis.md §17). It
                // completes every programmed row of the 0xBFFE3828 x
                // 0xBFFE382C (PNG_IMG_WIDTH x PNG_IMG_HEIGHT) pixel image;
                // when the bitstream ends early the remaining rows come
                // from the zero-filled band buffer, and a decode longer
                // than the image stops at the image end. The driver has no
                // software post-processing pass (0xA2002630..0xA20026C0 is
                // accounting only), so the pixel output is the final image.
                let (width, height) = {
                    let sh = self.decomp.borrow();
                    (sh.reg(0x28), sh.reg(0x2C))
                };
                let pixels = width as usize * height as usize;
                let written = if pixels > 0 && pixels <= 0x4_0000 {
                    let target = pixels.min(0x4_0000);
                    if out.len() > target {
                        out.truncate(target);
                    } else if out.len() < target {
                        out.resize(target, 0);
                    }
                    target
                } else {
                    n
                };
                // PNG_BS_USED_CNT (0xBFFE381C): completed 128-pixel units.
                // The driver's accounting multiplies it by (8 << state)
                // with state = 4, i.e. x128; the real device's post-boot
                // DRAM stores 624 x 128 = 79,872 (§13.6), which pins this
                // unit size (78 rows x 8 units of 128 px).
                if pixels > 0 {
                    let units = (written as u32) / 128;
                    self.decomp.borrow_mut().set_reg(0x1C, units);
                }
                // Write the decoded image through the kseg1 alias so the
                // bus translation and region dispatch stay canonical.
                for (i, &b) in out.iter().enumerate() {
                    let va = 0xA000_0000 | dst.wrapping_add(i as u32);
                    let _ = self
                        .bus
                        .store(va, crate::memory::AccessSize::Byte, u32::from(b));
                }
                self.decomp.borrow_mut().complete(written, None);
            }
            Err(e) => {
                self.decomp
                    .borrow_mut()
                    .complete(out.len(), Some(format!("{e}")));
            }
        }
    }

    /// Access the recorded access log of an instrumented window by name
    /// (e.g. `"decomp-engine"`, `"uart"`) for diagnostics.
    pub fn access_log(&self, name: &str) -> Option<&crate::machine::AccessLog> {
        self.logs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, log)| log)
    }

    /// Read a decompression-engine register (PNG_DEC_* block, sysreg
    /// +0x3800+off; §17). Diagnostics/tests: pins the firmware's engine
    /// program and the completion accounting.
    pub fn decomp_reg(&self, off: usize) -> u32 {
        self.decomp.borrow().reg(off)
    }

    /// The engine's last completion result (bytes written or an error).
    pub fn decomp_last_result(&self) -> Option<Result<usize, String>> {
        self.decomp.borrow().last_result.clone()
    }

    /// Names of all instrumented windows (for phase-by-phase access
    /// correlation; see `examples/window_correlation.rs`).
    pub fn access_log_names(&self) -> Vec<String> {
        self.logs.iter().map(|(n, _)| n.clone()).collect()
    }

    /// Install a physical-range watchpoint (Q8 instrument, §22): every
    /// successful CPU access (load/store/fetch per `kinds`) inside
    /// `base..base+len` is recorded with the issuing (step, PC), the
    /// address, the width and the value. Machine run loops stamp the
    /// context; hits are drained with [`Machine::take_watch_hits`].
    pub fn add_watch(&mut self, base: u32, len: u32, kinds: WatchKinds) {
        self.bus.add_watch(base, len, kinds);
    }

    /// Remove all watchpoints (recorded hits are kept until drained).
    pub fn clear_watches(&mut self) {
        self.bus.clear_watches();
    }

    /// Drain and return the recorded watchpoint hits.
    pub fn take_watch_hits(&mut self) -> Vec<WatchHit> {
        self.bus.take_watch_hits()
    }

    /// Number of watchpoint hits currently buffered.
    pub fn watch_hit_count(&self) -> usize {
        self.bus.watch_hit_count()
    }

    /// Reset the machine to its boot state.
    /// Hypothesis A: the SoC boot ROM has mapped SPI flash at the MIPS boot
    /// window and jumps to offset 0 (kseg1 `0xBFC00000`); see
    /// docs/firmware-analysis.md Q5. This is configurable via
    /// [`MachineConfig::flash_base`] (`0xB0000000 | flash_base`).
    pub fn reset(&mut self) {
        let entry = 0xB000_0000 | self.config.flash_base; // 0xBFC00000
        self.cpu.reset(entry);
    }

    /// Run up to `max_steps` instructions. Returns the stop reason and a
    /// per-window touchmap for diagnostics.
    pub fn run(&mut self, max_steps: u64) -> RunResult {
        self.cpu.set_describe(false);
        let mut steps: u64 = 0;
        let stop = loop {
            if steps >= max_steps {
                break StopReason::StepLimit;
            }
            // Attribute watchpoint hits to the instruction about to run
            // (Q8 instrument, docs/firmware-analysis.md §22).
            if self.bus.has_watches() {
                self.bus.watch_ctx = WatchCtx {
                    step: steps,
                    pc: self.cpu.pc(),
                };
            }
            // Wire the interrupt-controller output into the CPU inputs
            // (deterministic: uses only state set before this step).
            self.cpu.set_ext_irq_lines(self.intc_line.get());
            match self.cpu.step(&mut self.bus) {
                Ok(_) => {
                    steps += 1;
                    self.bus.tick();
                    self.timer_count.set(self.timer_count.get().wrapping_add(1));
                    self.service_decomp_engine();
                }
                Err(e) => break StopReason::Cpu(e),
            }
        };
        let touchmap = self
            .logs
            .iter()
            .map(|(name, log)| {
                let offsets: std::collections::BTreeSet<u32> =
                    log.borrow().iter().map(|e| e.0).collect();
                (name.clone(), offsets.into_iter().collect())
            })
            .collect();
        RunResult {
            steps,
            stop,
            pc: self.cpu.pc(),
            touchmap,
        }
    }

    /// Run up to `max_steps` instructions, stopping early — before
    /// executing the instruction there — when the PC reaches `watch`.
    ///
    /// Entry-harness support (docs/firmware-analysis.md §20): boot the real
    /// firmware with `watch = 0xA1600000` (the loader's `jr v0` target, the
    /// decompressed eCos entry) and the machine stops with the loader's
    /// exact post-jump register state live (v1 = 0x13800, §19.1), giving
    /// candidate decodes a register-aware dynamic oracle instead of static
    /// word classification.
    pub fn run_until_pc(&mut self, max_steps: u64, watch: u32) -> RunResult {
        self.cpu.set_describe(false);
        let mut steps: u64 = 0;
        let stop = loop {
            if self.cpu.pc() == watch {
                break StopReason::EntryReached { pc: watch };
            }
            if steps >= max_steps {
                break StopReason::StepLimit;
            }
            if self.bus.has_watches() {
                self.bus.watch_ctx = WatchCtx {
                    step: steps,
                    pc: self.cpu.pc(),
                };
            }
            self.cpu.set_ext_irq_lines(self.intc_line.get());
            match self.cpu.step(&mut self.bus) {
                Ok(_) => {
                    steps += 1;
                    self.bus.tick();
                    self.timer_count.set(self.timer_count.get().wrapping_add(1));
                    self.service_decomp_engine();
                }
                Err(e) => break StopReason::Cpu(e),
            }
        };
        let touchmap = self
            .logs
            .iter()
            .map(|(name, log)| {
                let offsets: std::collections::BTreeSet<u32> =
                    log.borrow().iter().map(|e| e.0).collect();
                (name.clone(), offsets.into_iter().collect())
            })
            .collect();
        RunResult {
            steps,
            stop,
            pc: self.cpu.pc(),
            touchmap,
        }
    }

    /// Restore a CPU register snapshot (entry-harness support, §20):
    /// re-enter a candidate image from the exact post-boot register state
    /// captured by [`Machine::snapshot`]. Memory state is the harness's
    /// responsibility (see `AddressSpace::copy_in`/`copy_out`).
    pub fn restore_cpu_state(&mut self, snap: &crate::cpu::CpuSnapshot) {
        self.cpu.restore(snap);
    }

    /// Execute up to `max_steps` instructions while capturing every step
    /// into a [`Tracer`]. Slower than [`Machine::run`]; used by `trace`.
    pub fn run_traced(&mut self, max_steps: u64, tracer: &mut Tracer) -> RunResult {
        self.cpu.set_describe(true);
        let mut steps: u64 = 0;
        let stop = loop {
            if steps >= max_steps {
                break StopReason::StepLimit;
            }
            if self.bus.has_watches() {
                self.bus.watch_ctx = WatchCtx {
                    step: steps,
                    pc: self.cpu.pc(),
                };
            }
            self.cpu.set_ext_irq_lines(self.intc_line.get());
            match self.cpu.step(&mut self.bus) {
                Ok(report) => {
                    tracer.push(report);
                    steps += 1;
                    self.bus.tick();
                    self.timer_count.set(self.timer_count.get().wrapping_add(1));
                    self.service_decomp_engine();
                }
                Err(e) => break StopReason::Cpu(e),
            }
        };
        let touchmap = self
            .logs
            .iter()
            .map(|(name, log)| {
                let offsets: std::collections::BTreeSet<u32> =
                    log.borrow().iter().map(|e| e.0).collect();
                (name.clone(), offsets.into_iter().collect())
            })
            .collect();
        RunResult {
            steps,
            stop,
            pc: self.cpu.pc(),
            touchmap,
        }
    }

    /// Register snapshot (for diagnostics output).
    pub fn snapshot(&self) -> crate::cpu::CpuSnapshot {
        self.cpu.snapshot()
    }

    /// Run up to `max_steps` instructions while recording a per-4KiB-page
    /// executed-instruction histogram (Milestone AD, §32). Semantics are
    /// exactly [`Machine::run`]; the histogram is instrumentation, not a
    /// behavior change (only instructions that actually executed are
    /// counted — the faulting instruction of a `StopReason::Cpu` stop is
    /// not).
    pub fn run_observed(&mut self, max_steps: u64, hist: &mut PageHistogram) -> RunResult {
        self.cpu.set_describe(false);
        let mut steps: u64 = 0;
        let stop = loop {
            if steps >= max_steps {
                break StopReason::StepLimit;
            }
            if self.bus.has_watches() {
                self.bus.watch_ctx = WatchCtx {
                    step: steps,
                    pc: self.cpu.pc(),
                };
            }
            self.cpu.set_ext_irq_lines(self.intc_line.get());
            let pc_before = self.cpu.pc();
            match self.cpu.step(&mut self.bus) {
                Ok(_) => {
                    hist.record(pc_before);
                    steps += 1;
                    self.bus.tick();
                    self.timer_count.set(self.timer_count.get().wrapping_add(1));
                    self.service_decomp_engine();
                }
                Err(e) => break StopReason::Cpu(e),
            }
        };
        let touchmap = self
            .logs
            .iter()
            .map(|(name, log)| {
                let offsets: std::collections::BTreeSet<u32> =
                    log.borrow().iter().map(|e| e.0).collect();
                (name.clone(), offsets.into_iter().collect())
            })
            .collect();
        RunResult {
            steps,
            stop,
            pc: self.cpu.pc(),
            touchmap,
        }
    }

    /// Map the post-handoff horizon (Milestone AD, §32).
    ///
    /// Boot the real chain up to the loader's `jr v0` entry
    /// ([`MachineConfig::handoff_entry`]) within `pre_budget` steps —
    /// stopping *before* the stage's first instruction executes, with the
    /// loader's register state live — then execute up to `post_budget`
    /// further steps under the post-handoff instrument:
    ///
    /// - a per-4KiB-page executed-instruction histogram
    ///   ([`PageHistogram`]),
    /// - MMIO watchpoints over every modeled device window (the sysreg
    ///   page and the three vendor windows), recording each post-handoff
    ///   access with its issuing `(step, PC)`,
    /// - a data-write watch over the DRAM span (Milestone AE, §33):
    ///   every post-handoff store is counted per written 4 KiB page and
    ///   the first [`WRITE_SAMPLE_CAP`] are sampled with their addresses
    ///   and values — the written-memory map, i.e. the framebuffer
    ///   question, answered without asserting any semantics,
    /// - an optional per-instruction head trace of the first `head_trace`
    ///   post-handoff instructions,
    /// - the UART console bytes emitted after the handoff.
    ///
    /// The map is evidence-only: it asserts nothing about stage-internal
    /// semantics. With the RFC value tables ([`ENGINE_VALUE_TABLES`]) the
    /// stage dies on its first bad instruction (§32.2); when the capture-
    /// recovered tables land, this same call maps the real menu path.
    pub fn map_handoff(
        &mut self,
        pre_budget: u64,
        post_budget: u64,
        head_trace: usize,
    ) -> HandoffMap {
        let handoff_pc = self.config.handoff_entry;
        let pre = self.run_until_pc(pre_budget, handoff_pc);
        if !matches!(pre.stop, StopReason::EntryReached { .. }) {
            return HandoffMap {
                handoff_reached: false,
                pre_steps: pre.steps,
                handoff_pc,
                handoff_regs: None,
                post_steps: 0,
                post_stop: None,
                final_pc: pre.pc,
                exec_pages: Vec::new(),
                mmio: Vec::new(),
                mmio_overflow: false,
                write_pages: Vec::new(),
                writes_total: 0,
                write_sample: Vec::new(),
                console_after: String::new(),
                head: Vec::new(),
            };
        }
        let handoff_regs = Some(self.snapshot());
        let console_at_handoff = self.uart_output.borrow().len();

        // Watch every modeled device window so post-handoff MMIO is
        // captured with (step, PC) attribution. Data reads and writes only;
        // executed code is covered by the page histogram.
        self.bus.add_watch(
            self.config.sysreg_base,
            self.config.sysreg_size,
            WatchKinds::READ_WRITE,
        );
        self.bus.add_watch(
            self.config.win_180_base,
            self.config.win_180_size,
            WatchKinds::READ_WRITE,
        );
        self.bus.add_watch(
            self.config.win_100_base,
            self.config.win_100_size,
            WatchKinds::READ_WRITE,
        );
        self.bus.add_watch(
            self.config.win_300_base,
            self.config.win_300_size,
            WatchKinds::READ_WRITE,
        );
        // The DRAM span itself, writes only (Milestone AE, §33): fetches
        // are the page histogram's business and reads are not evidence of
        // rendering; the stores are where a framebuffer would light up.
        self.bus.add_watch(
            self.config.dram_base,
            self.config.dram_size,
            WatchKinds::WRITE,
        );

        let mut hist = PageHistogram::new();
        let mut write_hist = PageHistogram::new();
        let mut writes_total: u64 = 0;
        let mut write_sample: Vec<WriteEvent> = Vec::new();
        let dram_base = self.config.dram_base;
        let dram_end = self.config.dram_base.saturating_add(self.config.dram_size);
        let mut head: Vec<crate::cpu::StepReport> = Vec::new();
        let mut post_steps: u64 = 0;
        let mut post_stop: Option<StopReason> = None;
        let mut mmio: Vec<(
            u64,
            u32,
            u32,
            crate::memory::WatchKind,
            crate::memory::AccessSize,
            u32,
        )> = Vec::new();

        // Segment 1: the head trace (optional). Watch hits drained here are
        // already post-window-relative (the loops number steps from zero).
        let mut head_steps: u64 = 0;
        if head_trace > 0 {
            let n = (head_trace as u64).min(post_budget);
            let mut tracer = Tracer::new(head_trace);
            let r1 = self.run_traced(n, &mut tracer);
            head_steps = r1.steps;
            post_steps += r1.steps;
            head = tracer.drain();
            // The traced segment does not feed the histogram itself — its
            // executed PCs are exactly the head reports.
            for r in &head {
                hist.record(r.pc);
            }
            post_stop = Some(r1.stop);
            for h in self.take_watch_hits() {
                record_post_hit(
                    h,
                    dram_base,
                    dram_end,
                    &mut mmio,
                    &mut write_hist,
                    &mut write_sample,
                    &mut writes_total,
                );
            }
        }

        // Segment 2: the remaining budget under the histogram-only loop.
        if head_steps < post_budget && matches!(post_stop, Some(StopReason::StepLimit) | None) {
            let r2 = self.run_observed(post_budget - head_steps, &mut hist);
            post_steps += r2.steps;
            post_stop = Some(r2.stop);
            // This segment's loop numbered steps from zero again — shift
            // them past the head segment so every hit is post-window-relative.
            for mut h in self.take_watch_hits() {
                h.step += head_steps;
                record_post_hit(
                    h,
                    dram_base,
                    dram_end,
                    &mut mmio,
                    &mut write_hist,
                    &mut write_sample,
                    &mut writes_total,
                );
            }
        }
        let mmio_overflow = self.bus.watch_overflow();
        self.clear_watches();

        let uart_len = self.uart_output.borrow().len();
        let console_after = if uart_len > console_at_handoff {
            String::from_utf8_lossy(&self.uart_output.borrow()[console_at_handoff..]).into_owned()
        } else {
            String::new()
        };

        HandoffMap {
            handoff_reached: true,
            pre_steps: pre.steps,
            handoff_pc,
            handoff_regs,
            post_steps,
            post_stop,
            final_pc: self.cpu.pc(),
            exec_pages: hist.top_pages(256),
            mmio,
            mmio_overflow,
            write_pages: write_hist.top_pages(256),
            writes_total,
            write_sample,
            console_after,
            head,
        }
    }

    /// Bytes emitted by the emulated UART so far (the serial console).
    pub fn uart_bytes(&self) -> Vec<u8> {
        self.uart_output.borrow().clone()
    }

    /// UART output as a lossy UTF-8 string (for console printing).
    pub fn uart_console(&self) -> String {
        String::from_utf8_lossy(&self.uart_output.borrow()).into_owned()
    }

    /// Raise external interrupt sources (bit i = source i). Takes effect
    /// on the next executed instruction; the source must be enabled in the
    /// controller's enable registers for the line to assert (the real
    /// firmware's IntCtrl init unmasks with all-ones).
    pub fn raise_int_sources(&self, mask: u32) {
        let cur = self.intc_sources.get();
        self.intc_sources.set(cur | mask);
    }

    /// Clear external interrupt sources.
    pub fn clear_int_sources(&self, mask: u32) {
        let cur = self.intc_sources.get();
        self.intc_sources.set(cur & !mask);
    }

    /// Current aggregate interrupt-controller output line (0/1).
    pub fn intc_output(&self) -> u8 {
        self.intc_line.get()
    }

    /// Press front-panel keys: set the raw key-line bits (bit 0/2/4/5/6
    /// are the lines the firmware's scan dispatcher tests). Takes effect
    /// on the next poll; the firmware then writes the matching keycode to
    /// the port, which lands in [`Machine::key_events`].
    pub fn press_key(&self, bits: u32) {
        let cur = self.key_pressed.get();
        self.key_pressed.set(cur | bits);
    }

    /// Release front-panel keys (clear the raw key-line bits).
    pub fn release_key(&self, bits: u32) {
        let cur = self.key_pressed.get();
        self.key_pressed.set(cur & !bits);
    }

    /// Drain the key event queue (keycodes written by the firmware's
    /// scan loop into the key data port). Order-preserving.
    pub fn take_key_events(&self) -> Vec<u32> {
        std::mem::take(&mut *self.key_events.borrow_mut())
    }

    /// Declared region map for diagnostics output.
    pub fn region_map(&self) -> Vec<(String, u32, u32, &'static str)> {
        self.bus.region_map()
    }
}

/// Hardware-session comparison (Milestone Y, docs/hardware-session.md):
/// the pure-data analysis behind the §22.5 handoff. When a real device's
/// DRAM capture arrives, [`compare_dumps`] decides whether the emulator's
/// engine reference and the device agree, and where they first diverge.
///
/// All arithmetic is integer-only and the function is deterministic —
/// the same inputs always produce the same report (pinned by tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DumpComparison {
    /// Length of the reference (emulator) side in bytes.
    pub reference_len: usize,
    /// Length of the captured (device) side in bytes.
    pub captured_len: usize,
    /// Bytes actually compared: `min(reference_len, captured_len)`.
    pub compared_len: usize,
    /// Matching bytes within the compared prefix.
    pub matching_bytes: usize,
    /// Matching bytes per thousand of the compared prefix
    /// (0 when nothing was compared; integer math, no floats).
    pub match_permille: u64,
    /// Byte index of the first mismatch within the compared prefix.
    pub first_divergence: Option<usize>,
    /// Indices of 1024-byte blocks (within the compared prefix) that
    /// contain at least one mismatching byte, ascending.
    pub divergent_blocks: Vec<usize>,
    /// Longest run of consecutive matching bytes in the compared prefix.
    pub longest_match_run: usize,
}

/// Compare a captured device region against the emulator's reference.
///
/// A size mismatch is not an error: the comparison covers the shorter
/// prefix and both lengths are reported so the caller can act on the
/// delta (e.g. §26.4's 75,398 vs 79,872 question).
pub fn compare_dumps(reference: &[u8], captured: &[u8]) -> DumpComparison {
    let compared = reference.len().min(captured.len());
    let mut matching = 0usize;
    let mut first_divergence: Option<usize> = None;
    let mut divergent_blocks: Vec<usize> = Vec::new();
    let mut longest_run = 0usize;
    let mut run = 0usize;
    let mut last_divergent_block: Option<usize> = None;
    for i in 0..compared {
        if reference[i] == captured[i] {
            matching += 1;
            run += 1;
            if run > longest_run {
                longest_run = run;
            }
        } else {
            if first_divergence.is_none() {
                first_divergence = Some(i);
            }
            run = 0;
            let block = i / 1024;
            if last_divergent_block != Some(block) {
                divergent_blocks.push(block);
                last_divergent_block = Some(block);
            }
        }
    }
    let match_permille = if compared == 0 {
        0
    } else {
        (matching as u64 * 1000) / compared as u64
    };
    DumpComparison {
        reference_len: reference.len(),
        captured_len: captured.len(),
        compared_len: compared,
        matching_bytes: matching,
        match_permille,
        first_divergence,
        divergent_blocks,
        longest_match_run: longest_run,
    }
}

/// Hardware-session residue classification (Milestone Y): the pure-data
/// analysis for experiment P2 (§22.5). The emulator side proves the boot
/// never writes below the pixel base (§18.3), so the expected pre-decode
/// content is all-zero; what a real device actually holds there is the
/// open head-paradox question (§21.6 item 2). This report structures a
/// capture without assuming either answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidueReport {
    /// Capture length in bytes.
    pub len: usize,
    /// Non-zero bytes in the capture.
    pub nonzero_bytes: usize,
    /// Non-zero bytes per thousand of the capture (0 when empty).
    pub nonzero_permille: u64,
    /// Byte index of the first non-zero byte.
    pub first_nonzero: Option<usize>,
    /// Longest run of consecutive non-zero bytes.
    pub longest_nonzero_run: usize,
    /// Indices of 1024-byte blocks containing at least one non-zero byte,
    /// ascending.
    pub nonzero_blocks: Vec<usize>,
}

/// Classify a captured region against the all-zero expectation.
pub fn classify_residue(captured: &[u8]) -> ResidueReport {
    let mut nonzero = 0usize;
    let mut first_nonzero: Option<usize> = None;
    let mut nonzero_blocks: Vec<usize> = Vec::new();
    let mut longest_run = 0usize;
    let mut run = 0usize;
    let mut last_flagged_block: Option<usize> = None;
    for (i, &b) in captured.iter().enumerate() {
        if b != 0 {
            nonzero += 1;
            run += 1;
            if run > longest_run {
                longest_run = run;
            }
            if first_nonzero.is_none() {
                first_nonzero = Some(i);
            }
            let block = i / 1024;
            if last_flagged_block != Some(block) {
                nonzero_blocks.push(block);
                last_flagged_block = Some(block);
            }
        } else {
            run = 0;
        }
    }
    let nonzero_permille = if captured.is_empty() {
        0
    } else {
        (nonzero as u64 * 1000) / captured.len() as u64
    };
    ResidueReport {
        len: captured.len(),
        nonzero_bytes: nonzero,
        nonzero_permille,
        first_nonzero,
        longest_nonzero_run: longest_run,
        nonzero_blocks,
    }
}

/// Per-4KiB-page executed-instruction histogram (Milestone AD, §32).
/// Recorded by [`Machine::run_observed`]: the executed-code map of a run
/// window without storing per-step records. Only instructions that
/// actually executed are counted.
#[derive(Debug, Clone, Default)]
pub struct PageHistogram {
    pages: std::collections::BTreeMap<u32, u64>,
    total: u64,
}

impl PageHistogram {
    /// New empty histogram.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one executed instruction at `pc`.
    pub fn record(&mut self, pc: u32) {
        *self.pages.entry(pc & 0xFFFF_F000).or_insert(0) += 1;
        self.total += 1;
    }

    /// Total instructions recorded.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Distinct pages recorded.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Executed pages, descending by instruction count (ties ascending by
    /// page base), at most `n` entries.
    pub fn top_pages(&self, n: usize) -> Vec<(u32, u64)> {
        let mut v: Vec<(u32, u64)> = self.pages.iter().map(|(k, c)| (*k, *c)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.truncate(n);
        v
    }

    /// All recorded pages ascending by base.
    pub fn pages(&self) -> impl Iterator<Item = (u32, u64)> + '_ {
        self.pages.iter().map(|(k, c)| (*k, *c))
    }

    /// Instruction count recorded for one page base (masked to 4 KiB).
    pub fn count_at(&self, base: u32) -> Option<u64> {
        self.pages.get(&(base & 0xFFFF_F000)).copied()
    }
}

/// Cap on the post-handoff data-store sample ([`HandoffMap::write_sample`],
/// Milestone AE, §33). The histogram counts every recorded store regardless;
/// the sample keeps the *first* writes — the ones that reveal stride and
/// framebuffer placement before the pattern saturates.
pub const WRITE_SAMPLE_CAP: usize = 64;

/// One post-handoff DRAM data store (Milestone AE, §33): the written-memory
/// evidence with its issuing instruction. Evidence-only — the fields record
/// what the machine did, not what the stage "meant".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteEvent {
    /// Step number within the post window (the handoff is step 0's start).
    pub step: u64,
    /// PC of the storing instruction.
    pub pc: u32,
    /// Virtual address issued by the CPU (kseg0/kseg1 view).
    pub vaddr: u32,
    /// Physical address after translation (the DRAM byte that changed).
    pub phys: u32,
    /// Access width.
    pub size: crate::memory::AccessSize,
    /// Value written (zero-extended to 32 bits).
    pub value: u32,
}

/// Route one post-handoff watch hit to its record (Milestone AE, §33).
/// DRAM data writes feed the written-memory map (histogram, total, bounded
/// sample); every other hit stays in the MMIO record. The split is by
/// physical address and kind, not by which watch fired — the DRAM span is
/// watched alongside the device windows with the same machinery, and any
/// hit that is neither a DRAM write nor a device-window access is kept in
/// the MMIO record rather than dropped (the map loses no recorded evidence).
fn record_post_hit(
    h: crate::memory::WatchHit,
    dram_base: u32,
    dram_end: u32,
    mmio: &mut Vec<(
        u64,
        u32,
        u32,
        crate::memory::WatchKind,
        crate::memory::AccessSize,
        u32,
    )>,
    write_hist: &mut PageHistogram,
    write_sample: &mut Vec<WriteEvent>,
    writes_total: &mut u64,
) {
    let in_dram = h.phys >= dram_base && h.phys < dram_end;
    if h.kind == crate::memory::WatchKind::Write && in_dram {
        *writes_total += 1;
        write_hist.record(h.phys);
        if write_sample.len() < WRITE_SAMPLE_CAP {
            write_sample.push(WriteEvent {
                step: h.step,
                pc: h.pc,
                vaddr: h.vaddr,
                phys: h.phys,
                size: h.size,
                value: h.value,
            });
        }
    } else {
        mmio.push((h.step, h.pc, h.phys, h.kind, h.size, h.value));
    }
}

/// Structured post-handoff execution map (Milestone AD, §32): what the
/// machine actually does after the loader's `jr v0` into the decompressed
/// stage. Evidence-only — every field is observed, none is assumed; no
/// stage-internal semantics are asserted. Deterministic for a deterministic
/// machine.
#[derive(Debug, Clone)]
pub struct HandoffMap {
    /// Whether the handoff entry was reached within the pre budget.
    pub handoff_reached: bool,
    /// Steps consumed booting to the handoff (when reached; otherwise the
    /// steps the pre run consumed before its stop).
    pub pre_steps: u64,
    /// The handoff entry PC ([`MachineConfig::handoff_entry`]).
    pub handoff_pc: u32,
    /// CPU snapshot at the handoff — the loader's live register state
    /// (on the real firmware: `v1` = the driver's 0x13800 return value,
    /// §19.1). `None` when the handoff was not reached.
    pub handoff_regs: Option<crate::cpu::CpuSnapshot>,
    /// Instructions executed after the handoff (a faulting instruction is
    /// not counted).
    pub post_steps: u64,
    /// Why the post window ended (`None` when the handoff was not reached).
    pub post_stop: Option<StopReason>,
    /// Final PC after the post window.
    pub final_pc: u32,
    /// Executed 4 KiB pages, descending by instruction count (at most 256).
    pub exec_pages: Vec<(u32, u64)>,
    /// Post-handoff MMIO accesses over the modeled device windows:
    /// `(step_in_post_window, pc, phys_addr, kind, size, value)`.
    pub mmio: Vec<(
        u64,
        u32,
        u32,
        crate::memory::WatchKind,
        crate::memory::AccessSize,
        u32,
    )>,
    /// True when the shared watch-hit cap truncated the post-handoff
    /// access record (device-window accesses and/or DRAM data stores —
    /// both are capped by the same budget).
    pub mmio_overflow: bool,
    /// Post-handoff DRAM data stores, per written 4 KiB physical page,
    /// descending by store count (at most 256) — the written-memory map
    /// (Milestone AE, §33). A framebuffer lights up here without any
    /// assumption about stage semantics.
    pub write_pages: Vec<(u32, u64)>,
    /// Total post-handoff DRAM data stores recorded (bounded by the shared
    /// watch-hit cap; [`HandoffMap::mmio_overflow`] reports truncation).
    pub writes_total: u64,
    /// The first post-handoff DRAM data stores, at most
    /// [`WRITE_SAMPLE_CAP`] — addresses, widths and values for stride and
    /// framebuffer inference (consecutive stores 0x400 apart are 1024-byte
    /// rows: the PNG width, §32.2).
    pub write_sample: Vec<WriteEvent>,
    /// UART console bytes emitted after the handoff (lossy UTF-8).
    pub console_after: String,
    /// The first post-handoff instructions, when a head trace was requested.
    pub head: Vec<crate::cpu::StepReport>,
}

/// Ring-buffer instruction tracer.
pub struct Tracer {
    buf: std::collections::VecDeque<StepReport>,
    capacity: usize,
    pushed: u64,
}

impl Tracer {
    /// Create a tracer holding at most `capacity` reports.
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: std::collections::VecDeque::with_capacity(capacity.min(1 << 20)),
            capacity,
            pushed: 0,
        }
    }

    fn push(&mut self, r: StepReport) {
        self.pushed += 1;
        if self.buf.len() == self.capacity {
            self.buf.pop_front();
        }
        self.buf.push_back(r);
    }

    /// Total instructions seen.
    pub fn total(&self) -> u64 {
        self.pushed
    }

    /// Drain the buffered reports (oldest first).
    pub fn drain(&mut self) -> Vec<StepReport> {
        self.buf.drain(..).collect()
    }
}
