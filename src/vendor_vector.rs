//! The vendor vector gate (§40.4): the SDK's own test container as a
//! mechanical second validation of any recovered value ladder.
//!
//! The vendor's `sp_gzip` test vector (`ZlibBuf` in the archived
//! `sphe-cmd_sp_png_dec.c`, `tools/reference/`) was compressed by the same
//! hardware engine against a *known* 9,216-byte pattern (9 chunks × 1024 —
//! the SP header's own chunk count). Milestone AM (§40) documented its
//! structure offline, byte-level: an RFC-canonical frame, a hex-character
//! literal backbone (§40.2: ≥99% of the literals are `[0-9a-f]`), an
//! engine-semantics decode that completes with zero invalid distances, and
//! RFC length accounting over-running the 9,216 anchor by +570/+377/+358
//! at the three candidate start bits (320/1024/1088).
//!
//! This module turns that documentation into code the solver executes:
//! [`vector_gate`] re-decodes the vector under candidate [`ValueTables`]
//! and reports, per start bit,
//!
//! * completion with zero invalid distances (the engine's own semantics),
//! * the clean output alphabet (`[0-9a-f]`, `\n`, `\r`, NUL only),
//! * the pinned literal backbone (≥990‰ hex, §40.2),
//! * the accounting against the container's own 9-chunk anchor (§40.1).
//!
//! The alphabet and the backbone are pinned by the stream *frame* (§19.3:
//! literal values are stream-determined — given the header and the
//! extra-bit widths, which are themselves stream-position data). They
//! validate the container, the parse and this implementation, and they
//! fail fast under any ladder that touches the widths. The accounting and
//! the invalid-distance check respond to the base *values*: within the
//! value family they alone discriminate. A ladder that fits the P1 capture
//! but fails here is wrong (docs/hardware-session.md, the mandatory
//! second validation).
//!
//! The reference implementation of the §40 data side (censuses, gap
//! structure, the §40.3 joint anchor sweep) remains
//! `tools/fw_vector_probe.py` — this module is the gate side, wired into
//! `ladder_solve` (Milestone AN). No emulator-semantics change; the §30.4
//! blocker stays hardware-gated.

use std::fmt;

use crate::inflate::{inflate_engine_with_traced, DecodeEventKind, InflateError, ValueTables};
use crate::machine::ENGINE_PRESET_BYTES;

/// The container's encoded size as declared by the vendor source:
/// `ZlibBuf[16*234]`.
pub const VECTOR_CONTAINER_BYTES: usize = 16 * 234;

/// The vector's decoded size per its own SP header: 9 chunks × 1024.
pub const VECTOR_ANCHOR_BYTES: usize = 9 * 1024;

/// The SP header's chunk count (the container's own accounting, §40.1).
pub const VECTOR_CHUNKS: u16 = 9;

/// The candidate symbol start bits (§40: the vendor driver's R3818 value
/// for the vector is not observed; the three documented candidates are the
/// Truman start 1024, its header-relative variant 1088, and the vector's
/// own header end 320).
pub const CANDIDATE_START_BITS: [usize; 3] = [320, 1024, 1088];

/// Output cap for the gate's decode. The RFC reference decode reaches
/// 9,786 bytes at the widest start bit; a recovered ladder near the truth
/// is in the same band. An absurd candidate can run away (u16 bases) — the
/// cap turns that into a gate failure, not a hang.
pub const VECTOR_MAX_OUT: usize = 1 << 20;

/// The pinned literal backbone floor, in per-mille of the literals
/// (§40.2: "≥99%").
pub const BACKBONE_PERMILLE_MIN: u64 = 990;

/// The vector's literal alphabet beyond the hex digits (§40.1).
const NON_HEX_ALPHABET: [u8; 3] = [b'\n', b'\r', 0];

/// A `VectorGateError` reading means the gate's *input* is unavailable —
/// an environment problem (exit code 2 territory), never a verdict about a
/// candidate ladder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorGateError {
    /// The `ZlibBuf[16*234] = {` marker is absent from the archived source.
    ContainerMarkerMissing,
    /// The array body has no closing `};`.
    ContainerTruncated,
    /// The parsed array is not the documented 3,744 bytes.
    ContainerBytes { expected: usize, found: usize },
    /// The container does not start with the SP magic.
    ContainerMagic { found: [u8; 2] },
    /// The container's own accounting is inconsistent (chunk count,
    /// declared stream length, or derived output anchor).
    ContainerAccounting {
        chunks: u16,
        stream_len: u32,
        bytes: usize,
    },
    /// The archived vendor source could not be read from either the build
    /// checkout or the working directory.
    ReferenceUnavailable { path: String },
}

impl fmt::Display for VectorGateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VectorGateError::ContainerMarkerMissing => {
                write!(
                    f,
                    "ZlibBuf[16*234] marker not found in archived vendor source"
                )
            }
            VectorGateError::ContainerTruncated => {
                write!(f, "ZlibBuf array body has no closing '}};'")
            }
            VectorGateError::ContainerBytes { expected, found } => {
                write!(f, "ZlibBuf array is {found} bytes, expected {expected}")
            }
            VectorGateError::ContainerMagic { found } => {
                write!(f, "container magic is {:02x?}, expected \"SP\"", found)
            }
            VectorGateError::ContainerAccounting {
                chunks,
                stream_len,
                bytes,
            } => write!(
                f,
                "container accounting inconsistent: chunks={chunks}, \
                 stream_len=0x{stream_len:x}, bytes={bytes}"
            ),
            VectorGateError::ReferenceUnavailable { path } => {
                write!(f, "cannot read the archived vendor source: {path}")
            }
        }
    }
}

impl std::error::Error for VectorGateError {}

/// The vendor vector container, parsed and validated against its own
/// accounting (§40.1): SP magic, 9 chunks, declared stream length, and the
/// 3,744-byte array identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorContainer {
    /// SP header chunk count (9 — each chunk decodes to 1024 bytes).
    pub chunks: u16,
    /// SP header declared stream length (the bytes after the 8-byte header).
    pub stream_len: u32,
    /// The stream the engine consumes: everything after the SP header.
    pub stream: Vec<u8>,
}

/// Extract and validate the vector container from the archived vendor
/// source (`tools/reference/sphe-cmd_sp_png_dec.c`).
///
/// The parse mirrors the reference implementation
/// (`tools/fw_vector_probe.py::extract_vector`): the `ZlibBuf[16*234] = {`
/// marker, hex byte pairs to the first `};`. Validation is the container's
/// own accounting — magic, chunk count, declared stream length, array size
/// — so a corrupted or substituted reference fails here, before any gate
/// verdict could be poisoned.
pub fn extract_vector_container(c_source: &str) -> Result<VectorContainer, VectorGateError> {
    const MARKER: &str = "ZlibBuf[16*234] = {";
    let start = c_source
        .find(MARKER)
        .ok_or(VectorGateError::ContainerMarkerMissing)?
        + MARKER.len();
    let end = c_source[start..]
        .find("};")
        .ok_or(VectorGateError::ContainerTruncated)?
        + start;
    let body = &c_source.as_bytes()[start..end];
    let mut bytes = Vec::new();
    let mut i = 0;
    while i + 3 < body.len() {
        if body[i] == b'0' && body[i + 1] == b'x' {
            let hi = (body[i + 2] as char).to_digit(16);
            let lo = (body[i + 3] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                bytes.push((hi * 16 + lo) as u8);
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    if bytes.len() != VECTOR_CONTAINER_BYTES {
        return Err(VectorGateError::ContainerBytes {
            expected: VECTOR_CONTAINER_BYTES,
            found: bytes.len(),
        });
    }
    if bytes[0..2] != *b"SP" {
        return Err(VectorGateError::ContainerMagic {
            found: [bytes[0], bytes[1]],
        });
    }
    let chunks = u16::from_be_bytes([bytes[2], bytes[3]]);
    let stream_len = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let consistent = chunks == VECTOR_CHUNKS
        && chunks as usize * 1024 == VECTOR_ANCHOR_BYTES
        && stream_len as usize == bytes.len() - 8;
    if !consistent {
        return Err(VectorGateError::ContainerAccounting {
            chunks,
            stream_len,
            bytes: bytes.len(),
        });
    }
    Ok(VectorContainer {
        chunks,
        stream_len,
        stream: bytes[8..].to_vec(),
    })
}

/// Path of the archived vendor source, relative to the checkout root.
pub const REFERENCE_RELATIVE: &str = "tools/reference/sphe-cmd_sp_png_dec.c";

/// Load the vector container from the build checkout's archived source
/// (falling back to the working directory), validating it on the way.
pub fn reference_container() -> Result<VectorContainer, VectorGateError> {
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(REFERENCE_RELATIVE);
    let path = if manifest_path.exists() {
        manifest_path
    } else {
        std::path::PathBuf::from(REFERENCE_RELATIVE)
    };
    let source =
        std::fs::read_to_string(&path).map_err(|_| VectorGateError::ReferenceUnavailable {
            path: path.display().to_string(),
        })?;
    extract_vector_container(&source)
}

/// One candidate start bit's gate evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorGateReport {
    /// The symbol start bit this decode ran at.
    pub start_bit: usize,
    /// The engine-semantics decode ran to input end with zero invalid
    /// distances and inside the output cap.
    pub completed: bool,
    /// The decode error, if any (the ladder-discriminating failure modes
    /// are `BadDistance` and `OutputOverflow`; parse errors are
    /// stream-side and cannot be caused by the ladder).
    pub error: Option<InflateError>,
    /// Decoded output bytes (partial on error).
    pub decode_len: usize,
    /// `decode_len − 9,216`: the no-waste accounting (§40.3; the RFC
    /// ladder pins +570/+377/+358).
    pub anchor_delta: i64,
    /// Literal symbols decoded (stream-determined at a fixed frame:
    /// header + extra-bit widths, §19.3).
    pub lit_total: usize,
    /// Hex-character literals among them (the §40.2 backbone).
    pub hex_literals: usize,
    /// `hex_literals` per mille of `lit_total`.
    pub hex_permille: u64,
    /// Match symbols decoded.
    pub matches: usize,
    /// Literal bytes outside the documented alphabet (empty when clean).
    pub alphabet_violations: Vec<u8>,
}

impl VectorGateReport {
    /// Every literal is inside the documented alphabet
    /// (`[0-9a-f]`, `\n`, `\r`, NUL — §40.1).
    pub fn alphabet_clean(&self) -> bool {
        self.alphabet_violations.is_empty()
    }

    /// The pinned backbone holds: ≥[`BACKBONE_PERMILLE_MIN`]‰ hex
    /// (§40.2). Frame-pinned — moves only if the widths move.
    pub fn backbone_ok(&self) -> bool {
        self.lit_total > 0 && self.hex_permille >= BACKBONE_PERMILLE_MIN
    }

    /// The accounting hits the container's own 9-chunk anchor exactly
    /// (the no-waste reading, §40.3 — the condition the RFC ladder
    /// fails, and therefore the gate's discriminating edge).
    pub fn accounting_ok(&self) -> bool {
        self.completed && self.decode_len == VECTOR_ANCHOR_BYTES
    }

    /// The full §40.4 gate: completion, clean alphabet, pinned backbone,
    /// and the 9,216 accounting.
    pub fn pass(&self) -> bool {
        self.completed && self.alphabet_clean() && self.backbone_ok() && self.accounting_ok()
    }
}

impl fmt::Display for VectorGateReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "vector start={}: decode {} bytes (anchor {VECTOR_ANCHOR_BYTES}, delta {:+}), \
             literals {} ({}‰ hex), matches {}, alphabet {}, backbone {}, gate {}",
            self.start_bit,
            self.decode_len,
            self.anchor_delta,
            self.lit_total,
            self.hex_permille,
            self.matches,
            if self.alphabet_clean() {
                "clean"
            } else {
                "VIOLATED"
            },
            if self.backbone_ok() { "pinned" } else { "LOST" },
            if self.pass() { "PASS" } else { "FAIL" },
        )?;
        if let Some(e) = &self.error {
            write!(f, "; decode error: {e}")?;
        }
        if !self.alphabet_violations.is_empty() {
            write!(f, "; alphabet violations: {:?}", self.alphabet_violations)?;
        }
        Ok(())
    }
}

/// Evaluate the vector gate at one start bit under `tables`: the
/// engine-semantics decode (32 KiB zero preset, decode to input end) plus
/// the §40 censuses.
pub fn vector_gate_report(
    stream: &[u8],
    start_bit: usize,
    tables: &ValueTables,
) -> VectorGateReport {
    let mut out = Vec::new();
    let mut events = Vec::new();
    let result = inflate_engine_with_traced(
        stream,
        start_bit,
        ENGINE_PRESET_BYTES,
        &mut out,
        VECTOR_MAX_OUT,
        tables,
        Some(&mut events),
    );
    let mut lit_total = 0usize;
    let mut hex_literals = 0usize;
    let mut matches = 0usize;
    let mut violations = Vec::new();
    for ev in &events {
        match &ev.kind {
            DecodeEventKind::Literal { value } => {
                lit_total += 1;
                if value.is_ascii_digit() || (b'a'..=b'f').contains(value) {
                    hex_literals += 1;
                } else if !NON_HEX_ALPHABET.contains(value) {
                    violations.push(*value);
                }
            }
            DecodeEventKind::Match { .. } => matches += 1,
            DecodeEventKind::EndOfBlock => {}
        }
    }
    violations.sort_unstable();
    violations.dedup();
    let decode_len = out.len();
    VectorGateReport {
        start_bit,
        completed: result.is_ok(),
        error: result.err(),
        decode_len,
        anchor_delta: decode_len as i64 - VECTOR_ANCHOR_BYTES as i64,
        lit_total,
        hex_literals,
        hex_permille: (hex_literals as u64 * 1000) / std::cmp::max(lit_total, 1) as u64,
        matches,
        alphabet_violations: violations,
    }
}

/// The full gate: every candidate start bit, and whether ANY candidate
/// passes all four conditions. A vector whose true ladder exists must
/// decode cleanly and account exactly at the vendor's own start bit; the
/// report list carries the per-start-bit evidence either way.
pub fn vector_gate(stream: &[u8], tables: &ValueTables) -> (Vec<VectorGateReport>, bool) {
    let reports: Vec<VectorGateReport> = CANDIDATE_START_BITS
        .iter()
        .map(|&start| vector_gate_report(stream, start, tables))
        .collect();
    let pass = reports.iter().any(VectorGateReport::pass);
    (reports, pass)
}
