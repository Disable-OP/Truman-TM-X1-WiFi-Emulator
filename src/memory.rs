//! Address space: virtual→physical translation and region management.
//!
//! The emulated CPU is a MIPS32r2 core using the standard MIPS memory map
//! (no TLB translation is needed for the boot path, which runs entirely out
//! of kseg0/kseg1). Translation follows the MIPS32 specification:
//!
//! | Segment  | Range                 | Physical address      |
//! |----------|-----------------------|-----------------------|
//! | kuseg    | 0x00000000–0x7FFFFFFF | TLB-mapped (unimplemented here → fault) |
//! | kseg0    | 0x80000000–0x9FFFFFFF | `va & 0x1FFF_FFFF` (cached) |
//! | kseg1    | 0xA0000000–0xBFFFFFFF | `va & 0x1FFF_FFFF` (uncached) |
//! | kseg2/3  | 0xC0000000–0xFFFFFFFF | TLB-mapped (unimplemented here → fault) |
//!
//! Physical regions are declared per machine configuration
//! (see [`crate::machine`]) and are tagged as RAM, ROM or device-backed.

use std::cell::Cell;

use crate::devices::Device;

/// Access width used by a load/store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessSize {
    /// 1 byte.
    Byte,
    /// 2 bytes, little-endian in memory.
    Half,
    /// 4 bytes, little-endian in memory.
    Word,
}

impl AccessSize {
    /// Width in bytes.
    pub fn bytes(self) -> u32 {
        match self {
            AccessSize::Byte => 1,
            AccessSize::Half => 2,
            AccessSize::Word => 4,
        }
    }
}

/// Why a memory access failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// Instruction fetch from an unmapped address.
    Fetch,
    /// Data read from an unmapped address.
    Read,
    /// Data write to an unmapped, read-only or undecoded address.
    Write,
    /// Address was not naturally aligned for the access width.
    Alignment,
}

impl FaultKind {
    /// Short tag used in diagnostics.
    pub fn tag(self) -> &'static str {
        match self {
            FaultKind::Fetch => "fetch",
            FaultKind::Read => "read",
            FaultKind::Write => "write",
            FaultKind::Alignment => "alignment",
        }
    }
}

/// A failed memory access, with both the virtual and (attempted) physical
/// address so diagnostics can distinguish translation problems from
/// physical decode problems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessFault {
    /// Virtual address issued by the CPU.
    pub vaddr: u32,
    /// Physical address after translation, if translation succeeded.
    pub phys: Option<u32>,
    /// Kind of access that failed.
    pub kind: FaultKind,
    /// Access width.
    pub size: AccessSize,
}

impl std::fmt::Display for AccessFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} fault vaddr=0x{:08x} {} size={} phys={}",
            self.kind.tag(),
            self.vaddr,
            match self.kind {
                FaultKind::Alignment => "unaligned",
                _ => "unmapped",
            },
            self.size.bytes(),
            match self.phys {
                Some(p) => format!("0x{p:08x}"),
                None => "untranslatable".to_string(),
            }
        )
    }
}

impl std::error::Error for AccessFault {}

/// Which access kinds a watchpoint observes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchKind {
    /// Data read (`load`).
    Read,
    /// Data write (`store`).
    Write,
    /// Instruction fetch (`fetch`).
    Fetch,
}

/// Bit-set of access kinds a watchpoint fires on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchKinds(u8);

impl WatchKinds {
    /// Data reads only.
    pub const READ: Self = Self(1);
    /// Data writes only.
    pub const WRITE: Self = Self(2);
    /// Instruction fetches only.
    pub const FETCH: Self = Self(4);
    /// Reads and writes.
    pub const READ_WRITE: Self = Self(3);
    /// Reads, writes and fetches.
    pub const ALL: Self = Self(7);

    fn contains(self, kind: WatchKind) -> bool {
        let bit = match kind {
            WatchKind::Read => 1,
            WatchKind::Write => 2,
            WatchKind::Fetch => 4,
        };
        self.0 & bit != 0
    }
}

/// One recorded watchpoint hit.
#[derive(Debug, Clone)]
pub struct WatchHit {
    /// Step number (completed steps) of the executing instruction.
    pub step: u64,
    /// PC of the instruction that issued the access.
    pub pc: u32,
    /// Virtual address issued by the CPU.
    pub vaddr: u32,
    /// Physical address after translation.
    pub phys: u32,
    /// Access kind.
    pub kind: WatchKind,
    /// Access width.
    pub size: AccessSize,
    /// Value read (zero-extended) or written.
    pub value: u32,
    /// Name of the physical region backing the access.
    pub region: String,
}

/// Per-step execution context the machine stamps onto the bus so
/// watchpoint hits attribute to the issuing instruction (step, PC).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WatchCtx {
    /// Completed step count when the instruction begins.
    pub step: u64,
    /// PC of the instruction being executed.
    pub pc: u32,
}

#[derive(Debug, Clone, Copy)]
struct WatchRange {
    base: u32,
    len: u32,
    kinds: WatchKinds,
}

/// Default cap on recorded watchpoint hits (prevents unbounded growth on
/// long runs over hot ranges). Further hits only raise the overflow flag.
const WATCH_CAP_DEFAULT: usize = 1 << 20;

/// Hot-region memo size (Milestone V, §24): 4 direct-mapped slots keyed by
/// a hashed 4 KiB page. Boot execution has strong locality, so this
/// resolves the vast majority of lookups without the linear region scan.
const HOT_REGION_SLOTS: usize = 4;

/// Slot index for `phys`: 2 bits from a multiplied hash of the 4 KiB page.
/// A plain `page % 4` collides for the machine's aligned region bases
/// (every base's page number is ≡ 0 mod 4), parking all regions in one
/// slot and thrashing the memo.
#[inline]
fn hot_slot(phys: u32) -> usize {
    (((phys >> 12).wrapping_mul(0x9E37_79B1)) >> 30) as usize
}

/// Backing kind of a physical region.
enum RegionKind {
    /// Read-write memory.
    Ram(Vec<u8>),
    /// Read-only memory (writes are faults, reported with the region name).
    Rom(Vec<u8>),
    /// Delegated to a device model.
    Device(Box<dyn Device>),
}

/// A declared physical region.
struct Region {
    /// Physical base address (inclusive).
    base: u32,
    /// Length in bytes; region covers `base ..= base + len - 1`.
    len: u32,
    /// Human-readable region name (used in diagnostics).
    name: String,
    kind: RegionKind,
}

impl Region {
    fn contains(&self, phys: u32) -> bool {
        phys >= self.base && phys < self.base.saturating_add(self.len)
    }
}

/// The emulated physical address space with MIPS segment translation.
///
/// All CPU-issued addresses are virtual (kseg0/kseg1); this struct performs
/// translation and dispatches to RAM, ROM or device regions.
pub struct AddressSpace {
    regions: Vec<Region>,
    /// Hot-region memo (Milestone V, §24): per-step fetch/load/store pay a
    /// linear scan here otherwise; boot execution has strong locality (one
    /// code region, one stack window), so a 4-slot direct-mapped memo keyed
    /// by `phys >> 12` resolves most lookups with one range check. Entries
    /// store the region's (base, limit, index) and a hit re-validates the
    /// full range, so the memo is sound even if it thrashes; region layout
    /// is frozen at machine construction (see `push_region`). `Cell` lets
    /// the shared `&self` lookup path fill the memo.
    hot: [Cell<(u32, u32, u32)>; HOT_REGION_SLOTS],
    /// Indices of regions whose device implements time behavior.
    tickable: Vec<usize>,
    /// Installed watchpoints (Q8 instrument, docs/firmware-analysis.md §22).
    watches: Vec<WatchRange>,
    /// Recorded hits, capped by `watch_cap`.
    watch_hits: Vec<WatchHit>,
    /// Set when hits were dropped because the cap was reached.
    watch_overflow: bool,
    watch_cap: usize,
    /// (step, pc) of the currently executing instruction, stamped by the
    /// machine's run loop before each `CpuBackend::step`.
    pub(crate) watch_ctx: WatchCtx,
}

impl Default for AddressSpace {
    fn default() -> Self {
        Self {
            regions: Vec::new(),
            hot: std::array::from_fn(|_| Cell::new((0, 0, u32::MAX))),
            tickable: Vec::new(),
            watches: Vec::new(),
            watch_hits: Vec::new(),
            watch_overflow: false,
            watch_cap: WATCH_CAP_DEFAULT,
            watch_ctx: WatchCtx::default(),
        }
    }
}

impl AddressSpace {
    /// Create an empty address space.
    pub fn new() -> Self {
        Self::default()
    }

    /// Install a watchpoint over the physical range `base..base+len`.
    /// Recording is off until a watch exists; the per-hit cost is one
    /// range scan plus a capped push.
    pub fn add_watch(&mut self, base: u32, len: u32, kinds: WatchKinds) {
        self.watches.push(WatchRange { base, len, kinds });
    }

    /// Remove all watchpoints (recorded hits are kept).
    pub fn clear_watches(&mut self) {
        self.watches.clear();
    }

    /// True while at least one watchpoint is installed.
    pub fn has_watches(&self) -> bool {
        !self.watches.is_empty()
    }

    /// Number of recorded hits currently buffered.
    pub fn watch_hit_count(&self) -> usize {
        self.watch_hits.len()
    }

    /// True if hits were dropped because the cap was reached.
    pub fn watch_overflow(&self) -> bool {
        self.watch_overflow
    }

    /// Change the hit cap (tests use small caps to exercise the overflow
    /// path; long runs keep the default).
    pub fn set_watch_cap(&mut self, cap: usize) {
        self.watch_cap = cap;
    }

    /// Drain and return the recorded hits.
    pub fn take_watch_hits(&mut self) -> Vec<WatchHit> {
        std::mem::take(&mut self.watch_hits)
    }

    fn watch_armed(&self, kind: WatchKind, phys: u32) -> bool {
        if self.watches.is_empty() {
            return false; // hot path: no watches installed
        }
        self.watches.iter().any(|w| {
            w.kinds.contains(kind) && phys >= w.base && phys < w.base.saturating_add(w.len)
        })
    }

    fn record_watch(
        &mut self,
        kind: WatchKind,
        vaddr: u32,
        phys: u32,
        size: AccessSize,
        value: u32,
    ) {
        if self.watch_hits.len() >= self.watch_cap {
            self.watch_overflow = true;
            return;
        }
        let region = self.region_name(phys).unwrap_or("?").to_string();
        self.watch_hits.push(WatchHit {
            step: self.watch_ctx.step,
            pc: self.watch_ctx.pc,
            vaddr,
            phys,
            kind,
            size,
            value,
            region,
        });
    }

    /// Declare a RAM region over `base..base+len`.
    pub fn add_ram(&mut self, name: &str, base: u32, len: u32) {
        self.push_region(Region {
            base,
            len,
            name: name.to_string(),
            kind: RegionKind::Ram(vec![0; len as usize]),
        });
    }

    /// Declare a read-only ROM region filled with `data`.
    pub fn add_rom(&mut self, name: &str, base: u32, data: Vec<u8>) {
        let len = data.len() as u32;
        self.push_region(Region {
            base,
            len,
            name: name.to_string(),
            kind: RegionKind::Rom(data),
        });
    }

    /// Declare a device-backed region. The device receives offsets relative
    /// to `base`.
    pub fn add_device(&mut self, name: &str, base: u32, len: u32, dev: Box<dyn Device>) {
        self.push_region(Region {
            base,
            len,
            name: name.to_string(),
            kind: RegionKind::Device(dev),
        });
    }

    /// Append a region, asserting the layout invariant the hot-region memo
    /// relies on: regions do not overlap (the linear scan's first-match
    /// rule is then equivalent to any region containing the address). The
    /// region vec is append-only; indices stay stable for the memo.
    fn push_region(&mut self, region: Region) {
        let base = region.base;
        let limit = region.base.saturating_add(region.len);
        for r in &self.regions {
            let r_base = r.base;
            let r_limit = r.base.saturating_add(r.len);
            debug_assert!(
                limit <= r_base || base >= r_limit,
                "overlapping regions: {} [{base:#x}..{limit:#x}) vs {} [{r_base:#x}..{r_limit:#x})",
                region.name,
                r.name,
            );
        }
        self.regions.push(region);
    }

    /// Declare a device-backed region that advances with CPU time
    /// (timers). Its `tick()` runs once per executed instruction.
    pub fn add_tickable_device(&mut self, name: &str, base: u32, len: u32, dev: Box<dyn Device>) {
        self.add_device(name, base, len, dev);
        self.tickable.push(self.regions.len() - 1);
    }

    /// Advance all time-dependent devices by one CPU step.
    pub fn tick(&mut self) {
        for &i in &self.tickable {
            if let RegionKind::Device(dev) = &mut self.regions[i].kind {
                dev.tick();
            }
        }
    }

    /// Translate a MIPS virtual address to a physical one.
    pub fn translate(&self, vaddr: u32) -> Result<u32, AccessFault> {
        match vaddr {
            0x0000_0000..=0x7FFF_FFFF => Err(AccessFault {
                vaddr,
                phys: None,
                kind: FaultKind::Read, // refined by caller (fetch/read)
                size: AccessSize::Word,
            }),
            0x8000_0000..=0xBFFF_FFFF => Ok(vaddr & 0x1FFF_FFFF),
            0xC000_0000..=0xFFFF_FFFF => Err(AccessFault {
                vaddr,
                phys: None,
                kind: FaultKind::Read,
                size: AccessSize::Word,
            }),
        }
    }

    fn find_region(&self, phys: u32) -> Option<&Region> {
        // Hot memo first (Milestone V, §24): one or two range checks in the
        // common case; entries are re-validated over the region's full
        // range, so a stale/aliased slot costs only the linear rescan.
        // Slot key: multiplied hash of the 4 KiB page — all region bases in
        // the machine are page-aligned with page numbers ≡ 0 (mod 4), so a
        // plain `page % 4` would park every region in slot 0 and thrash.
        let slot = hot_slot(phys);
        let (base, limit, idx) = self.hot[slot].get();
        if base <= phys && phys < limit && (idx as usize) < self.regions.len() {
            let r = &self.regions[idx as usize];
            if r.base == base && r.base.saturating_add(r.len) == limit {
                return Some(r);
            }
        }
        let found = self.regions.iter().position(|r| r.contains(phys))?;
        self.hot_set(phys, found);
        Some(&self.regions[found])
    }

    fn find_region_mut(&mut self, phys: u32) -> Option<&mut Region> {
        let slot = hot_slot(phys);
        let (base, limit, idx) = self.hot[slot].get();
        let memo_hit = if base <= phys && phys < limit && (idx as usize) < self.regions.len() {
            let r = &self.regions[idx as usize];
            r.base == base && r.base.saturating_add(r.len) == limit
        } else {
            false
        };
        let found = if memo_hit {
            idx as usize
        } else {
            let f = self.regions.iter_mut().position(|r| r.contains(phys))?;
            self.hot_set(phys, f);
            f
        };
        Some(&mut self.regions[found])
    }

    /// Fill the hot-region slot for `phys`'s 4 KiB page with the resolved
    /// region. Keyed by the page so neighboring accesses share a slot;
    /// conflicts overwrite (direct-mapped cache).
    fn hot_set(&self, phys: u32, index: usize) {
        let r = &self.regions[index];
        let limit = r.base.saturating_add(r.len);
        let slot = hot_slot(phys);
        self.hot[slot].set((r.base, limit, index as u32));
    }

    /// Name of the region containing `phys`, for diagnostics.
    pub fn region_name(&self, phys: u32) -> Option<&str> {
        self.find_region(phys).map(|r| r.name.as_str())
    }

    /// Read `size` bytes at virtual address `vaddr`, zero-extended.
    pub fn load(&mut self, vaddr: u32, size: AccessSize) -> Result<u32, AccessFault> {
        if !vaddr.is_multiple_of(size.bytes()) {
            return Err(AccessFault {
                vaddr,
                phys: None,
                kind: FaultKind::Alignment,
                size,
            });
        }
        let phys = match self.translate(vaddr) {
            Ok(p) => p,
            Err(mut f) => {
                f.kind = FaultKind::Read;
                f.size = size;
                return Err(f);
            }
        };
        let watch = self.watch_armed(WatchKind::Read, phys);
        let region = match self.find_region_mut(phys) {
            Some(r) => r,
            None => {
                return Err(AccessFault {
                    vaddr,
                    phys: Some(phys),
                    kind: FaultKind::Read,
                    size,
                })
            }
        };
        let off = (phys - region.base) as usize;
        let value = match &mut region.kind {
            RegionKind::Ram(bytes) | RegionKind::Rom(bytes) => {
                let n = size.bytes() as usize;
                let sl = &bytes[off..off + n];
                match size {
                    AccessSize::Byte => u32::from(sl[0]),
                    AccessSize::Half => u32::from(u16::from_le_bytes([sl[0], sl[1]])),
                    AccessSize::Word => u32::from_le_bytes([sl[0], sl[1], sl[2], sl[3]]),
                }
            }
            RegionKind::Device(dev) => {
                let rel = phys - region.base;
                dev.read(rel, size)
            }
        };
        if watch {
            self.record_watch(WatchKind::Read, vaddr, phys, size, value);
        }
        Ok(value)
    }

    /// Write `size` bytes at virtual address `vaddr`.
    pub fn store(&mut self, vaddr: u32, size: AccessSize, value: u32) -> Result<(), AccessFault> {
        if !vaddr.is_multiple_of(size.bytes()) {
            return Err(AccessFault {
                vaddr,
                phys: None,
                kind: FaultKind::Alignment,
                size,
            });
        }
        let phys = match self.translate(vaddr) {
            Ok(p) => p,
            Err(mut f) => {
                f.kind = FaultKind::Write;
                f.size = size;
                return Err(f);
            }
        };
        let watch = self.watch_armed(WatchKind::Write, phys);
        let region = match self.find_region_mut(phys) {
            Some(r) => r,
            None => {
                return Err(AccessFault {
                    vaddr,
                    phys: Some(phys),
                    kind: FaultKind::Write,
                    size,
                })
            }
        };
        let off = (phys - region.base) as usize;
        let outcome = match &mut region.kind {
            RegionKind::Ram(bytes) => {
                let b = value.to_le_bytes();
                let n = size.bytes() as usize;
                bytes[off..off + n].copy_from_slice(&b[..n]);
                Ok(())
            }
            RegionKind::Rom(_) => Err(AccessFault {
                vaddr,
                phys: Some(phys),
                kind: FaultKind::Write,
                size,
            }),
            RegionKind::Device(dev) => {
                let rel = phys - region.base;
                dev.write(rel, size, value);
                Ok(())
            }
        };
        if watch && outcome.is_ok() {
            self.record_watch(WatchKind::Write, vaddr, phys, size, value);
        }
        outcome
    }

    /// Fetch one little-endian instruction word at virtual address `vaddr`.
    pub fn fetch(&mut self, vaddr: u32) -> Result<u32, AccessFault> {
        if !vaddr.is_multiple_of(4) {
            return Err(AccessFault {
                vaddr,
                phys: None,
                kind: FaultKind::Alignment,
                size: AccessSize::Word,
            });
        }
        let phys = match self.translate(vaddr) {
            Ok(p) => p,
            Err(mut f) => {
                f.kind = FaultKind::Fetch;
                f.size = AccessSize::Word;
                return Err(f);
            }
        };
        let watch = self.watch_armed(WatchKind::Fetch, phys);
        let region = match self.find_region_mut(phys) {
            Some(r) => r,
            None => {
                return Err(AccessFault {
                    vaddr,
                    phys: Some(phys),
                    kind: FaultKind::Fetch,
                    size: AccessSize::Word,
                })
            }
        };
        let off = (phys - region.base) as usize;
        let value = match &mut region.kind {
            RegionKind::Ram(bytes) | RegionKind::Rom(bytes) => {
                let b: [u8; 4] = bytes[off..off + 4].try_into().map_err(|_| AccessFault {
                    vaddr,
                    phys: Some(phys),
                    kind: FaultKind::Fetch,
                    size: AccessSize::Word,
                })?;
                u32::from_le_bytes(b)
            }
            RegionKind::Device(dev) => {
                if dev.fetchable() {
                    dev.read(phys - region.base, AccessSize::Word)
                } else {
                    return Err(AccessFault {
                        vaddr,
                        phys: Some(phys),
                        kind: FaultKind::Fetch,
                        size: AccessSize::Word,
                    });
                }
            }
        };
        if watch {
            self.record_watch(WatchKind::Fetch, vaddr, phys, AccessSize::Word, value);
        }
        Ok(value)
    }

    /// Direct read access to RAM/ROM/device contents at physical address
    /// (tests, dumps). Devices are read via their normal read path.
    pub fn peek_phys(&mut self, phys: u32, size: AccessSize) -> Option<u32> {
        let region = self.find_region_mut(phys)?;
        let off = (phys - region.base) as usize;
        match &mut region.kind {
            RegionKind::Ram(bytes) | RegionKind::Rom(bytes) => {
                let n = size.bytes() as usize;
                if off + n > bytes.len() {
                    return None;
                }
                let sl = &bytes[off..off + n];
                Some(match size {
                    AccessSize::Byte => u32::from(sl[0]),
                    AccessSize::Half => u32::from(u16::from_le_bytes([sl[0], sl[1]])),
                    AccessSize::Word => u32::from_le_bytes([sl[0], sl[1], sl[2], sl[3]]),
                })
            }
            RegionKind::Device(dev) => Some(dev.read(off as u32, size)),
        }
    }

    /// Bulk diagnostic read of a physically backed region into `buf`.
    /// Entry-harness support (docs/firmware-analysis.md §20): snapshot the
    /// post-boot DRAM once, then restore it between candidate evaluations.
    /// Copies stop at the first region boundary or device region; returns
    /// the number of bytes actually copied.
    pub fn copy_out(&mut self, phys: u32, buf: &mut [u8]) -> usize {
        let Some(region) = self.find_region_mut(phys) else {
            return 0;
        };
        let off = (phys - region.base) as usize;
        match &mut region.kind {
            RegionKind::Ram(bytes) | RegionKind::Rom(bytes) => {
                let n = buf.len().min(bytes.len().saturating_sub(off));
                buf[..n].copy_from_slice(&bytes[off..off + n]);
                n
            }
            RegionKind::Device(_) => 0,
        }
    }

    /// Bulk diagnostic write into a physically backed region (mirror of
    /// [`Self::copy_out`]). Like [`Self::poke_phys`], this is a
    /// diagnostics path: ROM is writable only here, and CPU-facing stores
    /// still fault on ROM. Stops at the first region boundary or device
    /// region; returns the number of bytes actually written.
    pub fn copy_in(&mut self, phys: u32, bytes: &[u8]) -> usize {
        let Some(region) = self.find_region_mut(phys) else {
            return 0;
        };
        let off = (phys - region.base) as usize;
        match &mut region.kind {
            RegionKind::Ram(backing) | RegionKind::Rom(backing) => {
                let n = bytes.len().min(backing.len().saturating_sub(off));
                backing[off..off + n].copy_from_slice(&bytes[..n]);
                n
            }
            RegionKind::Device(_) => 0,
        }
    }

    /// Diagnostic write to physical memory (mirror of [`Self::peek_phys`]).
    /// Used by analysis examples to seed DRAM with canary patterns before a
    /// run (e.g. bounding the pre-decode content around the PNG pixel
    /// buffer, docs/firmware-analysis.md §18.3). Writing to a device region
    /// goes through the device's normal write path; ROM is writable only
    /// through this diagnostics path (the CPU-facing `store` rejects ROM,
    /// so this cannot be used to smuggle writes into running code).
    pub fn poke_phys(&mut self, phys: u32, size: AccessSize, value: u32) -> Option<()> {
        let region = self.find_region_mut(phys)?;
        let off = (phys - region.base) as usize;
        match &mut region.kind {
            RegionKind::Ram(bytes) | RegionKind::Rom(bytes) => {
                let n = size.bytes() as usize;
                if off + n > bytes.len() {
                    return None;
                }
                match size {
                    AccessSize::Byte => bytes[off] = value as u8,
                    AccessSize::Half => {
                        let b = value.to_le_bytes();
                        bytes[off..off + 2].copy_from_slice(&b[..2]);
                    }
                    AccessSize::Word => {
                        bytes[off..off + 4].copy_from_slice(&value.to_le_bytes());
                    }
                }
                Some(())
            }
            RegionKind::Device(dev) => {
                dev.write(off as u32, size, value);
                Some(())
            }
        }
    }

    /// Summary of declared regions for diagnostics output.
    pub fn region_map(&self) -> Vec<(String, u32, u32, &'static str)> {
        self.regions
            .iter()
            .map(|r| {
                let kind = match &r.kind {
                    RegionKind::Ram(_) => "RAM",
                    RegionKind::Rom(_) => "ROM",
                    RegionKind::Device(_) => "DEV",
                };
                (r.name.clone(), r.base, r.len, kind)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::LoggingDevice;

    /// Milestone V (§24): the hot-region memo must stay transparent —
    /// interleaved accesses over more regions than memo slots (slot
    /// conflicts) resolve to the correct region data every time.
    #[test]
    fn hot_region_memo_is_transparent_under_slot_conflicts() {
        let mut bus = AddressSpace::new();
        // 6 disjoint RAM regions > HOT_REGION_SLOTS(4), hashed so that
        // their 4 KiB pages collide in slots.
        let bases = [
            0x0000_0000u32,
            0x0100_0000,
            0x0200_0000,
            0x0300_0000,
            0x0400_0000,
            0x0500_0000,
        ];
        for (i, b) in bases.iter().enumerate() {
            bus.add_ram(&format!("r{i}"), *b, 0x1000);
            bus.store(*b | 0x8000_0000, AccessSize::Word, 0xA000_0000 + i as u32)
                .unwrap();
        }
        // Interleave accesses (memo thrash) and verify every read.
        for round in 0..50 {
            for (i, b) in bases.iter().enumerate() {
                let v = bus.load(b | 0x8000_0000, AccessSize::Word).unwrap();
                assert_eq!(v, 0xA000_0000 + i as u32, "round {round}");
                let name = bus.region_name(*b).unwrap().to_string();
                assert_eq!(name, format!("r{i}"));
            }
        }
        // Writes through the memo path stay precise (no cross-region bleed).
        bus.store(bases[1] | 0x8000_0000, AccessSize::Word, 0xDEAD_BEEF)
            .unwrap();
        for (i, b) in bases.iter().enumerate() {
            let v = bus.load(b | 0x8000_0000, AccessSize::Word).unwrap();
            let want = if i == 1 {
                0xDEAD_BEEF
            } else {
                0xA000_0000 + i as u32
            };
            assert_eq!(v, want);
        }
    }

    #[test]
    fn kseg_translation() {
        let asp = AddressSpace::new();
        assert_eq!(asp.translate(0x8000_0000).unwrap(), 0x0000_0000);
        assert_eq!(asp.translate(0x9FC0_0050).unwrap(), 0x1FC0_0050);
        assert_eq!(asp.translate(0xBFC0_0000).unwrap(), 0x1FC0_0000);
        // kseg1 masks with 0x1FFFFFFF: 0xBFFE0210 -> phys 0x1FFE0210 (the
        // loader param window sits in the top 256 KiB of the 4 MiB window)
        assert_eq!(asp.translate(0xBFFE_0210).unwrap(), 0x1FFE_0210);
        assert_eq!(asp.translate(0xBFFC_0E00).unwrap(), 0x1FFC_0E00);
        assert!(asp.translate(0x0000_1234).is_err());
        assert!(asp.translate(0xC000_0000).is_err());
    }

    #[test]
    fn ram_read_write_bounds() {
        let mut asp = AddressSpace::new();
        asp.add_ram("dram", 0x0000_0000, 0x1000);
        // kseg0 alias
        asp.store(0x8000_0100, AccessSize::Word, 0xDEAD_BEEF)
            .unwrap();
        assert_eq!(
            asp.load(0x8000_0100, AccessSize::Word).unwrap(),
            0xDEAD_BEEF
        );
        // kseg1 alias sees the same physical bytes
        assert_eq!(
            asp.load(0xA000_0100, AccessSize::Word).unwrap(),
            0xDEAD_BEEF
        );
        // byte lane check (little-endian)
        assert_eq!(asp.load(0x8000_0100, AccessSize::Byte).unwrap(), 0xEF);
        assert_eq!(asp.load(0x8000_0103, AccessSize::Byte).unwrap(), 0xDE);
        // out of bounds
        assert!(asp.load(0x8000_2000, AccessSize::Word).is_err());
        assert!(asp.store(0x8000_2000, AccessSize::Word, 1).is_err());
    }

    #[test]
    fn rom_is_read_only() {
        let mut asp = AddressSpace::new();
        asp.add_rom("flash", 0x1FC0_0000, vec![0x11, 0x22, 0x33, 0x44]);
        assert_eq!(
            asp.load(0xBFC0_0000, AccessSize::Word).unwrap(),
            0x4433_2211
        );
        let err = asp.store(0xBFC0_0000, AccessSize::Word, 0).unwrap_err();
        assert_eq!(err.kind, FaultKind::Write);
        assert_eq!(err.phys, Some(0x1FC0_0000));
    }

    #[test]
    fn device_dispatch() {
        let mut asp = AddressSpace::new();
        asp.add_device(
            "params",
            0x1FFE_0000,
            0x1000,
            Box::new(LoggingDevice::new("params", 0xFFFF_FFFF)),
        );
        // device reads return the configured default (kseg1 0xBFFE0000 ->
        // phys 0x1FFE0000)
        assert_eq!(
            asp.load(0xBFFE_0000, AccessSize::Word).unwrap(),
            0xFFFF_FFFF
        );
        // writes go to the device (no fault) and are recorded
        asp.store(0xBFFE_0210, AccessSize::Word, 0x380).unwrap();
        assert_eq!(
            asp.load(0xBFFE_0210, AccessSize::Word).unwrap(),
            0xFFFF_FFFF
        );
    }

    #[test]
    fn alignment_faults() {
        let mut asp = AddressSpace::new();
        asp.add_ram("dram", 0, 0x1000);
        assert!(asp.load(0x8000_0002, AccessSize::Word).is_err());
        assert!(asp.store(0x8000_0001, AccessSize::Half, 1).is_err());
        assert!(asp.fetch(0xBFC0_0002).is_err());
    }

    #[test]
    fn useg_and_kseg2_fault() {
        let mut asp = AddressSpace::new();
        asp.add_ram("dram", 0, 0x1000);
        assert!(asp.load(0x0000_0000, AccessSize::Word).is_err());
        assert!(asp.load(0xC000_0000, AccessSize::Word).is_err());
    }

    #[test]
    fn peek_phys_works() {
        let mut asp = AddressSpace::new();
        asp.add_ram("dram", 0, 0x100);
        asp.store(0x8000_0010, AccessSize::Word, 0x1234_5678)
            .unwrap();
        assert_eq!(asp.peek_phys(0x10, AccessSize::Word), Some(0x1234_5678));
    }

    #[test]
    fn bulk_copy_roundtrips_and_clamps() {
        let mut asp = AddressSpace::new();
        asp.add_ram("dram", 0, 0x100);
        asp.add_rom("flash", 0x1000_0000, vec![0x55; 0x40]);
        // Roundtrip through RAM.
        let src: Vec<u8> = (0u8..=63).collect();
        assert_eq!(asp.copy_in(0x20, &src), 64);
        let mut dst = [0u8; 64];
        assert_eq!(asp.copy_out(0x20, &mut dst), 64);
        assert_eq!(&dst[..], &src[..]);
        // Clamped at the region end.
        let mut tail = [0u8; 0x100];
        assert_eq!(asp.copy_out(0xE0, &mut tail), 0x20);
        // Unmapped base copies nothing.
        assert_eq!(asp.copy_out(0x9000_0000, &mut tail), 0);
        // ROM reads back through the bulk path too.
        let mut rom = [0u8; 8];
        assert_eq!(asp.copy_out(0x1000_0000, &mut rom), 8);
        assert!(rom.iter().all(|&b| b == 0x55));
        // ...and copy_in writes ROM only through this diagnostics path.
        assert_eq!(asp.copy_in(0x1000_0004, &[0x77, 0x88]), 2);
        assert_eq!(asp.peek_phys(0x1000_0004, AccessSize::Half), Some(0x8877));
    }

    #[test]
    fn poke_phys_seeds_ram_but_not_cpu_writable_rom() {
        let mut asp = AddressSpace::new();
        asp.add_ram("dram", 0, 0x100);
        asp.add_rom("flash", 0x1000_0000, vec![0xAA; 0x100]);
        // RAM: poke lands, peek sees it.
        assert!(asp.poke_phys(0x40, AccessSize::Word, 0xA5A5_A5A5).is_some());
        assert_eq!(asp.peek_phys(0x40, AccessSize::Word), Some(0xA5A5_A5A5));
        // ROM: diagnostics path writes it (canonical 0xAA..0xA5).
        assert!(asp
            .poke_phys(0x1000_0010, AccessSize::Word, 0x1111_2222)
            .is_some());
        assert_eq!(
            asp.peek_phys(0x1000_0010, AccessSize::Word),
            Some(0x1111_2222)
        );
        // ...but the CPU-facing store still faults on ROM.
        assert!(asp
            .store(0xB000_0010, AccessSize::Word, 0xDEAD_BEEF)
            .is_err());
        assert_eq!(
            asp.peek_phys(0x1000_0010, AccessSize::Word),
            Some(0x1111_2222)
        );
        // Out-of-range pokes are None.
        assert!(asp.poke_phys(0x200, AccessSize::Word, 0).is_none());
    }
}
