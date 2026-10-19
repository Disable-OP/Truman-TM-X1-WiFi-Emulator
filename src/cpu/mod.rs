//! CPU backend abstraction.
//!
//! The emulator core talks to the CPU only through [`CpuBackend`], so a
//! different execution engine (e.g. a future native-Unicorn integration or
//! a QEMU-based backend) can be added without touching machine or device
//! code. The current backend is [`mips32::Mips32`], a pure-Rust MIPS32r2
//! little-endian interpreter — see docs/firmware-analysis.md §7 for why a
//! custom core was selected.

pub mod cop0;
pub mod mips32;

use crate::memory::{AccessFault, AddressSpace};

/// A single observed memory access during instruction execution.
#[derive(Debug, Clone, Copy)]
pub struct MemEvent {
    /// True for stores, false for loads.
    pub write: bool,
    /// Virtual address used by the instruction.
    pub vaddr: u32,
    /// Access width.
    pub size: crate::memory::AccessSize,
    /// Value loaded (zero-extended) or stored.
    pub value: u32,
}

/// Result of one successfully executed instruction.
#[derive(Debug, Clone)]
pub struct StepReport {
    /// Address of the executed instruction.
    pub pc: u32,
    /// Raw instruction word.
    pub word: u32,
    /// Compact textual description (mnemonic + operands).
    pub desc: String,
    /// Memory accesses performed by this instruction (0–4 events).
    pub mem: Vec<MemEvent>,
}

/// Errors that terminate emulation cleanly (never panic, never loop).
#[derive(Debug, Clone)]
pub enum CpuError {
    /// Memory fault (fetch/read/write/alignment).
    Fault { pc: u32, fault: AccessFault },
    /// Opcode not implemented by this backend.
    UnknownInstruction { pc: u32, word: u32 },
    /// Encodable but not supported / would require state we don't model.
    Unsupported { pc: u32, word: u32, reason: String },
    /// BREAK instruction.
    Break { pc: u32, code: u32 },
    /// SYSCALL instruction.
    Syscall { pc: u32 },
    /// Trap instruction condition fired.
    Trap { pc: u32, word: u32 },
}

impl std::fmt::Display for CpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CpuError::Fault { pc, fault } => write!(f, "cpu fault at pc=0x{pc:08x}: {fault}"),
            CpuError::UnknownInstruction { pc, word } => {
                write!(f, "unknown instruction at pc=0x{pc:08x}: 0x{word:08x}")
            }
            CpuError::Unsupported { pc, word, reason } => {
                write!(
                    f,
                    "unsupported instruction at pc=0x{pc:08x}: 0x{word:08x} ({reason})"
                )
            }
            CpuError::Break { pc, code } => write!(f, "break at pc=0x{pc:08x} code={code:#x}"),
            CpuError::Syscall { pc } => write!(f, "syscall at pc=0x{pc:08x}"),
            CpuError::Trap { pc, word } => write!(f, "trap at pc=0x{pc:08x}: 0x{word:08x}"),
        }
    }
}

impl std::error::Error for CpuError {}

/// Register-file snapshot for debugging output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuSnapshot {
    pub pc: u32,
    pub next_pc: u32,
    pub regs: [u32; 32],
    pub hi: u32,
    pub lo: u32,
}

/// CPU backend interface used by the machine run loop.
pub trait CpuBackend {
    /// Reset the core and start fetching from `entry`.
    fn reset(&mut self, entry: u32);

    /// Current program counter.
    fn pc(&self) -> u32;

    /// Execute one instruction. Returns a report for tracing or an error
    /// that explains why execution had to stop.
    fn step(&mut self, bus: &mut AddressSpace) -> Result<StepReport, CpuError>;

    /// General register read (index 0..31).
    fn reg(&self, index: usize) -> u32;

    /// Full state snapshot.
    fn snapshot(&self) -> CpuSnapshot;

    /// Backend name (for diagnostics).
    fn name(&self) -> &'static str;

    /// Type-erased self reference for backend-specific diagnostics.
    fn as_any(&self) -> &dyn std::any::Any;
}
