//! Memory-mapped device models.
//!
//! Devices receive offsets relative to their region base and serve
//! byte/half/word accesses. Unimplemented behavior must be *visible*:
//! stubs log their accesses instead of silently pretending to work
//! (project rule: never fake a functioning peripheral).

/// Shared device access log: (offset, size, value, was_write).
pub type AccessLog =
    std::rc::Rc<std::cell::RefCell<Vec<(u32, crate::memory::AccessSize, u32, bool)>>>;

/// A memory-mapped device model.
pub trait Device: std::fmt::Debug {
    /// Device name for diagnostics.
    fn name(&self) -> &str;

    /// Read at region-relative offset. Returns the zero-extended value.
    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32;

    /// Write at region-relative offset.
    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32);

    /// Advance time-dependent devices by one CPU step. Default: static.
    fn tick(&mut self) {}

    /// Whether the address space may serve instruction fetches from this
    /// device (RAM-like devices). Default: no (explicit opt-in).
    fn fetchable(&self) -> bool {
        false
    }

    /// Human-readable description of a region-relative offset, if known.
    fn describe_offset(&self, _offset: u32) -> Option<&'static str> {
        None
    }
}

/// A stub device that records every access and returns a fixed default.
///
/// Used for register windows whose semantics are still unknown; the access
/// log feeds the memory-map investigation (see docs/firmware-analysis.md §8).
#[derive(Debug)]
pub struct LoggingDevice {
    name: String,
    default_read: u32,
    /// Shared access log: (offset, size, value, was_write).
    pub log: AccessLog,
    log_capacity: usize,
}

impl LoggingDevice {
    /// Create a logging stub returning `default_read` on reads.
    pub fn new(name: &str, default_read: u32) -> Self {
        Self {
            name: name.to_string(),
            default_read,
            log: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
            log_capacity: 8192,
        }
    }

    /// Cap the internal access log (oldest entries are dropped).
    pub fn with_log_capacity(mut self, cap: usize) -> Self {
        self.log_capacity = cap;
        self
    }

    /// Shared handle to the access log (for machine-level diagnostics).
    pub fn log_handle(&self) -> AccessLog {
        std::rc::Rc::clone(&self.log)
    }

    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.remove(0);
        }
        log.push((offset, size, value, write));
    }
}

impl Device for LoggingDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        self.record(offset, size, 0, false);
        self.default_read
    }

    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32) {
        self.record(offset, size, value, true);
    }
}

/// RAM-backed "parameter/register" window.
///
/// Hypothesis A (docs/firmware-analysis.md Q1) treats the kseg1 windows
/// 0xBFFC0000 (loader stack) and 0xBFFE0000 (read-modify-write parameters)
/// as writable on-chip memory rather than flash. This device models that:
/// reads return stored contents, writes are remembered, and every access is
/// recorded for later analysis.
#[derive(Debug)]
pub struct ParamRam {
    name: String,
    data: Vec<u8>,
    log: AccessLog,
    log_capacity: usize,
}

impl ParamRam {
    /// Create a parameter RAM of `len` bytes filled with `fill`.
    pub fn new(name: &str, len: u32, fill: u8) -> Self {
        Self {
            name: name.to_string(),
            data: vec![fill; len as usize],
            log: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
            log_capacity: 4096,
        }
    }

    /// Shared handle to the access log (for machine-level diagnostics).
    pub fn log_handle(&self) -> AccessLog {
        std::rc::Rc::clone(&self.log)
    }

    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.remove(0);
        }
        log.push((offset, size, value, write));
    }
}

impl Device for ParamRam {
    fn name(&self) -> &str {
        &self.name
    }

    fn fetchable(&self) -> bool {
        true
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        self.record(offset, size, 0, false);
        let off = offset as usize;
        let n = size.bytes() as usize;
        if off + n > self.data.len() {
            return 0;
        }
        let sl = &self.data[off..off + n];
        match size {
            crate::memory::AccessSize::Byte => u32::from(sl[0]),
            crate::memory::AccessSize::Half => u32::from(u16::from_le_bytes([sl[0], sl[1]])),
            crate::memory::AccessSize::Word => u32::from_le_bytes([sl[0], sl[1], sl[2], sl[3]]),
        }
    }

    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32) {
        self.record(offset, size, value, true);
        let off = offset as usize;
        let n = size.bytes() as usize;
        if off + n > self.data.len() {
            return;
        }
        let b = value.to_le_bytes();
        self.data[off..off + n].copy_from_slice(&b[..n]);
    }
}

/// RAM prefilled with a source image, with shared access logging.
///
/// Models the 0x18000000 "fast mirror" window: on real silicon this is
/// likely on-chip SRAM that the boot ROM fills with (a prefix of) the SPI
/// flash for faster execution, or a hardware alias. Reads/writes behave as
/// plain RAM; the initial content is a linear copy of the firmware image.
#[derive(Debug)]
pub struct MirroredRam {
    name: String,
    data: Vec<u8>,
    log: AccessLog,
    log_capacity: usize,
}

impl MirroredRam {
    /// Create a RAM of `len` bytes prefilled with `image` (truncated/zeroed).
    pub fn new(name: &str, len: u32, image: &[u8], log: AccessLog) -> Self {
        let mut data = vec![0u8; len as usize];
        let n = image.len().min(data.len());
        data[..n].copy_from_slice(&image[..n]);
        Self {
            name: name.to_string(),
            data,
            log,
            log_capacity: 16_384,
        }
    }

    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.remove(0);
        }
        log.push((offset, size, value, write));
    }
}

impl Device for MirroredRam {
    fn name(&self) -> &str {
        &self.name
    }

    fn fetchable(&self) -> bool {
        true
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        self.record(offset, size, 0, false);
        let off = offset as usize;
        let n = size.bytes() as usize;
        if off + n > self.data.len() {
            return 0;
        }
        let sl = &self.data[off..off + n];
        match size {
            crate::memory::AccessSize::Byte => u32::from(sl[0]),
            crate::memory::AccessSize::Half => u32::from(u16::from_le_bytes([sl[0], sl[1]])),
            crate::memory::AccessSize::Word => u32::from_le_bytes([sl[0], sl[1], sl[2], sl[3]]),
        }
    }

    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32) {
        self.record(offset, size, value, true);
        let off = offset as usize;
        let n = size.bytes() as usize;
        if off + n > self.data.len() {
            return;
        }
        let b = value.to_le_bytes();
        self.data[off..off + n].copy_from_slice(&b[..n]);
    }
}

/// Minimal Sunplus-style UART model discovered by Milestone B tracing.
///
/// Register layout inside the 0xBFFE0000 system page (verified by the
/// firmware's own putchar routine at mirror 0x958-0x970):
/// - +0x900: TX data register (write = emit byte)
/// - +0x904: status register; bit 0 = transmitter ready (always 1 here)
///
/// Emitted bytes are collected in a shared buffer the frontend prints as
/// the emulated serial console.
#[derive(Debug)]
pub struct Uart {
    name: String,
    pub output: std::rc::Rc<std::cell::RefCell<Vec<u8>>>,
    log: AccessLog,
    log_capacity: usize,
}

impl Uart {
    /// Create a UART whose output appends to `output`.
    pub fn new(
        name: &str,
        output: std::rc::Rc<std::cell::RefCell<Vec<u8>>>,
        log: AccessLog,
    ) -> Self {
        Self {
            name: name.to_string(),
            output,
            log,
            log_capacity: 16_384,
        }
    }

    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.remove(0);
        }
        log.push((offset, size, value, write));
    }
}

impl Device for Uart {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        self.record(offset, size, 0, false);
        match (offset, size) {
            // status: bit 0 = TX ready (stage-1/2 putchar polls this);
            // bit 6 = TX idle/fifo-empty (main firmware polls 0x40 —
            // hypothesis H7, observed at 0xA200044C).
            (0x004, _) => 0x41,
            _ => 0,
        }
    }

    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32) {
        self.record(offset, size, value, true);
        if offset == 0x000 {
            let byte = (value & 0xFF) as u8;
            self.output.borrow_mut().push(byte);
        }
    }
}

/// Sunplus-style SPI controller (probe/-control registers) discovered by
/// Milestone B/C tracing and static analysis of the stage-2 loader
/// (docs/firmware-analysis.md §9, hypothesis H3).
///
/// Register block inside the 0xBFFE0000 system page at +0x3E00..+0x3E40:
/// - +0x04 control: write bit 0x20 = start transfer (fast variant) → sets
///   status bit 0x400; write bit 0x40 = start (alt variant) → status 0x800.
/// - +0x08 status: bit 0x400 / 0x800 = transfer done; any-value write clears
///   the written bits (`status &= !value`).
/// - +0x0C TX word: captures the last written 16-bit command (e.g. 0x0604).
/// - +0x14 RX byte: **hypothesis H3** — returns the echo of the TX word's
///   high byte (`tx >> 8`). Evidence: the loader sends command 0x0604 four
///   times and its result checks (RX==6 → success paths) are all consistent
///   with `0x0604 >> 8 == 6`; no call site requires any other value.
/// - +0x24 CS/timing config: stored, no behavioral effect.
///
/// Bulk flash data does NOT flow through this block — the loader reads the
/// payload through kseg2 mapped windows (0xA2200000/0xA2300000) — so an
/// instant-complete model is sufficient for the boot path. All accesses are
/// logged; the hypothesis is falsifiable via the trace.
#[derive(Debug)]
pub struct SpiController {
    name: String,
    /// Last word written to +0x0C (TX command).
    tx_word: u32,
    /// Status latched in +0x08 (done bits).
    status: u32,
    /// Last control word written to +0x04.
    control: u32,
    /// Stored CS/timing config from +0x24.
    cs_config: u32,
    log: AccessLog,
    log_capacity: usize,
}

impl SpiController {
    /// Create an SPI controller block sharing `log`.
    pub fn new(name: &str, log: AccessLog) -> Self {
        Self {
            name: name.to_string(),
            tx_word: 0,
            status: 0,
            control: 0,
            cs_config: 0,
            log,
            log_capacity: 16_384,
        }
    }

    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.remove(0);
        }
        log.push((offset, size, value, write));
    }
}

impl Device for SpiController {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        let v = match offset {
            0x04 => self.control,
            0x08 => self.status,
            0x0C => self.tx_word,
            // H3: RX byte = opcode echo of the last TX word.
            0x14 => (self.tx_word >> 8) & 0xFF,
            0x24 => self.cs_config,
            _ => 0,
        };
        self.record(offset, size, v, false);
        v
    }

    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32) {
        self.record(offset, size, value, true);
        match offset {
            0x04 => {
                self.control = value;
                // Start bits launch the (instant) transfer and latch done bits.
                if value & 0x20 != 0 {
                    self.status |= 0x400;
                }
                if value & 0x40 != 0 {
                    self.status |= 0x800;
                }
            }
            0x08 => {
                // Observed: the loader writes 0 to clear the done bits.
                // Rule: write-0 clears everything; other values clear the
                // written bits (defensive generalization, unverified).
                if value == 0 {
                    self.status = 0;
                } else {
                    self.status &= !value;
                }
            }
            0x0C => self.tx_word = value & 0xFFFF,
            0x24 => self.cs_config = value,
            _ => {}
        }
    }
}

/// Free-running timer counter in the 0xBFFE0000 system page (+0x600 block).
///
/// Discovered by the stage-2 `delay(N)` routine at mirror 0xB8008880:
/// it writes 0 to +0x600 (software reset) then polls +0x600 until the value
/// reaches 90*N (docs/firmware-analysis.md §9). So the register is a
/// software-writable counter that also increments on its own — a free-
/// running hardware timer.
///
/// Emulation model (documented hypothesis H4):
/// - offset 0x00 (+0x600): the counter. Read = current count; write = set.
///   Each `tick()` (one executed CPU instruction) increments it by 1
///   (wrapping u32). The real tick rate is unknown (probably 1/90 ms given
///   the 90x multiplier); 1/instruction preserves the *flow* semantics
///   (delays terminate, ordering preserved) at a faster wall-clock ratio.
/// - offsets 0x04..0x20: timer config registers observed being written on
///   the boot path (+0x60C = 0x8095, +0x61C = 0). Stored and logged; no
///   behavioral effect modeled.
#[derive(Debug)]
pub struct TimerCounter {
    name: String,
    count: u32,
    config: [u32; 7], // +0x04 .. +0x1C
    log: AccessLog,
    log_capacity: usize,
}

impl TimerCounter {
    /// Create a timer block sharing `log`.
    pub fn new(name: &str, log: AccessLog) -> Self {
        Self {
            name: name.to_string(),
            count: 0,
            config: [0; 7],
            log,
            log_capacity: 16_384,
        }
    }

    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.remove(0);
        }
        log.push((offset, size, value, write));
    }
}

impl Device for TimerCounter {
    fn name(&self) -> &str {
        &self.name
    }

    fn tick(&mut self) {
        self.count = self.count.wrapping_add(1);
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        let v = if offset == 0 {
            self.count
        } else if (4..0x20).contains(&offset) && (offset & 3) == 0 {
            self.config[(offset >> 2) as usize - 1]
        } else {
            0
        };
        self.record(offset, size, v, false);
        v
    }

    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32) {
        self.record(offset, size, value, true);
        if offset == 0 {
            self.count = value;
        } else if (4..0x20).contains(&offset) && (offset & 3) == 0 {
            self.config[(offset >> 2) as usize - 1] = value;
        }
    }
}

/// DDR PHY training engine block in the 0xBFFE0000 system page
/// (+0x1580..+0x15E0), reverse-engineered from the stage-2 DDR-V6.2 training
/// code (docs/firmware-analysis.md §9, hypothesis H5).
///
/// Observed protocol (mirror 0xB800C24C..):
/// - config writes: 0x1580=0x0036AA01, 0x158C=0x000A1220, 0x1590=0x00F0F0F3,
///   0x15B0=0x00455627, 0x15C8=0x00100409, 0x15CC=0x0212521D, 0x15D4=0xC400
/// - command: +0x1584 <- 0x0F02 then 0x0F06 (GO variants)
/// - status: +0x1588, bit 0 = training done (polled until set)
/// - results read after done: +0x1588 bits 9..11, +0x15A4 bits 1..2 —
///   printed as the WL/REYE/WEYE training metrics.
///
/// Emulation model: config registers store values; a command write to
/// +0x1584 completes instantly (status bit 0 set, done auto-clears on next
/// command write); result registers read 0 (no real measurement — values
/// are hypothesis H5 and flagged for validation; if the boot later fails on
/// training margins, this is the knob).
#[derive(Debug)]
pub struct DdrPhy {
    name: String,
    config: std::collections::BTreeMap<u32, u32>,
    status_done: bool,
    log: AccessLog,
    log_capacity: usize,
}

impl DdrPhy {
    /// Create a DDR PHY block sharing `log`.
    pub fn new(name: &str, log: AccessLog) -> Self {
        Self {
            name: name.to_string(),
            config: std::collections::BTreeMap::new(),
            status_done: false,
            log,
            log_capacity: 16_384,
        }
    }

    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.remove(0);
        }
        log.push((offset, size, value, write));
    }
}

impl Device for DdrPhy {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        // Region base is 0x1580; +0x08 = 0x1588 status, +0x24 = 0x15A4 results.
        let v = match offset {
            0x08 if self.status_done => 0x1, // done bit only; result bits 9..11 = 0
            0x24 => 0,                       // training results (H5: zeros)
            other => self.config.get(&other).copied().unwrap_or(0),
        };
        self.record(offset, size, v, false);
        v
    }

    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32) {
        self.record(offset, size, value, true);
        match offset {
            0x04 => {
                // Command (0x0F02/0x0F06 observed): complete instantly.
                self.status_done = true;
                self.config.insert(0x04, value);
            }
            other => {
                self.config.insert(other, value);
            }
        }
    }
}

/// DDR controller command/status block in the 0xBFFE0000 system page
/// (+0x1500..+0x16A0), reverse-engineered from the stage-2 DDR-V6.2 training
/// code (docs/firmware-analysis.md §9, hypothesis H6).
///
/// Observed protocol (mirror 0xB800B394..0xB800C9F4):
/// - +0x1504 (offset 0x04): config+status mix; **bit 2** polled with a
///   10,000,000-iteration timeout at 0xB800B394 ("hardware ready" — on the
///   emulator the wait would always expire; modeled as instant-ready).
/// - +0x1600 (offset 0x20): status; **bit 0** polled ("ready") at 0xB800C9EC.
/// - +0x1628 (offset 0x28): issue/strobe register, RMW'd with bits
///   0x20 / 0x01 / 0x02 / 0x100 / 0x200 before each command; the loader then
///   polls **bit 17** ("cmd issue ack", 0xB800C5C4) and **bit 18**
///   ("DRAM initial done", 0xB800C61C).
/// - +0x1660 (offset 0x60): command word (e.g. 0x20420A73 written after
///   clearing).
/// - +0x166C (offset 0x6C): argument register (0x80000002, 0x80000003,
///   0x00A00000, 0x00100A73... sequence).
///
/// Emulation model: values stored; the polled ready/done bits always read 1
/// because emulated commands complete instantly (hypothesis H6, falsifiable
/// via the access log if the boot later takes a wrong path).
///
/// NOTE: the DdrPhy training block (0x1580..0x15E0) sits inside this range;
/// it is registered in the address space *before* this device and region
/// lookup is first-match-wins, so PHY accesses take precedence there.
#[derive(Debug)]
pub struct DdrCtrl {
    name: String,
    regs: std::collections::BTreeMap<u32, u32>,
    log: AccessLog,
    log_capacity: usize,
}

impl DdrCtrl {
    /// Create a DDR controller block sharing `log`.
    pub fn new(name: &str, log: AccessLog) -> Self {
        Self {
            name: name.to_string(),
            regs: std::collections::BTreeMap::new(),
            log,
            log_capacity: 16_384,
        }
    }

    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.remove(0);
        }
        log.push((offset, size, value, write));
    }
}

impl Device for DdrCtrl {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        let mut v = self.regs.get(&offset).copied().unwrap_or(0);
        match offset {
            0x04 => v |= 0x4,       // +0x1504 bit 2: hw ready (H6)
            0x100 => v |= 0x1,      // +0x1600 bit 0: ready (H6)
            0x128 => v |= 0x6_0000, // +0x1628 bits 17/18: ack + init done (H6)
            _ => {}
        }
        self.record(offset, size, v, false);
        v
    }

    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32) {
        self.record(offset, size, value, true);
        self.regs.insert(offset, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::AccessSize;

    #[test]
    fn logging_stub_records() {
        let mut d = LoggingDevice::new("test", 0xFFFF_FFFF);
        assert_eq!(d.read(0x210, AccessSize::Word), 0xFFFF_FFFF);
        d.write(0x210, AccessSize::Word, 0x380);
        let log = d.log_handle();
        let log = log.borrow();
        assert_eq!(log.len(), 2);
        assert!(!log[0].3 && log[1].3);
        assert_eq!(log[1].2, 0x380);
    }

    #[test]
    fn uart_transmits_and_reports_ready() {
        let out = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut uart = Uart::new("uart", std::rc::Rc::clone(&out), std::rc::Rc::clone(&log));
        let status = uart.read(0x004, AccessSize::Word);
        assert_eq!(status & 0x1, 0x1, "TX ready must be set");
        assert_eq!(
            status & 0x40,
            0x40,
            "TX idle must be set (main fw polls bit 6)"
        );
        uart.write(0x000, AccessSize::Byte, u32::from(b'Z'));
        uart.write(0x000, AccessSize::Byte, u32::from(b'!'));
        assert_eq!(*out.borrow(), b"Z!".to_vec());
    }

    #[test]
    fn param_ram_roundtrip() {
        let mut d = ParamRam::new("p", 0x100, 0xFF);
        assert_eq!(d.read(0x10, AccessSize::Word), 0xFFFF_FFFF);
        d.write(0x10, AccessSize::Word, 0x1234_5678);
        assert_eq!(d.read(0x10, AccessSize::Word), 0x1234_5678);
        assert_eq!(d.read(0x10, AccessSize::Byte), 0x78);
        assert_eq!(d.read(0x13, AccessSize::Byte), 0x12);
        let log = d.log_handle();
        assert_eq!(log.borrow().len(), 5, "every access must be recorded");
    }

    #[test]
    fn timer_counts_and_is_software_resettable() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut t = TimerCounter::new("timer", std::rc::Rc::clone(&log));
        assert_eq!(t.read(0x00, AccessSize::Word), 0);
        t.tick();
        t.tick();
        t.tick();
        assert_eq!(t.read(0x00, AccessSize::Word), 3, "one count per tick");
        // delay() semantics: software reset then poll to target.
        t.write(0x00, AccessSize::Word, 0);
        t.tick();
        assert_eq!(t.read(0x00, AccessSize::Word), 1);
        // Config registers store values (+0x60C -> offset 0x0C).
        t.write(0x0C, AccessSize::Word, 0x8095);
        assert_eq!(t.read(0x0C, AccessSize::Word), 0x8095);
    }

    #[test]
    fn spi_controller_matches_loader_protocol() {
        // Reproduce the loader's exact transaction (hypothesis H3):
        // TX=0x0604 -> +0x3E0C, start 0x20 -> +0x3E04, poll +0x3E08 bit
        // 0x400, write 0 to clear, read +0x3E14 == 6.
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut spi = SpiController::new("spi", std::rc::Rc::clone(&log));
        spi.write(0x0C, AccessSize::Word, 0x0604);
        assert_eq!(spi.read(0x08, AccessSize::Word), 0, "not done before start");
        spi.write(0x04, AccessSize::Word, 0x20);
        assert_eq!(
            spi.read(0x08, AccessSize::Word) & 0x400,
            0x400,
            "done after start"
        );
        spi.write(0x08, AccessSize::Word, 0); // loader clears with 0
        assert_eq!(spi.read(0x08, AccessSize::Word), 0);
        assert_eq!(spi.read(0x14, AccessSize::Byte), 6, "RX must echo TX[15:8]");
        // Alt variant: start 0x40 -> done bit 0x800 (observed at 0xB800CE4C).
        spi.write(0x04, AccessSize::Word, 0x40);
        assert_eq!(spi.read(0x08, AccessSize::Word) & 0x800, 0x800);
        spi.write(0x08, AccessSize::Word, 0);
        // CS config is captured.
        spi.write(0x24, AccessSize::Word, 0xBFFE_0006);
        assert_eq!(spi.read(0x24, AccessSize::Word), 0xBFFE_0006);
        assert!(!log.borrow().is_empty());
    }
}
