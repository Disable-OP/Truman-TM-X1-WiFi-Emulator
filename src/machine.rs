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
//! | 0x98000000+     | 0x18000000+    | unknown fast RAM/peripheral window | hypothesis |
//! | 0xB0000000+     | 0x10000000+    | unknown peripheral window | hypothesis |
//! | 0xF0000000+     | 0x30000000+    | unknown peripheral window | hypothesis |
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
    DdrCtrl, DdrPhy, LoggingDevice, MirroredRam, ParamRam, SpiController, TimerCounter, Uart,
};
use crate::firmware::Image;
use crate::memory::AddressSpace;

/// Shared access log entry: (offset, size, value, was_write).
pub type AccessLog =
    std::rc::Rc<std::cell::RefCell<Vec<(u32, crate::memory::AccessSize, u32, bool)>>>;

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
    /// Unknown kseg0 window at 0x98000000 (phys 0x18000000).
    /// Evidence (Milestone B): stage-1 `jal`s into 0xB8000998 after init —
    /// a fast execution mirror of the SPI flash. Modeled as copy-on-write
    /// RAM prefilled with the firmware image (linear mirror).
    pub win_180_base: u32,
    pub win_180_size: u32,
    /// If true, the 0x18000000 window is prefilled with the flash image
    /// contents (linear mirror hypothesis). Writes persist.
    pub win_180_mirror_flash: bool,
    /// Unknown kseg1 window at 0xB0000000 (phys 0x10000000).
    pub win_100_base: u32,
    pub win_100_size: u32,
    /// Unknown kseg1 window at 0xF0000000 (phys 0x30000000).
    pub win_300_base: u32,
    pub win_300_size: u32,
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
            win_180_base: 0x1800_0000,
            win_180_size: 0x0040_0000, // 4 MiB linear mirror of the flash
            win_180_mirror_flash: true,
            win_100_base: 0x1000_0000,
            win_100_size: 0x0010_0000,
            win_300_base: 0x3000_0000,
            win_300_size: 0x0010_0000,
        }
    }
}

/// Why a run stopped.
#[derive(Debug)]
pub enum StopReason {
    /// Executed the requested number of instructions.
    StepLimit,
    /// The CPU backend stopped with an error.
    Cpu(CpuError),
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StopReason::StepLimit => write!(f, "step limit reached"),
            StopReason::Cpu(e) => write!(f, "{e}"),
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

        // Timer block at +0x600..+0x620 (delay() polls the counter at
        // +0x600; see devices::TimerCounter). Carved out of the low param
        // RAM, which is therefore split in two.
        let timer_log = LoggingDevice::new("timer-log", 0).log_handle();
        let timer = TimerCounter::new("timer", std::rc::Rc::clone(&timer_log));
        logs.push(("timer".to_string(), timer_log));
        bus.add_tickable_device("timer", cfg.sysreg_base + 0x600, 0x20, Box::new(timer));
        let param = ParamRam::new("sysreg-params", 0x600, cfg.sysreg_fill);
        logs.push(("sysreg-params".to_string(), param.log_handle()));
        bus.add_device("sysreg-params", cfg.sysreg_base, 0x600, Box::new(param));
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
        // devices::DdrCtrl). Carved out of the upper param RAM.
        let ddc_log = LoggingDevice::new("ddrctrl-log", 0).log_handle();
        let ddrctrl = DdrCtrl::new("ddrctrl", std::rc::Rc::clone(&ddc_log));
        logs.push(("ddrctrl".to_string(), ddc_log));
        bus.add_device(
            "ddrctrl",
            cfg.sysreg_base + 0x1500,
            0x1A0,
            Box::new(ddrctrl),
        );
        let param_hi2 = ParamRam::new("sysreg-params-hi2", 0x3E00 - 0x16A0, cfg.sysreg_fill);
        logs.push(("sysreg-params-hi2".to_string(), param_hi2.log_handle()));
        bus.add_device(
            "sysreg-params-hi2",
            cfg.sysreg_base + 0x16A0,
            0x3E00 - 0x16A0,
            Box::new(param_hi2),
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
            logs,
        }
    }

    /// Reset the machine to its boot state.
    ///
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
        let mut steps: u64 = 0;
        let stop = loop {
            if steps >= max_steps {
                break StopReason::StepLimit;
            }
            match self.cpu.step(&mut self.bus) {
                Ok(_) => {
                    steps += 1;
                    self.bus.tick();
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

    /// Execute up to `max_steps` instructions while capturing every step
    /// into a [`Tracer`]. Slower than [`Machine::run`]; used by `trace`.
    pub fn run_traced(&mut self, max_steps: u64, tracer: &mut Tracer) -> RunResult {
        let mut steps: u64 = 0;
        let stop = loop {
            if steps >= max_steps {
                break StopReason::StepLimit;
            }
            match self.cpu.step(&mut self.bus) {
                Ok(report) => {
                    tracer.push(report);
                    steps += 1;
                    self.bus.tick();
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

    /// Bytes emitted by the emulated UART so far (the serial console).
    pub fn uart_bytes(&self) -> Vec<u8> {
        self.uart_output.borrow().clone()
    }

    /// UART output as a lossy UTF-8 string (for console printing).
    pub fn uart_console(&self) -> String {
        String::from_utf8_lossy(&self.uart_output.borrow()).into_owned()
    }

    /// Declared region map for diagnostics output.
    pub fn region_map(&self) -> Vec<(String, u32, u32, &'static str)> {
        self.bus.region_map()
    }
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
