//! Memory-mapped device models.
//!
//! Devices receive offsets relative to their region base and serve
//! byte/half/word accesses. Unimplemented behavior must be *visible*:
//! stubs log their accesses instead of silently pretending to work
//! (project rule: never fake a functioning peripheral).

/// Shared device access log: (offset, size, value, was_write). A capped
/// ring (oldest entries dropped at capacity) — Milestone V switched the
/// backing store from `Vec` to `VecDeque` so the per-access record is O(1)
/// instead of an O(n) shift at capacity (§24).
pub type AccessLog = std::rc::Rc<
    std::cell::RefCell<std::collections::VecDeque<(u32, crate::memory::AccessSize, u32, bool)>>,
>;

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
            log: std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new())),
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
            log.pop_front();
        }
        log.push_back((offset, size, value, write));
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
            log: std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new())),
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
            log.pop_front();
        }
        log.push_back((offset, size, value, write));
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
            log.pop_front();
        }
        log.push_back((offset, size, value, write));
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
            log.pop_front();
        }
        log.push_back((offset, size, value, write));
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
            log.pop_front();
        }
        log.push_back((offset, size, value, write));
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
    /// Shared counter (Milestone Z, §28): the machine run loop increments
    /// it directly once per step, so the per-instruction cost is one Cell
    /// add instead of the tickable-fan-out trait dispatch. Semantics
    /// unchanged: one count per executed instruction.
    count: std::rc::Rc<std::cell::Cell<u32>>,
    config: [u32; 7], // +0x04 .. +0x1C
    log: AccessLog,
    log_capacity: usize,
}

impl TimerCounter {
    /// Create a timer block sharing `log` (own counter cell).
    pub fn new(name: &str, log: AccessLog) -> Self {
        let count = std::rc::Rc::new(std::cell::Cell::new(0u32));
        Self::with_count(name, count, log)
    }

    /// Create a timer block whose counter cell is shared with the machine
    /// run loop (the machine increments it once per executed instruction).
    pub fn with_count(
        name: &str,
        count: std::rc::Rc<std::cell::Cell<u32>>,
        log: AccessLog,
    ) -> Self {
        Self {
            name: name.to_string(),
            count,
            config: [0; 7],
            log,
            log_capacity: 16_384,
        }
    }

    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.pop_front();
        }
        log.push_back((offset, size, value, write));
    }
}

impl Device for TimerCounter {
    fn name(&self) -> &str {
        &self.name
    }

    fn tick(&mut self) {
        self.count.set(self.count.get().wrapping_add(1));
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        let v = if offset == 0 {
            self.count.get()
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
            self.count.set(value);
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
            log.pop_front();
        }
        log.push_back((offset, size, value, write));
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
/// The block is exposed as **two non-overlapping regions** that share one
/// register file: `ddrctrl` (0x1500..0x1580, local offset = absolute) and
/// `ddrctrl-hi` (0x15E0..0x16A0, absolute = local + 0xE0, skipping the PHY
/// window). Both record into the shared "ddrctrl" access log with absolute
/// offsets, so logs are byte-identical to the pre-split single-region
/// device (§31: the split exists because the Milestone V hot-region memo
/// requires non-overlapping regions; nested-region first-match routing was
/// unsound under the memo).
#[derive(Debug)]
pub struct DdrCtrlShared {
    regs: std::collections::BTreeMap<u32, u32>,
}

#[derive(Debug)]
pub struct DdrCtrl {
    name: String,
    /// Absolute-offset shift for this window: 0 for the low region,
    /// 0xE0 for the high region (skipping the PHY window 0x1580..0x15E0).
    offset_base: u32,
    shared: std::rc::Rc<std::cell::RefCell<DdrCtrlShared>>,
    log: AccessLog,
    log_capacity: usize,
}

impl DdrCtrlShared {
    fn read_regs(&self, offset: u32) -> u32 {
        let mut v = self.regs.get(&offset).copied().unwrap_or(0);
        match offset {
            0x04 => v |= 0x4,       // +0x1504 bit 2: hw ready (H6)
            0x100 => v |= 0x1,      // +0x1600 bit 0: ready (H6)
            0x128 => v |= 0x6_0000, // +0x1628 bits 17/18: ack + init done (H6)
            _ => {}
        }
        v
    }

    fn write_regs(&mut self, offset: u32, value: u32) {
        self.regs.insert(offset, value);
    }
}

impl DdrCtrl {
    /// Create the low DDR controller window (absolute offsets 0x000..0x080).
    /// Returns the device plus the shared register file for the high window.
    pub fn new(
        name: &str,
        log: AccessLog,
    ) -> (Self, std::rc::Rc<std::cell::RefCell<DdrCtrlShared>>) {
        let shared = std::rc::Rc::new(std::cell::RefCell::new(DdrCtrlShared {
            regs: std::collections::BTreeMap::new(),
        }));
        (
            Self {
                name: name.to_string(),
                offset_base: 0,
                shared: std::rc::Rc::clone(&shared),
                log,
                log_capacity: 16_384,
            },
            shared,
        )
    }

    /// Create the high DDR controller window (`offset_base` = 0xE0) attached
    /// to the shared register file.
    pub fn with_shared(
        name: &str,
        offset_base: u32,
        shared: std::rc::Rc<std::cell::RefCell<DdrCtrlShared>>,
        log: AccessLog,
    ) -> Self {
        Self {
            name: name.to_string(),
            offset_base,
            shared,
            log,
            log_capacity: 16_384,
        }
    }

    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.pop_front();
        }
        log.push_back((offset, size, value, write));
    }
}

impl Device for DdrCtrl {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        let abs = self.offset_base + offset;
        let v = self.shared.borrow().read_regs(abs);
        self.record(abs, size, v, false);
        v
    }

    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32) {
        let abs = self.offset_base + offset;
        self.record(abs, size, value, true);
        self.shared.borrow_mut().write_regs(abs, value);
    }
}

/// Interrupt controller block in the 0xBFFE0000 system page
/// (+0x480..+0x4C0), reverse-engineered from the main firmware's "IntCtrl"
/// init function at DRAM 0x82000E80 (docs/firmware-analysis.md §11).
///
/// **Verified by execution** (the program ran under the emulator and its
/// writes were confirmed against the final register state of a real boot):
///
/// | offset | init value | written by |
/// |--------|-----------|------------|
/// | +0x480 | 0xFFFFFFFF | `sw -1, -0x7B80($s6)` |
/// | +0x484 | 0xFFFFFFFF | `sw -1, -0x7B7C($s6)` |
/// | +0x488 | 0x00001F2F | `sw 0x1F2F, -0x7B78($s6)` |
/// | +0x48C | 0x00000007 | `sw 7, -0x7B74($s6)` |
/// | +0x490 | 0xFFFFFFFF | `sw -1, -0x7B70($s6)` |
/// | +0x494 | 0xFFFFFFFF | `sw -1, -0x7B6C($s6)` |
/// | +0x498 | 0x0000017C | `sw 0x17C, -0x7B68($s6)` |
/// | +0x49C..+0x4B0 | 0 | zero stores |
///
/// **Not observed** (and therefore NOT modeled): pending/ack register
/// semantics, per-source vector mapping, priority. The firmware's observed
/// boot never enables CPU interrupts (COP0 Status.IE stays 0; see §11), so
/// no dispatch code ever executed and no register in this block was ever
/// read in the traced horizon.
///
/// Emulation model (hypothesis **H11**, deliberately minimal and
/// level-triggered):
///
/// - All eight word registers are plain storage (matches the verified init
///   program exactly; a wrong guess would have shown up as a boot
///   divergence, which did not happen).
/// - External interrupt sources are injected through a shared 32-bit latch
///   ([`IntController::sources_handle`]). A source is "enabled" when its
///   bit is set in `+0x480 | +0x484` (the all-ones init values are
///   consistent with an unmask-all program).
/// - The aggregate output line (shared [`Cell<u8>`]) asserts while any
///   raised source is enabled. The machine wires it to CPU interrupt input
///   IP2 (mapping hypothesis). Level-triggered: no latch, no ack semantics
///   invented.
///
/// Related verified registers outside this block (modeled as plain param
/// RAM, no behavioral effect): 0xBFFE0014 bits 1:0 = CPU-line
/// enable/disable/query primitive (`|=3` / `&=~3` / read — function at
/// 0x8200117C, called with "disable" during IntCtrl init); 0xBFFE0B24 bit 6
/// set immediately before the COP0 Cause/Status programming.
#[derive(Debug)]
pub struct IntController {
    name: String,
    regs: std::collections::BTreeMap<u32, u32>,
    sources: std::rc::Rc<std::cell::Cell<u32>>,
    out: std::rc::Rc<std::cell::Cell<u8>>,
    log: AccessLog,
    log_capacity: usize,
    /// Sources value seen by the last tick (Milestone Z, §28): the
    /// per-step recompute becomes a one-compare change-detection — the
    /// output is a pure function of (sources, enable registers), and
    /// the registers already recompute on every write, so only a
    /// sources change between steps needs a re-evaluation.
    last_sources: u32,
}

impl IntController {
    /// Create the interrupt controller. `sources` is the shared external
    /// source latch; `out` is the aggregate output line wired to the CPU.
    pub fn new(
        name: &str,
        sources: std::rc::Rc<std::cell::Cell<u32>>,
        out: std::rc::Rc<std::cell::Cell<u8>>,
        log: AccessLog,
    ) -> Self {
        Self {
            name: name.to_string(),
            regs: std::collections::BTreeMap::new(),
            sources,
            out,
            log,
            log_capacity: 16_384,
            // u32::MAX forces the first tick to recompute (identical
            // observable: out is already 0 and stays 0 for empty regs).
            last_sources: u32::MAX,
        }
    }

    /// Shared handle to raise/clear external interrupt sources
    /// (bit i = source i; source i enabled when set in +0x480 | +0x484).
    pub fn sources_handle(&self) -> std::rc::Rc<std::cell::Cell<u32>> {
        std::rc::Rc::clone(&self.sources)
    }

    /// Shared handle to the aggregate output line (0/1; wired to IP2).
    pub fn out_handle(&self) -> std::rc::Rc<std::cell::Cell<u8>> {
        std::rc::Rc::clone(&self.out)
    }

    /// Recompute the output line from sources and enable registers.
    /// Level-triggered: `out = (sources & (en_a | en_b)) != 0`.
    fn update_output(&self) {
        let en =
            self.regs.get(&0x00).copied().unwrap_or(0) | self.regs.get(&0x04).copied().unwrap_or(0);
        let active = self.sources.get() & en != 0;
        self.out.set(u8::from(active));
    }
}

impl Device for IntController {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        let v = self.regs.get(&offset).copied().unwrap_or(0);
        self.record(offset, size, v, false);
        v
    }

    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32) {
        self.record(offset, size, value, true);
        self.regs.insert(offset, value);
        self.update_output();
    }

    /// Re-evaluate the output line when the external sources changed
    /// since the last step (Milestone Z, §28). The line is a pure
    /// function of (sources, enable registers); enable-register writes
    /// recompute immediately in `write`, so a per-step check needs only
    /// one sources read + compare — the boot never raises sources, so
    /// its two BTreeMap lookups per instruction were pure overhead.
    fn tick(&mut self) {
        let s = self.sources.get();
        if s != self.last_sources {
            self.last_sources = s;
            self.update_output();
        }
    }
}

impl IntController {
    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.pop_front();
        }
        log.push_back((offset, size, value, write));
    }
}

/// Hardware decompression engine (hypothesis H12), registers at
/// sysreg+0x3800..0x3878 (0xBFFE3800..0xBFFE3878).
///
/// Evidence (docs/firmware-analysis.md §13; the "LD eCos" boot stage):
///
/// * the loader programs source 0xBFFE3804/0808 = 0x0208_8000 (the copied
///   compressed image), a 32 KiB zero-filled window base 0xBFFE385C/60 =
///   0x0204_0000 with end 0xBFFE3850/54 = 0x0204_8000, an output
///   0xBFFE386C/70 = 0x0160_0000, then table-upload commands on
///   0xBFFE3844 with data bytes on 0xBFFE3848, and finally starts the
///   engine by writing 0xBFFE3824 = 1;
/// * it then polls 0xBFFE384C **bit 5** (the engine event line, §13.3) —
///   the poll loop is the same loop that scans front-panel keys, so keys
///   stay live while the engine runs;
/// * when bit 5 reads 1 the driver acks (writes 0x2000), counts the event
///   against the container's chunk limit and, at the limit, enters the
///   accounting block and returns non-zero (success). The bit-6 branch of
///   the same poll is the *failure* epilogue (returns 0 → "LD eCos fail").
///
/// The compressed image itself is a raw DEFLATE stream behind an 8-byte
/// "SP" header (verified: the header parse the firmware performs produces
/// exactly the Huffman tables it uploads — docs/firmware-analysis.md §13).
///
/// Model: register storage + start/busy/done handshake + a real DEFLATE
/// decode performed by the machine run loop
/// ([`EngineShared::take_request`](EngineShared::take_request) →
/// `crate::inflate::inflate`). The command/data table uploads are recorded
/// but not interpreted: the decode re-derives equivalent tables from the
/// stream itself (the RFC 1951 canonical codes of the stream's lengths
/// equal the uploaded arrays bit-for-bit). The done flag is shared with
/// the [`KeyPort`] so bit 5 of 0xBFFE384C reads 1 after completion.
#[derive(Debug)]
pub struct EngineShared {
    /// Register file covering sysreg+0x3800..0x3878 (little-endian bytes).
    ///
    /// The block is a PNG decoder; register names follow the SPHE15xx
    /// platform header (`regfile.h`, linux-sunplus-15xx), see
    /// docs/firmware-analysis.md §17:
    ///
    /// | off   | kernel name            | firmware program              |
    /// |-------|------------------------|-------------------------------|
    /// | 0x00  | PNG_DEC_MODE           | 1 then 0x90                   |
    /// | 0x04/08 | PNG_BS_STR_ADDR_M/L  | bitstream base (0x02088000)   |
    /// | 0x0C/10 | PNG_BS_END_ADDR_M/L  | round128(base+stream_len+8)   |
    /// | 0x14  | PNG_BS_BUF_ADDR        | 0                             |
    /// | 0x18  | PNG_BS_OFFSET          | start bit (1088)              |
    /// | 0x1C  | PNG_BS_USED_CNT        | engine-internal (completion)  |
    /// | 0x20  | PNG_BS_USED_BIT        | engine-internal (completion)  |
    /// | 0x24  | PNG_DEC_CTRL           | start bit 0 / ready bit 1     |
    /// | 0x28  | PNG_IMG_WIDTH          | 0x400 (driver constant)       |
    /// | 0x2C  | PNG_IMG_HEIGHT         | container u16@2 (78)          |
    /// | 0x30  | PNG_IMG_TYPE           | 0x68 (driver constant)        |
    /// | 0x34/38 | PNG_CHUNK_SIZE_M/L   | stream_len+8+(len&1)+6        |
    /// | 0x3C  | PNG_BLK_INFO           | (btype<<1)\|bfinal = 5        |
    /// | 0x40  | PNG_HUF_TB_ADDR        | 0x20                          |
    /// | 0x44/48 | PNG_HUF_TB_CTRL/DATA | table upload FIFO             |
    /// | 0x4C  | PNG_INTERRUPT          | event/ack line (§34.1: bit 0
    ///       bitstream-empty, 2 end-of-chunk, 4 end-of-image, 5
    ///       pixel-ready, 6 error; ack = bit<<8) |
    /// | 0x50/54 | PNG_SCL_EXT_ADDR_M/L | scaling buffer 0x02048000     |
    /// | 0x58  | PNG_SCL_BUF_ADDR       | 0xC0                          |
    /// | 0x5C/60 | PNG_BW_EXT_ADDR_M/L  | band buffer 0x02040000 (32K)  |
    /// | 0x64  | PNG_REM_CHUNK_SIZE_M   | 1                             |
    /// | 0x6C/70 | PNG_PXL_EXT_ADDR_M/L | pixel output (0x01600000)     |
    /// | 0x74  | PNG_PXL_LINE           | row countdown preload (77)    |
    pub regs: Vec<u8>,
    /// Pending decompression request (source phys, dest phys).
    pub request: Option<(u32, u32)>,
    /// True between the start write and the machine completing the decode.
    pub busy: bool,
    /// Completion flag (read as bit 5 of 0xBFFE384C via [`KeyPort`]).
    pub done: std::rc::Rc<std::cell::Cell<u8>>,
    /// Decode result for diagnostics: bytes written or an error string.
    pub last_result: Option<Result<usize, String>>,
    /// The exact stream bytes the last serviced decode consumed
    /// (post-"SP"-header, post input-length clamp, §36): captured at
    /// service time so the post-boot replay instrument
    /// ([`crate::machine::Machine::engine_decode_inputs`]) is byte-exact
    /// regardless of later DRAM writes. One container per observed boot.
    pub last_stream: Option<Vec<u8>>,
    /// The start bit the last serviced decode ran with (post-clamp, the
    /// value actually handed to the decoder).
    pub last_start_bit: Option<u32>,
    /// The last serviced decode's own output length — the pre-PNG-row
    /// decode length, before the machine's truncate/zero-fill to the
    /// programmed W×H (§17). Window bytes at or above this index are the
    /// zero-filled band region, not decode output.
    pub last_decoded_len: Option<usize>,
    /// The band-window preset content the last serviced decode used
    /// (§44): `None` = the documented zero fill ([`crate::machine::
    /// ENGINE_PRESET_BYTES`], §13/§18.3 — the default); `Some(v)` = the
    /// `MachineConfig::preset_window` seam value, byte-for-byte. Captured
    /// at service time so the §36 replay is exact under any preset.
    pub last_preset: Option<Vec<u8>>,
    /// How many bytes of source to feed the decoder.
    pub input_len: u32,
    /// Borrow-free request hint (Milestone Z, §28): set when a start
    /// write posts a request, cleared by [`Self::take_request`]. Lives
    /// outside the `RefCell` so the per-step machine poll is one
    /// `Cell::get` instead of a `borrow_mut` on every instruction.
    pub pending: std::rc::Rc<std::cell::Cell<bool>>,
}

impl EngineShared {
    fn new(
        done: std::rc::Rc<std::cell::Cell<u8>>,
    ) -> std::rc::Rc<std::cell::RefCell<EngineShared>> {
        std::rc::Rc::new(std::cell::RefCell::new(EngineShared {
            regs: vec![0; 0x78],
            request: None,
            busy: false,
            done,
            last_result: None,
            last_stream: None,
            last_start_bit: None,
            last_decoded_len: None,
            last_preset: None,
            input_len: 0,
            pending: std::rc::Rc::new(std::cell::Cell::new(false)),
        }))
    }

    /// Read a little-endian word at register offset `off` (block-relative).
    /// Exposed to the machine service loop, which acts on the registers
    /// the firmware programmed (start bit, input length).
    pub fn reg(&self, off: usize) -> u32 {
        u32::from_le_bytes([
            self.regs[off],
            self.regs[off + 1],
            self.regs[off + 2],
            self.regs[off + 3],
        ])
    }

    /// Machine-side hook: pop a pending (source, dest) request.
    pub fn take_request(&mut self) -> Option<(u32, u32)> {
        if self.busy {
            let r = self.request.take();
            // The hint means "a request is waiting"; after the take the
            // queue is empty either way.
            self.pending.set(false);
            r
        } else {
            None
        }
    }

    /// Machine-side completion: record the result and set the done line.
    pub fn complete(&mut self, written: usize, err: Option<String>) {
        self.busy = false;
        self.last_result = Some(match err {
            Some(e) => Err(e),
            None => Ok(written),
        });
        self.done.set(1);
    }

    /// Machine-side engine-internal register write (e.g. the completion
    /// accounting feeds PNG_BS_USED_CNT, 0xBFFE381C, §17).
    pub fn set_reg(&mut self, off: usize, value: u32) {
        if off + 4 <= self.regs.len() {
            self.regs[off..off + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    fn read_regs(&self, off: u32, size: crate::memory::AccessSize) -> u32 {
        let o = off as usize;
        if o + 4 > self.regs.len() {
            return 0;
        }
        match size {
            crate::memory::AccessSize::Byte => u32::from(self.regs[o]),
            crate::memory::AccessSize::Half => {
                u32::from(u16::from_le_bytes([self.regs[o], self.regs[o + 1]]))
            }
            crate::memory::AccessSize::Word => self.reg(o),
        }
    }

    fn write_regs(&mut self, off: u32, size: crate::memory::AccessSize, value: u32) {
        let o = off as usize;
        if o + size.bytes() as usize > self.regs.len() {
            return;
        }
        let b = value.to_le_bytes();
        let n = size.bytes() as usize;
        self.regs[o..o + n].copy_from_slice(&b[..n]);
    }
}

/// Low half of the engine register block: sysreg+0x3800..0x384C. Hosts the
/// start register (0x3824) and the command/data FIFO (0x3844/0x3848).
#[derive(Debug)]
pub struct DecompEngineLo {
    name: String,
    shared: std::rc::Rc<std::cell::RefCell<EngineShared>>,
    log: AccessLog,
    log_capacity: usize,
}

/// High half of the engine register block: sysreg+0x3850..0x3878. Hosts the
/// window base/end (0x3850/54, 0x385C/60) and output address (0x386C/70).
#[derive(Debug)]
pub struct DecompEngineHi {
    name: String,
    shared: std::rc::Rc<std::cell::RefCell<EngineShared>>,
    log: AccessLog,
    log_capacity: usize,
}

impl DecompEngineLo {
    /// Create the low engine region. Returns the device plus the shared
    /// state handle for the [`Machine`](crate::machine::Machine) run loop.
    /// `done` is the completion line shared with the [`KeyPort`].
    pub fn new(
        name: &str,
        done: std::rc::Rc<std::cell::Cell<u8>>,
        log: AccessLog,
    ) -> (Self, std::rc::Rc<std::cell::RefCell<EngineShared>>) {
        let shared = EngineShared::new(done);
        (
            Self {
                name: name.to_string(),
                shared: std::rc::Rc::clone(&shared),
                log,
                log_capacity: 16_384,
            },
            shared,
        )
    }

    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.pop_front();
        }
        log.push_back((offset, size, value, write));
    }
}

impl DecompEngineHi {
    /// Create the high engine region attached to `shared`.
    pub fn with_shared(
        name: &str,
        shared: std::rc::Rc<std::cell::RefCell<EngineShared>>,
        log: AccessLog,
    ) -> Self {
        Self {
            name: name.to_string(),
            shared,
            log,
            log_capacity: 16_384,
        }
    }

    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.pop_front();
        }
        log.push_back((offset, size, value, write));
    }
}

impl Device for DecompEngineLo {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        let sh = self.shared.borrow();
        let v = sh.read_regs(offset, size);
        drop(sh);
        self.record(offset, size, v, false);
        v
    }

    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32) {
        self.record(offset, size, value, true);
        let mut sh = self.shared.borrow_mut();
        sh.write_regs(offset, size, value);
        // Start bit: writing 0xBFFE3824 with bit 0 set launches the decode.
        // The engine then runs autonomously (DMA-style); the run loop does
        // the work and completion raises the shared done line.
        if offset == 0x24 && size == crate::memory::AccessSize::Word && value & 1 == 1 {
            // Address registers hold hi/lo halves: src = 0x3804:0x3808,
            // dst = 0x386C:0x3870 (observed 0x0208/0x8000 -> 0x0208_8000).
            let src = ((sh.reg(0x04) << 16) | sh.reg(0x08)) & 0x1FFF_FFFF;
            let dst = ((sh.reg(0x6C) << 16) | sh.reg(0x70)) & 0x1FFF_FFFF;
            let ilen = sh.reg(0x38);
            sh.input_len = ilen;
            sh.request = Some((src, dst));
            sh.busy = true;
            sh.pending.set(true);
            sh.done.set(0);
        }
    }
}

impl Device for DecompEngineHi {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        let sh = self.shared.borrow();
        let v = sh.read_regs(offset + 0x50, size);
        drop(sh);
        self.record(offset, size, v, false);
        v
    }

    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32) {
        self.record(offset, size, value, true);
        let mut sh = self.shared.borrow_mut();
        sh.write_regs(offset + 0x50, size, value);
    }
}

/// Register 0xBFFE384C (4 bytes) — vendor-named **PNG_DEC_INTERRUPT**
/// (Milestone AG, docs/firmware-analysis.md §34.1), first reverse-
/// engineered from the main firmware's service dispatcher
/// (docs/firmware-analysis.md §10.3, dispatch loop at DRAM
/// 0x82002548..0x82002630).
///
/// **Verified by execution + disassembly** ($s6 = 0xBFFE8000, so
/// `$s6 - 0x47B4` = 0xBFFE384C is loaded into `$a1` at 0x82002504):
///
/// - READ (`lw $v0, ($a1)`, masked to bits 15:0): the dispatcher tests
///   bits **0, 2, 4, 5, 6** (`andi/ext` sequences). Steady state reads 0
///   — matches the post-boot register dump. Under the vendor interrupt
///   map these are exactly the PNG_DEC interrupt lines: bit 0 =
///   bitstream-empty, bit 2 = end-of-chunk, bit 4 = end-of-image, bit 5
///   = pixel-buffer-ready (the engine event line, §13.3), bit 6 =
///   decode-error.
/// - WRITE (`sw $tX, ($a1)`): the firmware's **acknowledge** writes —
///   bit 0 → 0x100, bit 2 → 0x400, bit 4 → 0x1000, bit 5 → 0x2000 —
///   the vendor drivers clear the same bits with the same `bit << 8`
///   encoding (§34.1). Bit 6 takes a separate long path (via
///   0xBFFE3824 = PNG_DEC_CTRL → 0x82002308): the vendor error handler
///   reads PNG_DEC_CTRL to name the error type.
///
/// The §10.3 "front-panel key port / keycode" reading of this traffic is
/// thereby re-attributed: the tested bits are engine interrupt lines and
/// the written "keycodes" are their acks — which is also the structural
/// explanation of §23.3's negative keycode-consumer verdict (no code
/// consumes keycodes because the dispatcher produces none). The raw
/// low-bit driver source on the 1506HV (real front-panel GPIOs, if any
/// are muxed here) is unknown; the level model below is retained as the
/// documented falsifiable choice for those bits. All observed traffic is
/// engine-side and identical under both readings.
///
/// Emulation model (evidence-complete for the observed accesses):
/// - Reads return the currently **pressed** key-line level from a shared
///   handle — level semantics: a "button press" is injected by setting
///   bits and cleared by releasing them (see
///   [`Machine::press_key`](crate::machine::Machine::press_key)).
/// - Writes are captured verbatim into a shared **event queue**. Writes
///   do not modify the pressed level (falsifiable choice: a
///   latch/ack-on-write would read back 0 after a write; the level model
///   is the simpler of the two and matches all observed traffic — the
///   scan loop re-reads the same bits).
#[derive(Debug)]
pub struct KeyPort {
    name: String,
    pressed: std::rc::Rc<std::cell::Cell<u32>>,
    events: std::rc::Rc<std::cell::RefCell<Vec<u32>>>,
    /// Engine event line (decompression engine, §13.3): read as bit 5 of
    /// this register — vendor-named the pixel-buffer-ready interrupt
    /// (§34.1). 0xBFFE384C = PNG_DEC_INTERRUPT; the ack write 0x2000 is
    /// that bit's vendor encoding (`bit << 8`). Driver evidence: the
    /// poll's bit-5 branch acks, counts the event and routes to the
    /// success block; the bit-6 branch is the error epilogue.
    engine_done: std::rc::Rc<std::cell::Cell<u8>>,
    log: AccessLog,
    log_capacity: usize,
}

impl KeyPort {
    /// Create the key port. `pressed` = raw key-line level (bit 0/2/4/5/6
    /// observed); `events` receives every keycode the firmware writes;
    /// `engine_done` is the decompression-engine event flag that ORs into
    /// bit 5 of reads (docs/firmware-analysis.md §13.3 — the driver's
    /// success path counts bit-5 events; its bit-6 path is the failure
    /// epilogue that returns 0).
    pub fn new(
        name: &str,
        pressed: std::rc::Rc<std::cell::Cell<u32>>,
        events: std::rc::Rc<std::cell::RefCell<Vec<u32>>>,
        engine_done: std::rc::Rc<std::cell::Cell<u8>>,
        log: AccessLog,
    ) -> Self {
        Self {
            name: name.to_string(),
            pressed,
            events,
            engine_done,
            log,
            log_capacity: 4096,
        }
    }

    /// Shared handle to the pressed-key level (bit 0/2/4/5/6 meaningful).
    pub fn pressed_handle(&self) -> std::rc::Rc<std::cell::Cell<u32>> {
        std::rc::Rc::clone(&self.pressed)
    }

    /// Shared handle to the key event queue (firmware-written keycodes).
    pub fn events_handle(&self) -> std::rc::Rc<std::cell::RefCell<Vec<u32>>> {
        std::rc::Rc::clone(&self.events)
    }
}

impl Device for KeyPort {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&mut self, offset: u32, size: crate::memory::AccessSize) -> u32 {
        // All observed accesses are word-wide; byte/half views return the
        // corresponding little-endian slice of the same level. Bit 5 ORs in
        // the decompression engine's event line (§13.3: the driver's poll
        // takes the bit-5 branch to its success block; the bit-6 branch is
        // the failure epilogue).
        let done = u32::from(self.engine_done.get());
        let w = self.pressed.get() | (done << 5);
        let off = offset & 3;
        let v = match size {
            crate::memory::AccessSize::Byte => (w >> (8 * off)) & 0xFF,
            crate::memory::AccessSize::Half => {
                if off == 0 {
                    w & 0xFFFF
                } else if off == 2 {
                    w >> 16
                } else {
                    0
                }
            }
            crate::memory::AccessSize::Word => w,
        };
        self.record(offset, size, v, false);
        v
    }

    fn write(&mut self, offset: u32, size: crate::memory::AccessSize, value: u32) {
        self.record(offset, size, value, true);
        // Observed writes are the firmware's keycode acknowledgment
        // (word-wide). Capture them as key events. A write carrying the
        // 0x2000 code (key line 5 / engine event ack, §13.3) also clears
        // the engine's event line — the driver's bit-5 handler writes
        // exactly this value before counting the event.
        if offset == 0 && size == crate::memory::AccessSize::Word {
            self.events.borrow_mut().push(value);
            if value & 0x2000 != 0 {
                self.engine_done.set(0);
            }
        }
    }
}

impl KeyPort {
    fn record(&self, offset: u32, size: crate::memory::AccessSize, value: u32, write: bool) {
        let mut log = self.log.borrow_mut();
        if log.len() >= self.log_capacity {
            log.pop_front();
        }
        log.push_back((offset, size, value, write));
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
        let log = std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new()));
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
        let log = std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new()));
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
        let log = std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new()));
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

    #[test]
    fn int_controller_replays_firmware_init_and_level_output() {
        // The verified IntCtrl init program (docs/firmware-analysis.md
        // §11): all-ones enable words + config values. After it, the
        // level output must follow raised sources.
        let sources = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let out = std::rc::Rc::new(std::cell::Cell::new(0u8));
        let log = std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new()));
        let mut intc = IntController::new(
            "intc",
            std::rc::Rc::clone(&sources),
            std::rc::Rc::clone(&out),
            std::rc::Rc::clone(&log),
        );
        // Replay the init writes (region-relative offsets: 0x480 -> 0x00).
        intc.write(0x00, AccessSize::Word, 0xFFFF_FFFF);
        intc.write(0x04, AccessSize::Word, 0xFFFF_FFFF);
        intc.write(0x08, AccessSize::Word, 0x1F2F);
        intc.write(0x0C, AccessSize::Word, 0x7);
        intc.write(0x10, AccessSize::Word, 0xFFFF_FFFF);
        intc.write(0x14, AccessSize::Word, 0xFFFF_FFFF);
        intc.write(0x18, AccessSize::Word, 0x17C);
        for off in (0x1C..0x30).step_by(4) {
            intc.write(off, AccessSize::Word, 0);
        }
        // Final state must match the observed sysreg dump exactly.
        assert_eq!(intc.read(0x00, AccessSize::Word), 0xFFFF_FFFF);
        assert_eq!(intc.read(0x08, AccessSize::Word), 0x1F2F);
        assert_eq!(intc.read(0x0C, AccessSize::Word), 0x7);
        assert_eq!(intc.read(0x18, AccessSize::Word), 0x17C);
        assert_eq!(intc.read(0x1C, AccessSize::Word), 0);

        // No sources raised: output low.
        assert_eq!(out.get(), 0);
        // Raise source 0 (enabled by the all-ones mask): tick recomputes.
        sources.set(1);
        intc.tick();
        assert_eq!(out.get(), 1, "raised + enabled source asserts the line");
        // Mask it off (both enable words must clear — the model treats
        // +0x480 | +0x484 as one enable space): line drops.
        intc.write(0x00, AccessSize::Word, 0);
        intc.write(0x04, AccessSize::Word, 0);
        assert_eq!(out.get(), 0, "masked source deasserts the line");
        intc.write(0x00, AccessSize::Word, 0xFFFF_FFFF);
        assert_eq!(out.get(), 1, "unmasking re-asserts (level semantics)");
        // Clear the source: line drops without any ack write.
        sources.set(0);
        intc.tick();
        assert_eq!(out.get(), 0);
        // Every access was recorded.
        assert!(!log.borrow().is_empty());
    }

    #[test]
    fn key_port_level_read_and_event_writes() {
        let pressed = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let done = std::rc::Rc::new(std::cell::Cell::new(0u8));
        let log = std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new()));
        let mut kp = KeyPort::new(
            "keyport",
            std::rc::Rc::clone(&pressed),
            std::rc::Rc::clone(&events),
            std::rc::Rc::clone(&done),
            std::rc::Rc::clone(&log),
        );
        // No key, engine idle: reads 0 (steady state of the real boot).
        assert_eq!(kp.read(0, AccessSize::Word), 0);
        // Press the bit-2 key line: the dispatcher's `ext v0, v0, 2, 1`
        // sees 1. The firmware answers by writing keycode 0x400.
        pressed.set(0b0100);
        assert_eq!(kp.read(0, AccessSize::Word), 0b0100);
        kp.write(0, AccessSize::Word, 0x400);
        // Level semantics: the keycode write does not change the pressed
        // level (the scan loop re-reads the same bits while held).
        assert_eq!(kp.read(0, AccessSize::Word), 0b0100);
        // Release: reads 0 again.
        pressed.set(0);
        assert_eq!(kp.read(0, AccessSize::Word), 0);
        // The keycode was captured as an event.
        assert_eq!(*events.borrow(), vec![0x400]);
        // Engine event ORs into bit 5 (§13.3: the driver's poll takes the
        // bit-5 branch to its success accounting block; the bit-6 branch
        // is the failure epilogue).
        done.set(1);
        assert_eq!(kp.read(0, AccessSize::Word), 0b0010_0000);
        assert_eq!(kp.read(0, AccessSize::Byte), 0x20);
        // The firmware acks the event by writing keycode 0x2000; the event
        // line clears on that write.
        kp.write(0, AccessSize::Word, 0x2000);
        assert_eq!(kp.read(0, AccessSize::Word), 0);
        assert_eq!(*events.borrow(), vec![0x400, 0x2000]);
        done.set(0);
        // Byte/half views of the level (LE slices).
        pressed.set(0x0402);
        assert_eq!(kp.read(0, AccessSize::Byte), 0x02);
        assert_eq!(kp.read(1, AccessSize::Byte), 0x04);
        assert_eq!(kp.read(0, AccessSize::Half), 0x0402);
        assert!(!log.borrow().is_empty());
    }

    #[test]
    fn decomp_engine_start_request_and_done_handshake() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new()));
        let done = std::rc::Rc::new(std::cell::Cell::new(0u8));
        let (mut lo, shared) = DecompEngineLo::new(
            "decomp",
            std::rc::Rc::clone(&done),
            std::rc::Rc::clone(&log),
        );
        // Program the source (0x02088000) and destination (0x01600000) the
        // way the firmware does, then start. 0x6C/0x70/0x38 live in the
        // shared register file (the Hi device maps them in the machine).
        lo.write(0x04, AccessSize::Word, 0x0208);
        lo.write(0x08, AccessSize::Word, 0x8000);
        shared
            .borrow_mut()
            .write_regs(0x6C, AccessSize::Word, 0x0160);
        shared
            .borrow_mut()
            .write_regs(0x70, AccessSize::Word, 0x0000);
        shared
            .borrow_mut()
            .write_regs(0x38, AccessSize::Word, 0x8692);
        // Read back through the device (register storage semantics).
        assert_eq!(lo.read(0x04, AccessSize::Word), 0x0208);
        // Busy is clear before start.
        assert_eq!(lo.read(0x24, AccessSize::Word) & 2, 0);
        // Start: write 1 to 0x24 (0xBFFE3824).
        lo.write(0x24, AccessSize::Word, 0x1);
        // The machine loop picks the request up.
        let req = shared.borrow_mut().take_request();
        assert_eq!(req, Some((0x0208_8000, 0x0160_0000)));
        assert!(shared.borrow().busy);
        // Completion: done line rises, busy clears, result recorded.
        shared.borrow_mut().complete(28256, None);
        assert!(!shared.borrow().busy);
        assert_eq!(shared.borrow().done.get(), 1);
        assert_eq!(shared.borrow().last_result, Some(Ok(28256)));
        // A second start write resets the done line.
        lo.write(0x24, AccessSize::Word, 0x1);
        assert_eq!(shared.borrow().done.get(), 0);
        assert!(shared.borrow_mut().take_request().is_some());
        assert!(!log.borrow().is_empty());
    }
}
