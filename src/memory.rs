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
#[derive(Debug, Clone, Copy)]
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
#[derive(Default)]
pub struct AddressSpace {
    regions: Vec<Region>,
    /// Indices of regions whose device implements time behavior.
    tickable: Vec<usize>,
}

impl AddressSpace {
    /// Create an empty address space.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a RAM region over `base..base+len`.
    pub fn add_ram(&mut self, name: &str, base: u32, len: u32) {
        self.regions.push(Region {
            base,
            len,
            name: name.to_string(),
            kind: RegionKind::Ram(vec![0; len as usize]),
        });
    }

    /// Declare a read-only ROM region filled with `data`.
    pub fn add_rom(&mut self, name: &str, base: u32, data: Vec<u8>) {
        let len = data.len() as u32;
        self.regions.push(Region {
            base,
            len,
            name: name.to_string(),
            kind: RegionKind::Rom(data),
        });
    }

    /// Declare a device-backed region. The device receives offsets relative
    /// to `base`.
    pub fn add_device(&mut self, name: &str, base: u32, len: u32, dev: Box<dyn Device>) {
        self.regions.push(Region {
            base,
            len,
            name: name.to_string(),
            kind: RegionKind::Device(dev),
        });
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
        self.regions.iter().find(|r| r.contains(phys))
    }

    fn find_region_mut(&mut self, phys: u32) -> Option<&mut Region> {
        self.regions.iter_mut().find(|r| r.contains(phys))
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
        match &mut region.kind {
            RegionKind::Ram(bytes) | RegionKind::Rom(bytes) => {
                let n = size.bytes() as usize;
                let sl = &bytes[off..off + n];
                Ok(match size {
                    AccessSize::Byte => u32::from(sl[0]),
                    AccessSize::Half => u32::from(u16::from_le_bytes([sl[0], sl[1]])),
                    AccessSize::Word => u32::from_le_bytes([sl[0], sl[1], sl[2], sl[3]]),
                })
            }
            RegionKind::Device(dev) => {
                let rel = phys - region.base;
                Ok(dev.read(rel, size))
            }
        }
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
        match &mut region.kind {
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
        }
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
        match &mut region.kind {
            RegionKind::Ram(bytes) | RegionKind::Rom(bytes) => {
                let b: [u8; 4] = bytes[off..off + 4].try_into().map_err(|_| AccessFault {
                    vaddr,
                    phys: Some(phys),
                    kind: FaultKind::Fetch,
                    size: AccessSize::Word,
                })?;
                Ok(u32::from_le_bytes(b))
            }
            RegionKind::Device(dev) => {
                if dev.fetchable() {
                    Ok(dev.read(phys - region.base, AccessSize::Word))
                } else {
                    Err(AccessFault {
                        vaddr,
                        phys: Some(phys),
                        kind: FaultKind::Fetch,
                        size: AccessSize::Word,
                    })
                }
            }
        }
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
}
