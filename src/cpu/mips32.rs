//! Pure-Rust MIPS32r2 little-endian interpreter.
//!
//! Scope: the integer instruction set used by the SPHE15xx boot path —
//! full MIPS32 integer arithmetic/logic/shift/load/store (incl. lwl/lwr/
//! swl/swr, ll/sc), MIPS32r2 `ext`/`ins`/`seb`/`seh`/`wsbh`, SPECIAL2
//! `mul`/`madd`/`clz`, branches with delay slots (incl. likely variants),
//! `j`/`jal`/`jr`/`jalr`, and the COP0 subset listed in [`cop0`].
//! `cache`/`pref`/`sync` are logged no-ops. FPU, MIPS16e and microMIPS are
//! not implemented and produce clean [`CpuError`]s.
//!
//! Timing model: `Count` increments by 2 per retired instruction
//! (deterministic; see docs/architecture.md).

#[cfg(test)]
use super::cop0::cause_bits as c0_cause;
use super::cop0::{regs as c0, status_bits as c0_bits, Cop0};
use super::{CpuBackend, CpuError, CpuSnapshot, MemEvent, StepReport};
use crate::memory::{AccessSize, AddressSpace};

/// MIPS32r2 LE interpreter backend.
#[derive(Debug, Clone)]
pub struct Mips32 {
    regs: [u32; 32],
    hi: u32,
    lo: u32,
    pc: u32,
    next_pc: u32,
    in_delay_slot: bool,
    cop0: Cop0,
    /// Level-triggered hardware interrupt inputs (IP2..IP7) driven by the
    /// platform interrupt controller via `set_ext_irq_lines`.
    ext_irq_lines: u8,
}

impl Default for Mips32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Mips32 {
    /// Create a core in its power-on state (registers zeroed).
    pub fn new() -> Self {
        Self {
            regs: [0; 32],
            hi: 0,
            lo: 0,
            pc: 0,
            next_pc: 0,
            in_delay_slot: false,
            cop0: Cop0::new(),
            ext_irq_lines: 0,
        }
    }

    /// Access the COP0 model (diagnostics).
    pub fn cop0(&self) -> &Cop0 {
        &self.cop0
    }

    fn wreg(&mut self, r: u32, v: u32) {
        if r != 0 {
            self.regs[r as usize] = v;
        }
    }

    fn rreg(&self, r: u32) -> u32 {
        self.regs[r as usize]
    }

    #[allow(clippy::too_many_arguments)]
    fn load_bytes(
        &mut self,
        bus: &mut AddressSpace,
        pc: u32,
        vaddr: u32,
        n: u32,
        mem: &mut Vec<MemEvent>,
    ) -> Result<u32, CpuError> {
        let mut val: u32 = 0;
        for i in 0..n {
            let b = bus
                .load(vaddr.wrapping_add(i), AccessSize::Byte)
                .map_err(|f| CpuError::Fault { pc, fault: f })?;
            mem.push(MemEvent {
                write: false,
                vaddr: vaddr.wrapping_add(i),
                size: AccessSize::Byte,
                value: b,
            });
            val |= b << (8 * i);
        }
        Ok(val)
    }

    #[allow(clippy::too_many_arguments)]
    fn store_bytes(
        &mut self,
        bus: &mut AddressSpace,
        pc: u32,
        vaddr: u32,
        n: u32,
        val: u32,
        mem: &mut Vec<MemEvent>,
    ) -> Result<(), CpuError> {
        for i in 0..n {
            let b = (val >> (8 * i)) & 0xFF;
            bus.store(vaddr.wrapping_add(i), AccessSize::Byte, b)
                .map_err(|f| CpuError::Fault { pc, fault: f })?;
            mem.push(MemEvent {
                write: true,
                vaddr: vaddr.wrapping_add(i),
                size: AccessSize::Byte,
                value: b,
            });
        }
        Ok(())
    }

    /// Execute one decoded instruction.
    #[allow(clippy::too_many_lines)]
    fn execute(
        &mut self,
        bus: &mut AddressSpace,
        pc: u32,
        word: u32,
        mem: &mut Vec<MemEvent>,
        flow: &mut Flow,
    ) -> Result<(), CpuError> {
        let op = word >> 26;
        let rs = (word >> 21) & 0x1F;
        let rt = (word >> 16) & 0x1F;
        let rd = (word >> 11) & 0x1F;
        let sh = (word >> 6) & 0x1F;
        let funct = word & 0x3F;
        let imm = word & 0xFFFF;
        let simm = (imm as u16 as i16) as i32;
        let branch_target = pc.wrapping_add(4).wrapping_add((simm << 2) as u32);

        // Shorthand for a taken branch (delay slot then target).
        macro_rules! take {
            ($t:expr) => {{
                *flow = Flow::Branch($t);
            }};
        }
        // Shorthand for a likely-branch that was NOT taken (annul slot).
        macro_rules! annul {
            () => {{
                *flow = Flow::Annul;
            }};
        }

        match op {
            // ---- SPECIAL ----
            0x00 => match funct {
                0x00 => self.wreg(rd, self.rreg(rt) << sh), // sll (nop when word==0)
                0x02 => self.wreg(rd, self.rreg(rt) >> sh), // srl
                0x03 => {
                    self.wreg(rd, ((self.rreg(rt) as i32) >> sh) as u32); // sra
                }
                0x04 => self.wreg(rd, self.rreg(rt) << (self.rreg(rs) & 0x1F)), // sllv
                0x06 => self.wreg(rd, self.rreg(rt) >> (self.rreg(rs) & 0x1F)), // srlv
                0x07 => {
                    let v = ((self.rreg(rt) as i32) >> (self.rreg(rs) & 0x1F)) as u32; // srav
                    self.wreg(rd, v);
                }
                0x08 => *flow = Flow::Branch(self.rreg(rs)), // jr
                0x09 => {
                    let t = self.rreg(rs); // jalr
                    self.wreg(rd, pc.wrapping_add(8));
                    *flow = Flow::Branch(t);
                }
                0x0A => {
                    // movz
                    if self.rreg(rt) == 0 {
                        self.wreg(rd, self.rreg(rs));
                    }
                }
                0x0B => {
                    // movn
                    if self.rreg(rt) != 0 {
                        self.wreg(rd, self.rreg(rs));
                    }
                }
                0x0C => return Err(CpuError::Syscall { pc }),
                0x0D => {
                    return Err(CpuError::Break {
                        pc,
                        code: (word >> 6) & 0xFFFFF,
                    })
                }
                0x0F => {}                       // sync: no-op
                0x10 => self.wreg(rd, self.hi),  // mfhi
                0x11 => self.hi = self.rreg(rs), // mthi
                0x12 => self.wreg(rd, self.lo),  // mflo
                0x13 => self.lo = self.rreg(rs), // mtlo
                0x18 => {
                    // mult
                    let a = self.rreg(rs) as i32 as i64;
                    let b = self.rreg(rt) as i32 as i64;
                    let p = (a * b) as u64;
                    self.lo = p as u32;
                    self.hi = (p >> 32) as u32;
                }
                0x19 => {
                    // multu
                    let p = u64::from(self.rreg(rs)) * u64::from(self.rreg(rt));
                    self.lo = p as u32;
                    self.hi = (p >> 32) as u32;
                }
                0x1A => {
                    // div
                    let a = self.rreg(rs) as i32;
                    let b = self.rreg(rt) as i32;
                    if b == 0 {
                        self.lo = 0;
                        self.hi = 0; // UNPREDICTABLE on hardware; deterministic here
                    } else if a == i32::MIN && b == -1 {
                        self.lo = a as u32;
                        self.hi = 0;
                    } else {
                        self.lo = (a / b) as u32;
                        self.hi = (a % b) as u32;
                    }
                }
                0x1B => {
                    // divu
                    let a = self.rreg(rs);
                    let b = self.rreg(rt);
                    match a.checked_div(b) {
                        Some(q) => {
                            self.lo = q;
                            self.hi = a % b;
                        }
                        None => {
                            // divu by zero is UNPREDICTABLE on MIPS; we
                            // deterministically pick 0/0 so runs stay
                            // reproducible.
                            self.lo = 0;
                            self.hi = 0;
                        }
                    }
                }
                0x20 | 0x21 => self.wreg(rd, self.rreg(rs).wrapping_add(self.rreg(rt))), // add/addu
                0x22 | 0x23 => self.wreg(rd, self.rreg(rs).wrapping_sub(self.rreg(rt))), // sub/subu
                0x24 => self.wreg(rd, self.rreg(rs) & self.rreg(rt)),                    // and
                0x25 => self.wreg(rd, self.rreg(rs) | self.rreg(rt)),                    // or
                0x26 => self.wreg(rd, self.rreg(rs) ^ self.rreg(rt)),                    // xor
                0x27 => self.wreg(rd, !(self.rreg(rs) | self.rreg(rt))),                 // nor
                0x2A => {
                    self.wreg(
                        rd,
                        u32::from((self.rreg(rs) as i32) < (self.rreg(rt) as i32)),
                    ); // slt
                }
                0x2B => self.wreg(rd, u32::from(self.rreg(rs) < self.rreg(rt))), // sltu
                0x30..=0x36 => {
                    // tge/tgeu/tlt/tltu/teq/tne — fire => clean stop
                    let a = self.rreg(rs);
                    let b = self.rreg(rt);
                    let fires = match funct {
                        0x30 => (a as i32) >= (b as i32),
                        0x31 => a >= b,
                        0x32 => (a as i32) < (b as i32),
                        0x33 => a < b,
                        0x34 => a == b,
                        _ => a != b,
                    };
                    if fires {
                        return Err(CpuError::Trap { pc, word });
                    }
                }
                _ => return Err(CpuError::UnknownInstruction { pc, word }),
            },

            // ---- REGIMM ----
            0x01 => {
                let a = self.rreg(rs) as i32;
                let taken = match rt {
                    0x00 | 0x02 => a < 0,  // bltz / bltzl
                    0x01 | 0x03 => a >= 0, // bgez / bgezl
                    0x10 | 0x12 => {
                        // bltzal / bltzall
                        self.wreg(31, pc.wrapping_add(8));
                        a < 0
                    }
                    0x11 | 0x13 => {
                        // bgezal / bgezall
                        self.wreg(31, pc.wrapping_add(8));
                        a >= 0
                    }
                    _ => return Err(CpuError::UnknownInstruction { pc, word }),
                };
                let likely = matches!(rt, 0x02 | 0x03 | 0x12 | 0x13);
                if taken {
                    take!(branch_target);
                } else if likely {
                    annul!();
                }
            }

            0x02 => {
                // j
                let t = (pc.wrapping_add(4) & 0xF000_0000) | ((word & 0x03FF_FFFF) << 2);
                take!(t);
            }
            0x03 => {
                // jal
                self.wreg(31, pc.wrapping_add(8));
                let t = (pc.wrapping_add(4) & 0xF000_0000) | ((word & 0x03FF_FFFF) << 2);
                take!(t);
            }
            0x04 => {
                // beq
                if self.rreg(rs) == self.rreg(rt) {
                    take!(branch_target);
                }
            }
            0x05 => {
                // bne
                if self.rreg(rs) != self.rreg(rt) {
                    take!(branch_target);
                }
            }
            0x06 => {
                // blez
                if (self.rreg(rs) as i32) <= 0 {
                    take!(branch_target);
                }
            }
            0x07 => {
                // bgtz
                if (self.rreg(rs) as i32) > 0 {
                    take!(branch_target);
                }
            }
            0x14 => {
                // beql
                if self.rreg(rs) == self.rreg(rt) {
                    take!(branch_target);
                } else {
                    annul!();
                }
            }
            0x15 => {
                // bnel
                if self.rreg(rs) != self.rreg(rt) {
                    take!(branch_target);
                } else {
                    annul!();
                }
            }
            0x16 => {
                // blezl
                if (self.rreg(rs) as i32) <= 0 {
                    take!(branch_target);
                } else {
                    annul!();
                }
            }
            0x17 => {
                // bgtzl
                if (self.rreg(rs) as i32) > 0 {
                    take!(branch_target);
                } else {
                    annul!();
                }
            }

            // ---- immediates ----
            0x08 | 0x09 => self.wreg(rt, self.rreg(rs).wrapping_add(simm as u32)), // addi/addiu
            0x0A => self.wreg(rt, u32::from((self.rreg(rs) as i32) < simm)),       // slti
            0x0B => self.wreg(rt, u32::from(self.rreg(rs) < imm)),                 // sltiu
            0x0C => self.wreg(rt, self.rreg(rs) & imm),                            // andi
            0x0D => self.wreg(rt, self.rreg(rs) | imm),                            // ori
            0x0E => self.wreg(rt, self.rreg(rs) ^ imm),                            // xori
            0x0F => self.wreg(rt, imm << 16),                                      // lui

            // ---- COP0 ----
            0x10 => {
                let sel = word & 0x7;
                if word & (1 << 25) == 0 {
                    match (word >> 21) & 0x1F {
                        0x00 => self.wreg(rt, self.cop0.read(rd as u8, sel as u8)), // mfc0
                        0x04 => {
                            let v = self.rreg(rt);
                            self.cop0.write(rd as u8, sel as u8, v); // mtc0
                        }
                        _ => {
                            return Err(CpuError::Unsupported {
                                pc,
                                word,
                                reason: "COP0 rs not mfc0/mtc0".to_string(),
                            })
                        }
                    }
                } else {
                    // CO (function field)
                    match funct {
                        0x18 => {
                            // eret (MIPS32r2): with ERL set the return
                            // address is ErrorEPC and ERL clears (error/
                            // reset path); otherwise EPC is used and EXL
                            // clears (interrupt/exception path). No delay
                            // slot; llbit clears.
                            if self.cop0.status() & c0_bits::ERL != 0 {
                                let e = self.cop0.read(c0::ERROREPC, 0);
                                self.cop0
                                    .write(c0::STATUS, 0, self.cop0.status() & !c0_bits::ERL);
                                *flow = Flow::JumpNow(e);
                            } else {
                                let e = self.cop0.read(c0::EPC, 0);
                                self.cop0
                                    .write(c0::STATUS, 0, self.cop0.status() & !c0_bits::EXL);
                                *flow = Flow::JumpNow(e);
                            }
                        }
                        0x01 | 0x02 | 0x06 | 0x08 => {} // tlbwr/tlbwi/tlbr/tlbp: no-op (no TLB model)
                        _ => {
                            return Err(CpuError::UnknownInstruction { pc, word });
                        }
                    }
                }
            }

            // ---- loads/stores ----
            0x20 => {
                // lb
                let a = self.rreg(rs).wrapping_add(simm as u32);
                let v = bus
                    .load(a, AccessSize::Byte)
                    .map_err(|f| CpuError::Fault { pc, fault: f })?;
                mem.push(MemEvent {
                    write: false,
                    vaddr: a,
                    size: AccessSize::Byte,
                    value: v,
                });
                self.wreg(rt, (v as u8) as i8 as i32 as u32);
            }
            0x21 => {
                // lh
                let a = self.rreg(rs).wrapping_add(simm as u32);
                let v = bus
                    .load(a, AccessSize::Half)
                    .map_err(|f| CpuError::Fault { pc, fault: f })?;
                mem.push(MemEvent {
                    write: false,
                    vaddr: a,
                    size: AccessSize::Half,
                    value: v,
                });
                self.wreg(rt, (v as u16) as i16 as i32 as u32);
            }
            0x23 => {
                // lw
                let a = self.rreg(rs).wrapping_add(simm as u32);
                let v = bus
                    .load(a, AccessSize::Word)
                    .map_err(|f| CpuError::Fault { pc, fault: f })?;
                mem.push(MemEvent {
                    write: false,
                    vaddr: a,
                    size: AccessSize::Word,
                    value: v,
                });
                self.wreg(rt, v);
            }
            0x24 => {
                // lbu
                let a = self.rreg(rs).wrapping_add(simm as u32);
                let v = bus
                    .load(a, AccessSize::Byte)
                    .map_err(|f| CpuError::Fault { pc, fault: f })?;
                mem.push(MemEvent {
                    write: false,
                    vaddr: a,
                    size: AccessSize::Byte,
                    value: v,
                });
                self.wreg(rt, v);
            }
            0x25 => {
                // lhu
                let a = self.rreg(rs).wrapping_add(simm as u32);
                let v = bus
                    .load(a, AccessSize::Half)
                    .map_err(|f| CpuError::Fault { pc, fault: f })?;
                mem.push(MemEvent {
                    write: false,
                    vaddr: a,
                    size: AccessSize::Half,
                    value: v,
                });
                self.wreg(rt, v);
            }
            0x28 => {
                // sb
                let a = self.rreg(rs).wrapping_add(simm as u32);
                bus.store(a, AccessSize::Byte, self.rreg(rt) & 0xFF)
                    .map_err(|f| CpuError::Fault { pc, fault: f })?;
                mem.push(MemEvent {
                    write: true,
                    vaddr: a,
                    size: AccessSize::Byte,
                    value: self.rreg(rt) & 0xFF,
                });
            }
            0x29 => {
                // sh
                let a = self.rreg(rs).wrapping_add(simm as u32);
                let v = self.rreg(rt) & 0xFFFF;
                bus.store(a, AccessSize::Half, v)
                    .map_err(|f| CpuError::Fault { pc, fault: f })?;
                mem.push(MemEvent {
                    write: true,
                    vaddr: a,
                    size: AccessSize::Half,
                    value: v,
                });
            }
            0x2B => {
                // sw
                let a = self.rreg(rs).wrapping_add(simm as u32);
                let v = self.rreg(rt);
                bus.store(a, AccessSize::Word, v)
                    .map_err(|f| CpuError::Fault { pc, fault: f })?;
                mem.push(MemEvent {
                    write: true,
                    vaddr: a,
                    size: AccessSize::Word,
                    value: v,
                });
            }
            0x30 => {
                // ll — modeled as lw (single-core)
                let a = self.rreg(rs).wrapping_add(simm as u32);
                let v = bus
                    .load(a, AccessSize::Word)
                    .map_err(|f| CpuError::Fault { pc, fault: f })?;
                mem.push(MemEvent {
                    write: false,
                    vaddr: a,
                    size: AccessSize::Word,
                    value: v,
                });
                self.wreg(rt, v);
            }
            0x38 => {
                // sc — always succeeds (single-core)
                let a = self.rreg(rs).wrapping_add(simm as u32);
                let v = self.rreg(rt);
                bus.store(a, AccessSize::Word, v)
                    .map_err(|f| CpuError::Fault { pc, fault: f })?;
                mem.push(MemEvent {
                    write: true,
                    vaddr: a,
                    size: AccessSize::Word,
                    value: v,
                });
                self.wreg(rt, 1);
            }

            // lwl / lwr / swl / swr — little-endian semantics, byte-wise bus
            // access so unaligned addresses never hit the alignment check.
            0x22 => {
                // lwl
                let a = self.rreg(rs).wrapping_add(simm as u32);
                let b = a & 3;
                let n = b + 1;
                let w = self.load_bytes(bus, pc, a & !3, 4, mem)?;
                let val = w & mask_lo(8 * n);
                let shift = 8 * (4 - n);
                let cur = self.rreg(rt);
                self.wreg(rt, (val << shift) | (cur & mask_lo(shift)));
            }
            0x26 => {
                // lwr
                let a = self.rreg(rs).wrapping_add(simm as u32);
                let b = a & 3;
                let n = 4 - b;
                let w = self.load_bytes(bus, pc, a & !3, 4, mem)?;
                let val = (w >> (8 * b)) & mask_lo(8 * n);
                let cur = self.rreg(rt);
                self.wreg(rt, (cur & !mask_lo(8 * n)) | val);
            }
            0x2A => {
                // swl
                let a = self.rreg(rs).wrapping_add(simm as u32);
                let b = a & 3;
                let n = b + 1;
                let shift = 8 * (4 - n);
                let w = self.load_bytes(bus, pc, a & !3, 4, mem)?;
                let merged =
                    (w & !(mask_lo(8 * n) << shift)) | ((self.rreg(rt) >> shift) & mask_lo(8 * n));
                self.store_bytes(bus, pc, a & !3, 4, merged, mem)?;
            }
            0x2E => {
                // swr — LE: low n bytes of rt go to bytes [b..3] of the word
                let a = self.rreg(rs).wrapping_add(simm as u32);
                let b = a & 3;
                let n = 4 - b;
                let w = self.load_bytes(bus, pc, a & !3, 4, mem)?;
                let merged = (w & !(mask_lo(8 * n) << (8 * b)))
                    | ((self.rreg(rt) & mask_lo(8 * n)) << (8 * b));
                self.store_bytes(bus, pc, a & !3, 4, merged, mem)?;
            }

            // ---- SPECIAL2 ----
            0x1C => match funct {
                0x00 | 0x04 => {
                    // madd / msub
                    let acc = ((u64::from(self.hi)) << 32) | u64::from(self.lo);
                    let prod = i64::from(self.rreg(rs) as i32) * i64::from(self.rreg(rt) as i32);
                    let res = if funct == 0x00 {
                        (acc as i64).wrapping_add(prod)
                    } else {
                        (acc as i64).wrapping_sub(prod)
                    };
                    self.lo = res as u32;
                    self.hi = (res as u64 >> 32) as u32;
                }
                0x01 | 0x05 => {
                    // maddu / msubu
                    let acc = ((u64::from(self.hi)) << 32) | u64::from(self.lo);
                    let prod = u64::from(self.rreg(rs)) * u64::from(self.rreg(rt));
                    let res = if funct == 0x01 {
                        acc.wrapping_add(prod)
                    } else {
                        acc.wrapping_sub(prod)
                    };
                    self.lo = res as u32;
                    self.hi = (res >> 32) as u32;
                }
                0x02 => {
                    // mul
                    let p = (self.rreg(rs) as i32).wrapping_mul(self.rreg(rt) as i32);
                    self.wreg(rd, p as u32);
                }
                0x20 => self.wreg(rd, clz(self.rreg(rs))), // clz
                0x21 => self.wreg(rd, clo(self.rreg(rs))), // clo
                _ => return Err(CpuError::UnknownInstruction { pc, word }),
            },

            // ---- SPECIAL3 (MIPS32r2) ----
            // NOTE field placement: for ext/ins the RD field (15:11) holds
            // msb/msbd and the SA field (10:6) holds pos/lsb.
            0x1F => match funct {
                0x00 => {
                    // ext rt, rs, pos, size
                    let pos = sh;
                    let size = rd + 1;
                    let v = (self.rreg(rs) >> pos) & mask_lo(size);
                    self.wreg(rt, v);
                }
                0x04 => {
                    // ins rt, rs, pos, size (msb = rd)
                    let pos = sh;
                    let msb = rd;
                    let size = msb - pos + 1;
                    let mask = mask_lo(size) << pos;
                    let cur = self.rreg(rt);
                    self.wreg(rt, (cur & !mask) | ((self.rreg(rs) << pos) & mask));
                }
                0x20 => {
                    // bshfl: wsbh(0x02) / seb(0x10) / seh(0x18)
                    match word >> 6 & 0x1F {
                        0x02 => {
                            let v = self.rreg(rt);
                            let out = ((v & 0x00FF_00FF) << 8) | ((v >> 8) & 0x00FF_00FF);
                            self.wreg(rd, out); // wsbh
                        }
                        0x10 => self.wreg(rd, (self.rreg(rt) as u8) as i8 as i32 as u32), // seb
                        0x18 => self.wreg(rd, (self.rreg(rt) as u16) as i16 as i32 as u32), // seh
                        _ => return Err(CpuError::UnknownInstruction { pc, word }),
                    }
                }
                0x3B => {
                    // rdhwr
                    let v = match rd {
                        0 => 0,                            // CPUNum
                        1 => 32,                           // SYNCI_Step
                        2 => self.cop0.read(c0::COUNT, 0), // CC
                        3 => 1,                            // CCRes
                        29 => 0,                           // ULR
                        _ => {
                            return Err(CpuError::Unsupported {
                                pc,
                                word,
                                reason: format!("rdhwr reg {rd}"),
                            })
                        }
                    };
                    self.wreg(rt, v);
                }
                0x0A => {
                    // MIPS DSP ASE indexed loads (observed in the eCos memory
                    // routines of the main firmware). The SA field selects
                    // the sub-op: 0x00 = LWX (word), 0x04 = LHX (halfword).
                    // GPR[rd] = mem[GPR[rt] + GPR[rs]].
                    let addr = self.rreg(rt).wrapping_add(self.rreg(rs));
                    match sh {
                        0x00 => {
                            let v = bus
                                .load(addr, AccessSize::Word)
                                .map_err(|f| CpuError::Fault { pc, fault: f })?;
                            self.wreg(rd, v);
                        }
                        0x04 => {
                            let v = bus
                                .load(addr, AccessSize::Half)
                                .map_err(|f| CpuError::Fault { pc, fault: f })?;
                            self.wreg(rd, (v as u16) as i16 as i32 as u32);
                        }
                        _ => return Err(CpuError::UnknownInstruction { pc, word }),
                    }
                }
                _ => return Err(CpuError::UnknownInstruction { pc, word }),
            },

            0x2F => { /* cache — logged no-op (desc shows it) */ }
            0x33 => { /* pref — no-op */ }

            _ => return Err(CpuError::UnknownInstruction { pc, word }),
        }
        Ok(())
    }
}

/// Control-flow outcome of one executed instruction.
#[derive(Debug, Clone, Copy)]
enum Flow {
    /// Sequential execution (including not-taken non-likely branches: the
    /// delay slot still executes).
    Seq,
    /// Branch/jump taken: execute the delay slot, then transfer to target.
    Branch(u32),
    /// Likely-branch not taken: the delay slot is annulled (skipped).
    Annul,
    /// Immediate control transfer with no delay slot (eret).
    JumpNow(u32),
}

/// Branch-like opcodes (used to reject branches in delay slots).
fn is_branch(word: u32) -> bool {
    matches!(
        word >> 26,
        0x01 | 0x02 | 0x03 | 0x04..=0x07 | 0x14..=0x17
    ) || ((word >> 26) == 0x00 && matches!(word & 0x3F, 0x08 | 0x09))
}

#[inline]
fn mask_lo(bits: u32) -> u32 {
    if bits == 0 || bits >= 32 {
        u32::MAX
    } else {
        (1u32 << bits) - 1
    }
}

#[inline]
fn clz(x: u32) -> u32 {
    if x == 0 {
        32
    } else {
        x.leading_zeros()
    }
}

#[inline]
fn clo(x: u32) -> u32 {
    clz(!x)
}

impl CpuBackend for Mips32 {
    fn reset(&mut self, entry: u32) {
        self.regs = [0; 32];
        self.hi = 0;
        self.lo = 0;
        self.pc = entry;
        self.next_pc = entry.wrapping_add(4);
        self.in_delay_slot = false;
        self.cop0 = Cop0::new();
        self.ext_irq_lines = 0;
    }

    fn pc(&self) -> u32 {
        self.pc
    }

    fn reg(&self, index: usize) -> u32 {
        self.regs[index & 0x1F]
    }

    fn snapshot(&self) -> CpuSnapshot {
        CpuSnapshot {
            pc: self.pc,
            next_pc: self.next_pc,
            regs: self.regs,
            hi: self.hi,
            lo: self.lo,
        }
    }

    fn name(&self) -> &'static str {
        "mips32r2-le-interpreter"
    }

    fn set_ext_irq_lines(&mut self, lines: u8) {
        self.ext_irq_lines = lines & 0x3F;
        // Reflect the hardware inputs in Cause.IP[7:2] (read-only to
        // software) so mfc0 Cause and the pending check see one state.
        self.cop0.set_ip_hw(self.ext_irq_lines);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn step(&mut self, bus: &mut AddressSpace) -> Result<StepReport, CpuError> {
        // Interrupt delivery (MIPS32r2): before fetching the next
        // instruction, an enabled + unmasked interrupt at a non-exception
        // level vectors to the interrupt handler. Delivery consumes no
        // step of its own — the report returned for this step is the first
        // handler instruction, and EPC/BD record the interrupted context.
        // Deterministic: the decision uses only state set before the step.
        if self.cop0.status() & (c0_bits::IE | c0_bits::EXL | c0_bits::ERL) == c0_bits::IE
            && self.cop0.irq_pending()
        {
            let epc = if self.in_delay_slot {
                self.pc.wrapping_sub(4)
            } else {
                self.pc
            };
            let vector = self.cop0.enter_exception(0, epc, self.in_delay_slot);
            self.pc = vector;
            self.next_pc = vector.wrapping_add(4);
            self.in_delay_slot = false;
        }
        let pc = self.pc;
        let word = bus
            .fetch(pc)
            .map_err(|f| CpuError::Fault { pc, fault: f })?;
        if self.in_delay_slot && is_branch(word) {
            return Err(CpuError::Unsupported {
                pc,
                word,
                reason: "branch in delay slot".to_string(),
            });
        }
        let mut mem = Vec::new();
        let mut flow = Flow::Seq;
        self.execute(bus, pc, word, &mut mem, &mut flow)?;
        self.cop0.tick();
        match flow {
            Flow::Seq => {
                self.pc = self.next_pc;
                self.next_pc = self.pc.wrapping_add(4);
                self.in_delay_slot = false;
            }
            Flow::Branch(t) => {
                self.pc = self.next_pc;
                self.next_pc = t;
                self.in_delay_slot = true;
            }
            Flow::Annul => {
                let after = self.next_pc.wrapping_add(4);
                self.pc = after;
                self.next_pc = after.wrapping_add(4);
                self.in_delay_slot = false;
            }
            Flow::JumpNow(t) => {
                self.pc = t;
                self.next_pc = t.wrapping_add(4);
                self.in_delay_slot = false;
            }
        }
        Ok(StepReport {
            pc,
            word,
            desc: describe(word),
            mem,
        })
    }
}

/// Compact textual description of an instruction word for traces.
#[must_use]
pub fn describe(word: u32) -> String {
    let op = word >> 26;
    let rs = ((word >> 21) & 0x1F) as usize;
    let rt = ((word >> 16) & 0x1F) as usize;
    let rd = ((word >> 11) & 0x1F) as usize;
    let sh = (word >> 6) & 0x1F;
    let funct = word & 0x3F;
    let imm = word & 0xFFFF;
    let simm = (imm as u16 as i16) as i32;
    let rdu = rd as u32;

    let rn = |i: usize| REG_NAMES[i];
    match op {
        0x00 => match funct {
            0x00 if word == 0 => "nop".to_string(),
            0x00 => format!("sll {}, {}, {sh}", rn(rd), rn(rt)),
            0x02 => format!("srl {}, {}, {sh}", rn(rd), rn(rt)),
            0x03 => format!("sra {}, {}, {sh}", rn(rd), rn(rt)),
            0x04 => format!("sllv {}, {}, {}", rn(rd), rn(rt), rn(rs)),
            0x06 => format!("srlv {}, {}, {}", rn(rd), rn(rt), rn(rs)),
            0x07 => format!("srav {}, {}, {}", rn(rd), rn(rt), rn(rs)),
            0x08 => format!("jr {}", rn(rs)),
            0x09 => format!("jalr {}, {}", rn(rd), rn(rs)),
            0x0A => format!("movz {}, {}, {}", rn(rd), rn(rs), rn(rt)),
            0x0B => format!("movn {}, {}, {}", rn(rd), rn(rs), rn(rt)),
            0x0C => "syscall".to_string(),
            0x0D => "break".to_string(),
            0x0F => "sync".to_string(),
            0x10 => format!("mfhi {}", rn(rd)),
            0x11 => format!("mthi {}", rn(rs)),
            0x12 => format!("mflo {}", rn(rd)),
            0x13 => format!("mtlo {}", rn(rs)),
            0x18 => format!("mult {}, {}", rn(rs), rn(rt)),
            0x19 => format!("multu {}, {}", rn(rs), rn(rt)),
            0x1A => format!("div {}, {}", rn(rs), rn(rt)),
            0x1B => format!("divu {}, {}", rn(rs), rn(rt)),
            0x21 => format!("addu {}, {}, {}", rn(rd), rn(rs), rn(rt)),
            0x23 => format!("subu {}, {}, {}", rn(rd), rn(rs), rn(rt)),
            0x24 => format!("and {}, {}, {}", rn(rd), rn(rs), rn(rt)),
            0x25 => format!("or {}, {}, {}", rn(rd), rn(rs), rn(rt)),
            0x26 => format!("xor {}, {}, {}", rn(rd), rn(rs), rn(rt)),
            0x27 => format!("nor {}, {}, {}", rn(rd), rn(rs), rn(rt)),
            0x2A => format!("slt {}, {}, {}", rn(rd), rn(rs), rn(rt)),
            0x2B => format!("sltu {}, {}, {}", rn(rd), rn(rs), rn(rt)),
            _ => format!("special(0x{funct:02x})"),
        },
        0x01 => {
            let name = match rt {
                0x00 => "bltz",
                0x01 => "bgez",
                0x02 => "bltzl",
                0x03 => "bgezl",
                0x10 => "bltzal",
                0x11 => "bgezal",
                0x12 => "bltzall",
                0x13 => "bgezall",
                _ => "regimm?",
            };
            format!("{name} {}, {simm:+}", rn(rs))
        }
        0x02 => format!("j 0x{:08x}", (word & 0x03FF_FFFF) << 2),
        0x03 => format!("jal 0x{:08x}", (word & 0x03FF_FFFF) << 2),
        0x04 => format!("beq {}, {}, {simm:+}", rn(rs), rn(rt)),
        0x05 => format!("bne {}, {}, {simm:+}", rn(rs), rn(rt)),
        0x06 => format!("blez {}, {simm:+}", rn(rs)),
        0x07 => format!("bgtz {}, {simm:+}", rn(rs)),
        0x08 | 0x09 => format!("addiu {}, {}, {simm:+}", rn(rt), rn(rs)),
        0x0A => format!("slti {}, {}, {simm:+}", rn(rt), rn(rs)),
        0x0B => format!("sltiu {}, {}, {simm:+}", rn(rt), rn(rs)),
        0x0C => format!("andi {}, {}, 0x{imm:04x}", rn(rt), rn(rs)),
        0x0D => format!("ori {}, {}, 0x{imm:04x}", rn(rt), rn(rs)),
        0x0E => format!("xori {}, {}, 0x{imm:04x}", rn(rt), rn(rs)),
        0x0F => format!("lui {}, 0x{imm:04x}", rn(rt)),
        0x10 => {
            let sel = word & 0x7;
            if word & (1 << 25) == 0 {
                match (word >> 21) & 0x1F {
                    0x00 => format!("mfc0 {}, c0({rdu},{sel})", rn(rt)),
                    0x04 => format!("mtc0 {}, c0({rdu},{sel})", rn(rt)),
                    _ => "cop0?".to_string(),
                }
            } else {
                match funct {
                    0x18 => "eret".to_string(),
                    _ => format!("cop0-co(0x{funct:02x})"),
                }
            }
        }
        0x14 => format!("beql {}, {}, {simm:+}", rn(rs), rn(rt)),
        0x15 => format!("bnel {}, {}, {simm:+}", rn(rs), rn(rt)),
        0x16 => format!("blezl {}, {simm:+}", rn(rs)),
        0x17 => format!("bgtzl {}, {simm:+}", rn(rs)),
        0x1C => match funct {
            0x02 => format!("mul {}, {}, {}", rn(rd), rn(rs), rn(rt)),
            0x20 => format!("clz {}, {}", rn(rd), rn(rs)),
            0x21 => format!("clo {}, {}", rn(rd), rn(rs)),
            _ => format!("special2(0x{funct:02x})"),
        },
        0x1F => match funct {
            0x00 => format!("ext {}, {}, {}, {}", rn(rt), rn(rs), sh, rd + 1),
            0x04 => format!(
                "ins {}, {}, {}, {}",
                rn(rt),
                rn(rs),
                sh,
                (rd as u32) - sh + 1
            ),
            0x20 => match (word >> 6) & 0x1F {
                0x02 => format!("wsbh {}, {}", rn(rd), rn(rt)),
                0x10 => format!("seb {}, {}", rn(rd), rn(rt)),
                0x18 => format!("seh {}, {}", rn(rd), rn(rt)),
                _ => "bshfl?".to_string(),
            },
            0x3B => format!("rdhwr {}, {rdu}", rn(rt)),
            _ => format!("special3(0x{funct:02x})"),
        },
        0x20 => format!("lb {}, {simm:+}({})", rn(rt), rn(rs)),
        0x21 => format!("lh {}, {simm:+}({})", rn(rt), rn(rs)),
        0x22 => format!("lwl {}, {simm:+}({})", rn(rt), rn(rs)),
        0x23 => format!("lw {}, {simm:+}({})", rn(rt), rn(rs)),
        0x24 => format!("lbu {}, {simm:+}({})", rn(rt), rn(rs)),
        0x25 => format!("lhu {}, {simm:+}({})", rn(rt), rn(rs)),
        0x26 => format!("lwr {}, {simm:+}({})", rn(rt), rn(rs)),
        0x28 => format!("sb {}, {simm:+}({})", rn(rt), rn(rs)),
        0x29 => format!("sh {}, {simm:+}({})", rn(rt), rn(rs)),
        0x2A => format!("swl {}, {simm:+}({})", rn(rt), rn(rs)),
        0x2B => format!("sw {}, {simm:+}({})", rn(rt), rn(rs)),
        0x2E => format!("swr {}, {simm:+}({})", rn(rt), rn(rs)),
        0x2F => "cache".to_string(),
        0x30 => format!("ll {}, {simm:+}({})", rn(rt), rn(rs)),
        0x33 => "pref".to_string(),
        0x38 => format!("sc {}, {simm:+}({})", rn(rt), rn(rs)),
        _ => format!("op(0x{op:02x})"),
    }
}

/// MIPS register name table ($zero..$ra).
pub const REG_NAMES: [&str; 32] = [
    "zero", "at", "v0", "v1", "a0", "a1", "a2", "a3", "t0", "t1", "t2", "t3", "t4", "t5", "t6",
    "t7", "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7", "t8", "t9", "k0", "k1", "gp", "sp", "fp",
    "ra",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::AddressSpace;

    /// Build an address space with 64 KiB RAM at phys 0 (kseg0 0x80000000),
    /// load `words` at the start, and return (cpu, bus).
    fn setup(words: &[u32]) -> (Mips32, AddressSpace) {
        let mut bus = AddressSpace::new();
        let mut bytes = Vec::with_capacity(words.len() * 4);
        for w in words {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        bus.add_rom("prog", 0x0000_0000, bytes);
        bus.add_ram("ram", 0x0001_0000, 0x0001_0000);
        let mut cpu = Mips32::new();
        cpu.reset(0x8000_0000);
        (cpu, bus)
    }

    fn run(cpu: &mut Mips32, bus: &mut AddressSpace, steps: usize) {
        for _ in 0..steps {
            cpu.step(bus).expect("step failed");
        }
    }

    const T0: usize = 8;
    const T1: usize = 9;
    const T2: usize = 10;
    const SP: usize = 29;
    const RA: usize = 31;

    fn lui(rt: usize, imm: u16) -> u32 {
        (0x0F << 26) | ((rt as u32) << 16) | u32::from(imm)
    }
    fn ori(rt: usize, rs: usize, imm: u16) -> u32 {
        (0x0D << 26) | ((rs as u32) << 21) | ((rt as u32) << 16) | u32::from(imm)
    }
    fn addiu(rt: usize, rs: usize, simm: i16) -> u32 {
        (0x09 << 26) | ((rs as u32) << 21) | ((rt as u32) << 16) | (simm as u16 as u32)
    }
    fn special(rs: usize, rt: usize, rd: usize, sh: u32, funct: u32) -> u32 {
        (rs as u32) << 21 | (rt as u32) << 16 | (rd as u32) << 11 | sh << 6 | funct
    }

    #[test]
    fn lui_ori_build_constant() {
        let (mut cpu, mut bus) = setup(&[lui(T0, 0x1234), ori(T0, T0, 0x5678)]);
        run(&mut cpu, &mut bus, 2);
        assert_eq!(cpu.reg(T0), 0x1234_5678);
    }

    #[test]
    fn addiu_sign_extension() {
        let (mut cpu, mut bus) = setup(&[
            ori(T0, 0, 0x0001),
            addiu(T1, T0, -2), // 1 + (-2) = -1
        ]);
        run(&mut cpu, &mut bus, 2);
        assert_eq!(cpu.reg(T1), 0xFFFF_FFFF);
    }

    #[test]
    fn sw_lw_roundtrip_and_byte_lanes() {
        let (mut cpu, mut bus) = setup(&[
            lui(T0, 0x8001),                                          // RAM base (phys 0x10000)
            ori(T0, T0, 0x0100),                                      // +0x100
            lui(T1, 0xDEAD),                                          // value hi
            ori(T1, T1, 0xBEEF),                                      // value = 0xDEADBEEF
            (0x2B << 26) | ((T0 as u32) << 21) | ((T1 as u32) << 16), // sw t1, 0(t0)
            (0x23 << 26) | ((T0 as u32) << 21) | ((T2 as u32) << 16), // lw t2, 0(t0)
        ]);
        run(&mut cpu, &mut bus, 6);
        assert_eq!(cpu.reg(T2), 0xDEAD_BEEF);
        // byte lanes little-endian
        assert_eq!(bus.load(0x8001_0100, AccessSize::Byte).unwrap(), 0xEF);
        assert_eq!(bus.load(0x8001_0103, AccessSize::Byte).unwrap(), 0xDE);
    }

    #[test]
    fn beq_taken_executes_delay_slot_then_branches() {
        // 0x00: beq $zero,$zero,+2  (target = 0x00+4 + 8 = 0x0C)
        // 0x04: addiu t0,zero,1     (delay slot — must execute)
        // 0x08: addiu t1,zero,99    (skipped by branch)
        // 0x0C: addiu t2,zero,2     (branch target)
        let beq = (0x04 << 26) | 2; // offset 2 words from pc+4
        let (mut cpu, mut bus) = setup(&[beq, addiu(T0, 0, 1), addiu(T1, 0, 99), addiu(T2, 0, 2)]);
        run(&mut cpu, &mut bus, 3); // beq, delay slot, target instr
        assert_eq!(cpu.reg(T0), 1, "delay slot must execute");
        assert_eq!(cpu.reg(T1), 0, "skipped instruction must not execute");
        assert_eq!(cpu.reg(T2), 2, "branch target must execute");
        assert_eq!(cpu.pc(), 0x8000_0010);
    }

    #[test]
    fn beql_not_taken_annuls_delay_slot() {
        // bnel $zero,$zero: 0 != 0 is false → NOT taken → annul delay slot.
        // Program: [bnel, addiu t0(annulled), addiu t1, addiu t2]
        let bnel = (0x15 << 26) | 2;
        let (mut cpu, mut bus) = setup(&[
            bnel,
            addiu(T0, 0, 1),  // annulled!
            addiu(T1, 0, 99), // executes after the annulled slot
            addiu(T2, 0, 2),
        ]);
        run(&mut cpu, &mut bus, 2);
        assert_eq!(
            cpu.reg(T0),
            0,
            "delay slot of not-taken likely branch must be annulled"
        );
        assert_eq!(
            cpu.reg(T1),
            99,
            "instruction after the annulled slot executes"
        );
        assert_eq!(cpu.pc(), 0x8000_000C);
    }

    #[test]
    fn jal_jr_call_and_return() {
        // 0x00: jal 0x14 (function)
        // 0x04: nop (delay)
        // 0x08: addiu t3,zero,7   (return lands here)
        // 0x0C..0x13: nops
        // 0x14: addiu v0,zero,42; jr ra; nop
        let jal = (0x03 << 26) | (0x14 >> 2);
        let jr_ra = special(RA, 0, 0, 0, 0x08);
        let (mut cpu, mut bus) = setup(&[
            jal,
            0, // delay
            addiu(11, 0, 7),
            0,
            0,
            0,               // pad to 0x14
            addiu(2, 0, 42), // v0=42 at 0x14
            jr_ra,           // 0x18
            0,               // delay 0x1C
        ]);
        run(&mut cpu, &mut bus, 7);
        assert_eq!(cpu.reg(2), 42);
        assert_eq!(cpu.reg(RA), 0x8000_0008, "ra = address after delay slot");
        assert_eq!(cpu.reg(11), 7, "execution returns after the call");
        assert_eq!(cpu.pc(), 0x8000_000C);
    }

    #[test]
    fn ext_ins_bitfields() {
        // t0 = 0x12345678
        // ext t1, t0, pos=8, size=8  → 0x56
        // ori t3, zero, 0xAB ; ins t0, t3, pos=0, size=8 → t0 = 0x123456AB
        const T3: usize = 11;
        // ext/ins encoding: RD field = msb/msbd, SA field = pos/lsb
        let ext = (0x1F << 26)
            | ((T0 as u32) << 21)  // rs = source
            | ((T1 as u32) << 16)  // rt = destination
            | (7 << 11)            // msbd = size-1 = 7
            | (8 << 6);
        let ins = ((0x1F << 26)
            | ((T3 as u32) << 21)  // rs = source
            | ((T0 as u32) << 16)  // rt = destination
            | (7 << 11))             // pos = 0
            | 0x04;
        let (mut cpu, mut bus) = setup(&[
            lui(T0, 0x1234),
            ori(T0, T0, 0x5678),
            ext,
            ori(T3, 0, 0xAB),
            ins,
        ]);
        run(&mut cpu, &mut bus, 5);
        assert_eq!(cpu.reg(T1), 0x56, "ext extracts bits [15:8]");
        assert_eq!(cpu.reg(T0), 0x1234_56AB, "ins replaces bits [7:0]");
    }

    #[test]
    fn mul_mult_div() {
        let mul =
            (0x1C << 26) | ((T0 as u32) << 21) | ((T1 as u32) << 16) | ((T2 as u32) << 11) | 0x02;
        let (mut cpu, mut bus) = setup(&[
            ori(T0, 0, 100),
            ori(T1, 0, 7),
            mul,                         // mul t2, t0, t1 → 700
            special(T0, T1, 0, 0, 0x18), // mult t0,t1 → hi=0 lo=700
            special(0, 0, T2, 0, 0x10),  // mfhi t2
            special(0, 0, T3, 0, 0x12),  // mflo t3
        ]);
        const T3: usize = 11;
        run(&mut cpu, &mut bus, 3);
        assert_eq!(cpu.reg(T2), 700, "SPECIAL2 mul");
        run(&mut cpu, &mut bus, 3);
        assert_eq!(cpu.reg(T2), 0, "hi of 100*7 = 0");
        assert_eq!(cpu.reg(T3), 700, "lo of 100*7 = 700");
    }

    #[test]
    fn slt_sltu_signedness() {
        let (mut cpu, mut bus) = setup(&[
            lui(T0, 0xFFFF),
            ori(T0, T0, 0xFFFF), // 0xFFFFFFFF = -1 signed
            ori(T1, 0, 1),
            special(T0, T1, T2, 0, 0x2A), // slt t2, t0, t1 → -1 < 1 → 1
            special(T0, T1, T3, 0, 0x2B), // sltu t3, t0, t1 → 0xFFFFFFFF < 1 → 0
        ]);
        const T3: usize = 11;
        run(&mut cpu, &mut bus, 5);
        assert_eq!(cpu.reg(T2), 1);
        assert_eq!(cpu.reg(T3), 0);
    }

    #[test]
    fn lwl_lwr_unaligned_load_le() {
        // memory at RAM+0x100: bytes 11 22 33 44 55 66 77 88
        // unaligned word at +1 = bytes [+1..+5] = 22 33 44 55 → 0x5544_3322
        let (mut cpu, mut bus) = setup(&[
            lui(T0, 0x8001),
            ori(T0, T0, 0x0100),
            lui(T1, 0x4433),
            ori(T1, T1, 0x2211),
            (0x2B << 26) | ((T0 as u32) << 21) | ((T1 as u32) << 16), // sw t1, 0(t0) → 11 22 33 44
            lui(T1, 0x8877),
            ori(T1, T1, 0x6655),
            (0x2B << 26) | ((T0 as u32) << 21) | ((T1 as u32) << 16) | 4, // sw t1, 4(t0) → 55 66 77 88
            // lwr t2, 1(t0) ; lwl t2, 4(t0)  → word at +1
            (0x26 << 26) | ((T0 as u32) << 21) | ((T2 as u32) << 16) | 1,
            (0x22 << 26) | ((T0 as u32) << 21) | ((T2 as u32) << 16) | 4,
        ]);
        run(&mut cpu, &mut bus, 10);
        assert_eq!(cpu.reg(T2), 0x5544_3322, "unaligned LE load of bytes at +1");
    }

    #[test]
    fn swl_swr_unaligned_store_le() {
        // store 0xAABBCCDD at +1: bytes at +1..+4 become BB CC DD AA? verify:
        // LE unaligned store word V at addr p: mem[p..p+4] = V bytes LE.
        // p=+1: mem[+1]=BB? V=0xAABBCCDD → LE bytes: DD(CC? no) DD CC BB AA.
        let (mut cpu, mut bus) = setup(&[
            lui(T0, 0x8001),
            ori(T0, T0, 0x0100),
            lui(T1, 0xAABB),
            ori(T1, T1, 0xCCDD),
            (0x2E << 26) | ((T0 as u32) << 21) | ((T1 as u32) << 16) | 1, // swr t1, 1(t0)
            (0x2A << 26) | ((T0 as u32) << 21) | ((T1 as u32) << 16) | 4, // swl t1, 4(t0)
            (0x24 << 26) | ((T0 as u32) << 21) | ((T2 as u32) << 16) | 1, // lbu t2, 1(t0)
        ]);
        run(&mut cpu, &mut bus, 7);
        // mem[+1..+5] must equal LE bytes of 0xAABBCCDD: DD CC BB AA
        assert_eq!(bus.load(0x8001_0101, AccessSize::Byte).unwrap(), 0xDD);
        assert_eq!(bus.load(0x8001_0102, AccessSize::Byte).unwrap(), 0xCC);
        assert_eq!(bus.load(0x8001_0103, AccessSize::Byte).unwrap(), 0xBB);
        assert_eq!(bus.load(0x8001_0104, AccessSize::Byte).unwrap(), 0xAA);
        assert_eq!(cpu.reg(T2), 0xDD);
    }

    #[test]
    fn cop0_mtc0_mfc0_and_config1() {
        // mtc0 rt, rd: source register goes in the RT field
        let mtc0 = (0x10 << 26) | (0x04 << 21) | ((T0 as u32) << 16) | (28 << 11); // mtc0 t0, TagLo
        let mfc0 = (0x10 << 26) | ((T1 as u32) << 16) | (28 << 11); // mfc0 t1, TagLo
        let mfc0_cfg1 = (0x10 << 26) | ((T2 as u32) << 16) | (16 << 11) | 1; // mfc0 t2, Config1
        let (mut cpu, mut bus) =
            setup(&[lui(T0, 0xCAFE), ori(T0, T0, 0xBABE), mtc0, mfc0, mfc0_cfg1]);
        run(&mut cpu, &mut bus, 5);
        assert_eq!(cpu.reg(T1), 0xCAFE_BABE);
        let cfg1 = cpu.reg(T2);
        let il = (cfg1 >> 19) & 7;
        assert_eq!(2u32 << il, 32, "Config1 reports 32-byte I-cache lines");
    }

    #[test]
    fn unknown_instruction_is_clean_error() {
        let (mut cpu, mut bus) = setup(&[0xFFFF_FFFF]); // undefined opcode 0x3F
        let err = cpu.step(&mut bus).unwrap_err();
        assert!(matches!(err, CpuError::UnknownInstruction { .. }));
    }

    #[test]
    fn fetch_fault_is_clean_error() {
        let (mut cpu, mut bus) = setup(&[0]);
        cpu.reset(0x9000_0000); // unmapped physical
        let err = cpu.step(&mut bus).unwrap_err();
        assert!(matches!(err, CpuError::Fault { .. }));
    }

    #[test]
    fn cop0_count_ticks_deterministically() {
        let mfc0_count = (0x10 << 26) | ((T0 as u32) << 16) | (9 << 11);
        let (mut cpu, mut bus) = setup(&[0, 0, 0, mfc0_count]);
        run(&mut cpu, &mut bus, 3); // three nops → Count = 6
        assert_eq!(cpu.cop0().read(9, 0), 6);
        // The mfc0 reads Count (6), then the instruction retires and Count
        // ticks to 8 — the loaded value is the pre-tick value.
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.reg(T0), 6, "mfc0 loads the pre-tick Count");
        assert_eq!(
            cpu.cop0().read(9, 0),
            8,
            "Count advances 2 per retired instruction"
        );
    }

    #[test]
    fn zero_register_is_immutable() {
        let (mut cpu, mut bus) = setup(&[ori(0, 0, 0x1234)]);
        run(&mut cpu, &mut bus, 1);
        assert_eq!(cpu.reg(0), 0);
    }

    #[test]
    fn store_to_unmapped_window_faults_cleanly() {
        // Loader-like prologue: sp = 0xBFFC0E00 then push. In the bare test
        // setup there is no IRAM overlay mapped, so the store must fault
        // cleanly (this is the exact situation the full machine resolves by
        // mapping the overlay — see tests/boot.rs).
        let (mut cpu, mut bus) = setup(&[
            lui(SP, 0xBFFC),
            ori(SP, SP, 0x0E00),
            addiu(SP, SP, -8),
            ((0x2B << 26) | ((SP as u32) << 21)), // sw zero, 0(sp)
        ]);
        run(&mut cpu, &mut bus, 3);
        assert_eq!(cpu.reg(SP), 0xBFFC_0DF8);
        let err = cpu.step(&mut bus).unwrap_err();
        assert!(
            matches!(err, CpuError::Fault { .. }),
            "store to unmapped kseg1 must fault"
        );
    }

    // ---- interrupt delivery (Milestone G) ----
    //
    // Step model notes (why the assertions use "next fetch address"):
    // `pc()` after `step()` is the address of the NEXT instruction to
    // fetch. Interrupt delivery happens at the start of a step (before
    // fetch) and consumes no step; `eret` performs its transfer as the
    // Flow of its own step. A handler [nop, eret] therefore takes:
    //   step A: deliver (pc->0x...180), execute nop (pc->0x...184)
    //   step B: execute eret (pc -> EPC)
    //   step C: if the line is still high, deliver again.

    fn mtc0(rt: usize, rd: u8, sel: u8) -> u32 {
        (0x10 << 26) | (0x04 << 21) | ((rt as u32) << 16) | ((rd as u32) << 11) | u32::from(sel)
    }
    const MFC0_T0_STATUS: u32 = (0x10 << 26) | ((T0 as u32) << 16) | (12 << 11);
    const ERET: u32 = 0x4200_0018;

    /// Build a core + bus with the program at kseg0 0x80000000, RAM covering
    /// the BEV=0 interrupt vector (phys 0x180, via kseg0 0x80000180) and a
    /// RAM window for the BEV=1 vector at 0xBFC00380 (phys 0x1FC00380).
    /// `handler` is copied to both vector sites.
    fn irq_setup(prog: &[u32], handler: &[u32]) -> (Mips32, AddressSpace) {
        let mut bus = AddressSpace::new();
        let mut bytes = Vec::with_capacity(prog.len() * 4);
        for w in prog {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        bus.add_rom("prog", 0x0000_0000, bytes); // kseg0 0x80000000
        bus.add_ram("vector", 0x0000_0100, 0x1000); // covers phys 0x180
        bus.add_ram("bootvec", 0x1FC0_0000, 0x1000); // kseg1 0xBFC00000
        let mut cpu = Mips32::new();
        cpu.reset(0x8000_0000);
        for (name, base) in [("vector", 0x8000_0180u32), ("bootvec", 0xBFC0_0380u32)] {
            for (i, w) in handler.iter().enumerate() {
                bus.store(base + (i as u32) * 4, AccessSize::Word, *w)
                    .unwrap_or_else(|e| panic!("{name}: {e}"));
            }
        }
        (cpu, bus)
    }

    /// Program preamble: `Status = (Status | IE | IM2) & 0xFFFB`.
    /// NOTE: `andi` zero-extends its 16-bit immediate, so this also clears
    /// the entire upper half of Status — including BEV. After the preamble
    /// Status = 0x0000_0401 (IE|IM2, kernel mode, ERL/EXL/BEV all clear),
    /// so interrupts vector to EBase + 0x180 = 0x8000_0180. (The BEV=1
    /// path is covered separately by `bev_one_vectors_to_boot_rom_window`.)
    fn enable_ie_im2() -> Vec<u32> {
        vec![
            MFC0_T0_STATUS,
            ori(T0, T0, 0x0401), // IE (bit 0) | IM2 (bit 10)
            (0x0C << 26) | ((T0 as u32) << 21) | ((T0 as u32) << 16) | 0xFFFB, // andi t0, t0, ~ERL
            mtc0(T0, 12, 0),
        ]
    }

    #[test]
    fn enabled_ext_interrupt_vectors_to_handler_and_eret_returns() {
        // Handler: sw marker; eret. Main: arm interrupts, set t0/t1, loop.
        let handler = [
            (0x2B << 26) | ((T0 as u32) << 21) | ((T1 as u32) << 16) | 0x1000, // sw t1, 0x1000(t0)
            ERET,
        ];
        let mut prog = enable_ie_im2();
        prog.extend_from_slice(&[lui(T0, 0x8000), addiu(T1, 0, 0x55)]);
        // Self-jump loop directly after the setup words.
        let loop_pc = 0x8000_0000 + (prog.len() as u32) * 4;
        let idx = (loop_pc & 0x0FFF_FFFF) >> 2;
        prog.extend_from_slice(&[0, 0]); // placeholder for j + delay slot
        let n = prog.len();
        prog[n - 2] = (0x02 << 26) | idx; // j loop_pc (at address loop_pc)
        prog[n - 1] = 0; // delay slot
        let (mut cpu, mut bus) = irq_setup(&prog, &handler);
        run(&mut cpu, &mut bus, prog.len() - 2); // arm + lui + addiu
        assert_eq!(cpu.pc(), loop_pc, "must be at the self-loop");

        // No interrupt yet: nothing may have run the handler.
        assert_eq!(
            bus.load(0x8000_1000, AccessSize::Word).unwrap(),
            0,
            "handler must not run before the line is raised"
        );

        // Raise IP2 (ext line bit 0 → IP2).
        cpu.set_ext_irq_lines(0b00_0001);
        // Step A: deliver to 0x80000180 and execute the handler's sw.
        let rep = cpu.step(&mut bus).unwrap();
        assert_eq!(rep.pc, 0x8000_0180, "delivery lands on the vector");
        assert_eq!(cpu.pc(), 0x8000_0184, "next fetch after handler sw");
        assert_eq!(
            bus.load(0x8000_1000, AccessSize::Word).unwrap(),
            0x55,
            "handler store must execute"
        );
        let st = cpu.cop0().status();
        assert_eq!(st & c0_bits::EXL, c0_bits::EXL, "EXL set on entry");
        let cause = cpu.cop0().read(13, 0);
        assert_eq!(cause & c0_cause::EXCCODE, 0, "ExcCode = Int");
        assert_eq!(cpu.cop0().read(14, 0), loop_pc, "EPC = interrupted PC");
        assert_eq!(cause & c0_cause::BD, 0, "not in a delay slot");
        // Step B: eret returns to EPC and clears EXL.
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc(), loop_pc, "eret returns to EPC");
        assert_eq!(cpu.cop0().status() & c0_bits::EXL, 0, "eret clears EXL");
    }

    #[test]
    fn disabled_or_masked_interrupts_do_not_vector() {
        let handler = [0, ERET];
        // Case 1: IE=0 (reset state) — lines raised, no delivery.
        let (mut cpu, mut bus) = irq_setup(&[0, 0], &handler);
        cpu.set_ext_irq_lines(0x3F);
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc(), 0x8000_0004, "IE=0: no delivery");
        // Case 2: IE=1 but IM2=0 — still no delivery.
        let prog2 = vec![
            MFC0_T0_STATUS,
            ori(T0, T0, 0x0001), // IE only
            (0x0C << 26) | ((T0 as u32) << 21) | ((T0 as u32) << 16) | 0xFFFB,
            mtc0(T0, 12, 0),
            0, // nop
        ];
        let (mut cpu2, mut bus2) = irq_setup(&prog2, &handler);
        run(&mut cpu2, &mut bus2, 4);
        cpu2.set_ext_irq_lines(0x01);
        cpu2.step(&mut bus2).unwrap(); // executes the trailing nop
        assert_eq!(cpu2.pc(), 0x8000_0014, "IM2=0: no delivery");
    }

    #[test]
    fn level_line_retriggers_until_cleared() {
        // Level-triggered semantics: while the line is asserted and
        // enabled, returning from the handler re-enters immediately.
        let handler = [0, ERET]; // nop; eret
        let mut prog = enable_ie_im2();
        prog.extend_from_slice(&[addiu(T1, 0, 7), 0]); // addiu @0x10, nop @0x14
        let (mut cpu, mut bus) = irq_setup(&prog, &handler);
        run(&mut cpu, &mut bus, 5); // arm + addiu; next fetch = 0x14
        let loop_pc = 0x8000_0014;

        cpu.set_ext_irq_lines(0b00_0001);
        // Entry 1: deliver + handler nop.
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc(), 0x8000_0184);
        assert_eq!(cpu.cop0().read(14, 0), loop_pc);
        // Return.
        cpu.step(&mut bus).unwrap(); // eret -> EPC
        assert_eq!(cpu.pc(), loop_pc);
        // Entry 2 (line still high): re-delivery on the next step.
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc(), 0x8000_0184, "level line re-vectors");
        // Clear the line, return, and confirm no further delivery.
        cpu.set_ext_irq_lines(0);
        cpu.step(&mut bus).unwrap(); // eret -> EPC
        assert_eq!(cpu.pc(), loop_pc);
        cpu.step(&mut bus).unwrap(); // executes the nop at loop_pc
        assert_eq!(cpu.pc(), loop_pc + 4, "no delivery after line cleared");
    }

    #[test]
    fn delay_slot_interrupt_sets_bd_and_epc() {
        // Interrupt while in a branch delay slot: EPC = branch address,
        // Cause.BD = 1, and eret re-executes the branch.
        let handler = [0, ERET];
        let mut prog = enable_ie_im2();
        prog.extend_from_slice(&[
            (0x04 << 26) | 1, // beq $zero,$zero,+1 -> 0x1C
            addiu(T1, 0, 1),  // delay slot @0x14
            addiu(T2, 0, 2),  // (skipped) @0x18
        ]);
        let branch_pc = 0x8000_0010;
        let (mut cpu, mut bus) = irq_setup(&prog, &handler);
        run(&mut cpu, &mut bus, 5); // arm(4) + beq; now at the delay slot
        assert_eq!(cpu.pc(), branch_pc + 4, "must be in the delay slot");

        cpu.set_ext_irq_lines(0b00_0001);
        // Entry: deliver + handler nop. EPC must be the BRANCH address.
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc(), 0x8000_0184);
        assert_eq!(cpu.cop0().read(14, 0), branch_pc, "EPC = branch address");
        assert_eq!(
            cpu.cop0().read(13, 0) & c0_cause::BD,
            c0_cause::BD,
            "BD set for delay-slot interrupt"
        );
        // Return: eret jumps to the branch (delay-slot semantics).
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc(), branch_pc, "eret resumes at the branch");
    }

    #[test]
    fn bev_one_vectors_to_boot_rom_window() {
        // BEV=1 + IE + IM2: vector = 0xBFC0_0200 + 0x180 = 0xBFC0_0380.
        let handler = [0, ERET];
        let prog = vec![
            MFC0_T0_STATUS,
            lui(T1, 0x0040), // BEV
            ori(T1, T1, 0x0401),
            (0x0C << 26) | ((T0 as u32) << 21) | ((T0 as u32) << 16) | 0xFFFB,
            special(T0, T1, T0, 0, 0x25), // or t0, t0, t1
            mtc0(T0, 12, 0),
            0, // nop (interrupted here)
        ];
        let (mut cpu, mut bus) = irq_setup(&prog, &handler);
        run(&mut cpu, &mut bus, 7);
        cpu.set_ext_irq_lines(0b00_0001);
        cpu.step(&mut bus).unwrap(); // deliver to ROM vector + handler nop
        assert_eq!(cpu.pc(), 0xBFC0_0384, "BEV=1 ROM interrupt vector");
    }

    #[test]
    fn exl_blocks_nesting() {
        // While EXL=1 (inside a handler) a new line must not re-vector
        // until eret clears EXL.
        let handler = [0, 0, ERET]; // nop; nop; eret
        let mut prog = enable_ie_im2();
        prog.extend_from_slice(&[0, 0]); // nops @0x10, 0x14
        let (mut cpu, mut bus) = irq_setup(&prog, &handler);
        run(&mut cpu, &mut bus, 6); // arm(4) + nop@0x10 + nop@0x14 → next 0x18
        assert_eq!(cpu.pc(), 0x8000_0018);
        cpu.set_ext_irq_lines(0b00_0001);
        // Entry: deliver + first handler nop. While at 0x80000184..0x188
        // EXL=1; raise more lines and verify they do not nest.
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc(), 0x8000_0184);
        cpu.set_ext_irq_lines(0b11_1111);
        cpu.step(&mut bus).unwrap(); // second handler nop — must not vector
        assert_eq!(cpu.pc(), 0x8000_0188, "EXL blocks nesting");
        cpu.set_ext_irq_lines(0);
        cpu.step(&mut bus).unwrap(); // eret
        assert_eq!(cpu.pc(), 0x8000_0018);
    }

    #[test]
    fn cause_write_mask_preserves_hardware_fields() {
        // mtc0 Cause may only change IV/DC/IP[1:0]; BD, ExcCode and
        // IP[7:2] are hardware-owned.
        let mut cop = Cop0::new();
        cop.set_ip_hw(0b00_0001);
        let _ = cop.enter_exception(7, 0x100, true);
        let pre = cop.read(13, 0);
        assert_eq!(pre & c0_cause::BD, c0_cause::BD);
        assert_eq!((pre & c0_cause::EXCCODE) >> 2, 7);
        assert_eq!(pre & c0_cause::IP_HW, 1 << 10);
        cop.write(13, 0, 0x4000_0101); // IV=1, IP1:0=01
        let post = cop.read(13, 0);
        assert_eq!(post & c0_cause::IV, c0_cause::IV, "IV writable");
        assert_eq!(post & c0_cause::IP_SW, 0x100, "IP1:0 writable");
        assert_eq!(post & c0_cause::BD, c0_cause::BD, "BD preserved");
        assert_eq!((post & c0_cause::EXCCODE) >> 2, 7, "ExcCode preserved");
        assert_eq!(post & c0_cause::IP_HW, 1 << 10, "IP[7:2] preserved");
    }
}
