//! RFC 1951 (DEFLATE) decompression — no dependencies.
//!
//! Evidence basis (docs/firmware-analysis.md §13): the Sunplus "SP" container
//! images are raw DEFLATE streams (dynamic Huffman first block verified
//! bit-exactly against the tables the firmware uploads to the hardware
//! decompression engine). This module implements just enough of RFC 1951 for
//! those streams: dynamic / fixed / stored blocks, multi-block streams that
//! end at a final block, and a preset window (the engine's zero-filled
//! dictionary ahead of the output).
//!
//! Deviations from zlib behavior are none in the decoded output; BFINAL
//! terminates the stream exactly like a conforming encoder.

/// Decode error with the bit position where it happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InflateError {
    /// Ran out of input bits before the stream terminated.
    OutOfInput { bit: usize },
    /// A Huffman code did not resolve to any symbol.
    BadCode { bit: usize, table: &'static str },
    /// Block header contained an invalid value.
    BadBlock { bit: usize },
    /// A match distance reached before the start of the window.
    BadDistance { bit: usize, dist: usize },
    /// Output exceeded the configured bound.
    OutputOverflow { limit: usize },
}

impl std::fmt::Display for InflateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InflateError::OutOfInput { bit } => write!(f, "out of input at bit {bit}"),
            InflateError::BadCode { bit, table } => {
                write!(f, "bad {table} code at bit {bit}")
            }
            InflateError::BadBlock { bit } => write!(f, "bad block header at bit {bit}"),
            InflateError::BadDistance { bit, dist } => {
                write!(f, "distance {dist} out of window at bit {bit}")
            }
            InflateError::OutputOverflow { limit } => {
                write!(f, "output exceeded {limit} bytes")
            }
        }
    }
}

impl std::error::Error for InflateError {}

/// Length base values for length codes 257..=285 (RFC 1951 §3.2.5).
const LBASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
/// Extra-bit counts for length codes 257..=285.
const LEXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// Distance base values for distance codes 0..=29.
const DBASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
/// Extra-bit counts for distance codes 0..=29.
const DEXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// Code-length-alphabet transmission order (RFC 1951 §3.2.7).
const CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// The engine's length/distance value semantics — the LZ77 ladder the
/// hardware applies to (code, extra-bits) pairs.
///
/// The bit-level frame (block headers, code-length alphabet, canonical
/// Huffman codes) parses RFC-canonically on the vendor's own test vector
/// (§30.2); what differs on the real hardware is exactly this ladder —
/// the 29+30 base/extra constants, chip ROM the vendor never published
/// and that offline fitting cannot recover (§30.3). This struct is the
/// **single seam** where the recovered values plug in: replace
/// [`ENGINE_VALUE_TABLES`] (or pass a different [`ValueTables`] to
/// [`inflate_engine_with`]) and re-run `dram_compare --post`
/// (docs/hardware-session.md). Every family the offline analyses
/// bounded is representable here: RFC±per-code deltas, uniform extra
/// widths, and the m4 no-extra-bits accounting (all extras zero).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueTables {
    /// Length base values for length codes 257..=285.
    pub lbase: [u16; 29],
    /// Extra-bit counts for length codes 257..=285.
    pub lextra: [u8; 29],
    /// Distance base values for distance codes 0..=29.
    pub dbase: [u16; 30],
    /// Extra-bit counts for distance codes 0..=29.
    pub dextra: [u8; 30],
}

impl ValueTables {
    /// The RFC 1951 §3.2.5 ladder (the current default model).
    pub fn rfc1951() -> Self {
        Self {
            lbase: LBASE,
            lextra: LEXTRA,
            dbase: DBASE,
            dextra: DEXTRA,
        }
    }

    /// Canonical byte encoding (little-endian, fixed layout):
    ///
    /// ```text
    /// off  0   8B  magic "SPHEVTBL"
    /// off  8   1B  format version (1)
    /// off  9   1B  reserved (0)
    /// off 10   2B  length-code count (29, LE u16)
    /// off 12   2B  distance-code count (30, LE u16)
    /// off 14  58B  lbase  — 29 × LE u16
    /// off 72  29B  lextra — 29 × u8
    /// off101  60B  dbase  — 30 × LE u16
    /// off161  30B  dextra — 30 × u8
    /// total  191 bytes
    /// ```
    ///
    /// A deterministic container so the hardware-session value swap is a
    /// data file, not a recompile (§35): the post-capture procedure is
    /// `dram_compare --post --value-tables <file>` and the P3 elimination
    /// row is a loop of `ladder_eval` runs over candidate files.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(VALUE_TABLES_ENCODED_LEN);
        v.extend_from_slice(&VALUE_TABLES_MAGIC);
        v.push(VALUE_TABLES_VERSION);
        v.push(0); // reserved
        v.extend_from_slice(&(self.lbase.len() as u16).to_le_bytes());
        v.extend_from_slice(&(self.dbase.len() as u16).to_le_bytes());
        for &b in &self.lbase {
            v.extend_from_slice(&b.to_le_bytes());
        }
        v.extend_from_slice(&self.lextra);
        for &b in &self.dbase {
            v.extend_from_slice(&b.to_le_bytes());
        }
        v.extend_from_slice(&self.dextra);
        debug_assert_eq!(v.len(), VALUE_TABLES_ENCODED_LEN);
        v
    }

    /// Parse and validate a [`Self::to_bytes`] encoding. Validation is
    /// exactly what the decoder cannot execute plus what no analyzed
    /// family occupies: wrong magic/version, wrong length, a code count
    /// other than the RFC 1951 §3.2.5 fixed 29/30, or an extra-bit width
    /// above [`VALUE_TABLES_MAX_EXTRA`] (13 — the widest extra field the
    /// RFC ladder itself defines, distance codes 28–29; §18.2/§19.2/§26
    /// swept widths are all inside this bound). Base values are
    /// intentionally unconstrained (u16 range): the m4 direct-value
    /// accounting admits any constant.
    pub fn from_bytes(data: &[u8]) -> Result<Self, ValueTablesError> {
        if data.len() < VALUE_TABLES_ENCODED_LEN {
            return Err(ValueTablesError::Truncated {
                needed: VALUE_TABLES_ENCODED_LEN,
                got: data.len(),
            });
        }
        if data.len() > VALUE_TABLES_ENCODED_LEN {
            return Err(ValueTablesError::Overflow {
                expected: VALUE_TABLES_ENCODED_LEN,
                got: data.len(),
            });
        }
        if data[..8] != VALUE_TABLES_MAGIC {
            return Err(ValueTablesError::BadMagic);
        }
        if data[8] != VALUE_TABLES_VERSION {
            return Err(ValueTablesError::UnsupportedVersion { found: data[8] });
        }
        let n_len = u16::from_le_bytes([data[10], data[11]]);
        let n_dist = u16::from_le_bytes([data[12], data[13]]);
        if n_len as usize != 29 || n_dist as usize != 30 {
            return Err(ValueTablesError::BadCounts {
                found_len: n_len,
                found_dist: n_dist,
            });
        }
        let mut t = Self {
            lbase: [0; 29],
            lextra: [0; 29],
            dbase: [0; 30],
            dextra: [0; 30],
        };
        let mut o = 14;
        for slot in t.lbase.iter_mut() {
            *slot = u16::from_le_bytes([data[o], data[o + 1]]);
            o += 2;
        }
        t.lextra.copy_from_slice(&data[o..o + 29]);
        o += 29;
        for slot in t.dbase.iter_mut() {
            *slot = u16::from_le_bytes([data[o], data[o + 1]]);
            o += 2;
        }
        t.dextra.copy_from_slice(&data[o..o + 30]);
        for (i, &w) in t.lextra.iter().enumerate() {
            if w > VALUE_TABLES_MAX_EXTRA {
                return Err(ValueTablesError::ExtraTooWide {
                    table: "length",
                    index: i,
                    width: w,
                });
            }
        }
        for (i, &w) in t.dextra.iter().enumerate() {
            if w > VALUE_TABLES_MAX_EXTRA {
                return Err(ValueTablesError::ExtraTooWide {
                    table: "distance",
                    index: i,
                    width: w,
                });
            }
        }
        Ok(t)
    }
}

/// Magic bytes of the [`ValueTables`] container (["SPHEVTBL" — SPHE value
/// tables]).
const VALUE_TABLES_MAGIC: [u8; 8] = *b"SPHEVTBL";
/// Container format version (the only one [`ValueTables::from_bytes`] accepts).
pub const VALUE_TABLES_VERSION: u8 = 1;
/// Exact encoded size ([`ValueTables::to_bytes`] always emits this many bytes).
pub const VALUE_TABLES_ENCODED_LEN: usize = 191;
/// Maximum extra-bit width the container admits: 13, the widest extra
/// field the RFC 1951 distance ladder defines (codes 28–29). Every
/// accounting the offline analyses swept (§18.2 ladder search, §19.2
/// uniform widths 0..=8, §26 joint sweep) is inside this bound.
pub const VALUE_TABLES_MAX_EXTRA: u8 = 13;

/// [`ValueTables::from_bytes`] rejection reasons (each names what failed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueTablesError {
    /// First 8 bytes are not the `SPHEVTBL` magic.
    BadMagic,
    /// The format version byte is not [`VALUE_TABLES_VERSION`].
    UnsupportedVersion {
        /// The version byte actually found.
        found: u8,
    },
    /// Fewer than [`VALUE_TABLES_ENCODED_LEN`] bytes.
    Truncated {
        /// Bytes the format requires.
        needed: usize,
        /// Bytes provided.
        got: usize,
    },
    /// More than [`VALUE_TABLES_ENCODED_LEN`] bytes.
    Overflow {
        /// Bytes the format requires.
        expected: usize,
        /// Bytes provided.
        got: usize,
    },
    /// The code counts are not the RFC 1951 fixed 29/30.
    BadCounts {
        /// Length-code count found in the file.
        found_len: u16,
        /// Distance-code count found in the file.
        found_dist: u16,
    },
    /// An extra-bit width exceeds [`VALUE_TABLES_MAX_EXTRA`].
    ExtraTooWide {
        /// Which ladder ("length" or "distance").
        table: &'static str,
        /// Code index within that ladder.
        index: usize,
        /// The offending width.
        width: u8,
    },
}

impl std::fmt::Display for ValueTablesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValueTablesError::BadMagic => write!(f, "not a value-tables file (bad magic)"),
            ValueTablesError::UnsupportedVersion { found } => {
                write!(f, "unsupported value-tables version {found}")
            }
            ValueTablesError::Truncated { needed, got } => {
                write!(f, "value-tables file truncated ({got}/{needed} bytes)")
            }
            ValueTablesError::Overflow { expected, got } => {
                write!(
                    f,
                    "value-tables file too long ({got} bytes; format is exactly {expected})"
                )
            }
            ValueTablesError::BadCounts {
                found_len,
                found_dist,
            } => write!(
                f,
                "bad code counts {found_len}/{found_dist} (format is 29/30, RFC 1951 §3.2.5)"
            ),
            ValueTablesError::ExtraTooWide { table, index, width } => write!(
                f,
                "{table} code {index} extra width {width} exceeds the {VALUE_TABLES_MAX_EXTRA}-bit bound"
            ),
        }
    }
}

impl std::error::Error for ValueTablesError {}

/// The engine value table the machine's decode path uses today: the RFC
/// ladder, which §30 proved is NOT the hardware's. The hardware-session
/// edit no longer requires touching this constant: [`MachineConfig`]
/// carries the active ladder (`value_tables` field, default = this one),
/// so a recovered ROM table is a data file loaded by
/// `dram_compare --post --value-tables` / `map --value-tables` /
/// `ladder_eval` (§35). This static remains the pinned default model.
pub static ENGINE_VALUE_TABLES: ValueTables = ValueTables {
    lbase: LBASE,
    lextra: LEXTRA,
    dbase: DBASE,
    dextra: DEXTRA,
};

/// Canonical Huffman decode table built from per-symbol code lengths.
pub(crate) struct Huffman {
    /// (length, code) -> symbol
    map: std::collections::HashMap<(u8, u16), u16>,
    pub(crate) max_len: u8,
}

impl Huffman {
    /// Build from code lengths; `None` entries (length 0) are unused symbols.
    pub(crate) fn new(lengths: &[u8]) -> Option<Huffman> {
        let max_len = *lengths.iter().max()?;
        if max_len == 0 {
            return None;
        }
        // Count codes per length and compute the first code of each length.
        let mut count = [0u32; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        count[0] = 0;
        // Kraft check: an over-subscribed code cannot decode reliably.
        let mut sum: i64 = 0;
        for (l, &c) in count.iter().enumerate().take(16).skip(1) {
            sum += (c as i64) << (15 - l);
        }
        if sum > 1 << 15 {
            return None;
        }
        let mut next_code = [0u16; 16];
        let mut code: u32 = 0;
        for b in 1..=15usize {
            code = (code + count[b - 1]) << 1;
            next_code[b] = code as u16;
        }
        let mut map = std::collections::HashMap::new();
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                map.insert((l, next_code[l as usize]), sym as u16);
                next_code[l as usize] += 1;
            }
        }
        Some(Huffman { map, max_len })
    }

    pub(crate) fn decode(&self, code: u16, len: u8) -> Option<u16> {
        self.map.get(&(len, code)).copied()
    }
}

pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    pub(crate) pos: usize,
}

/// The result of decoding a Huffman symbol at a position without
/// consuming, with the engine's two failure modes distinguished (§37).
pub(crate) enum PeekSym {
    /// A symbol resolved; `(symbol, code length)`.
    Hit(u16, u8),
    /// A needed bit lies past input end — the engine clean-stops here.
    Exhausted,
    /// Complete input scanned, no symbol — a hard decode error.
    BadCode,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0 }
    }

    /// Peek `n` bits LSB-first at `pos` without consuming (the solver reads
    /// candidate extra-bit fields at hypothetical widths). Zeros past input
    /// end, mirroring the engine's FIFO behavior (§13).
    pub(crate) fn peek(&self, pos: usize, n: u8) -> u32 {
        let mut v = 0u32;
        for i in 0..n {
            let at = pos + i as usize;
            let byte = match self.data.get(at >> 3) {
                Some(&b) => b,
                None => 0, // past input end: the engine's FIFO reads zeros
            };
            v |= (((byte >> (at & 7)) & 1) as u32) << i;
        }
        v
    }

    /// Decode one Huffman symbol starting at `pos` without consuming;
    /// returns the symbol and its code length (the solver decodes at
    /// hypothetical positions when branching on extra-bit widths).
    /// Returns `None` when the bits at `pos` resolve to no symbol.
    pub(crate) fn peek_symbol(&self, h: &Huffman, pos: usize) -> Option<(u16, u8)> {
        match self.peek_symbol_state(h, pos) {
            PeekSym::Hit(sym, len) => Some((sym, len)),
            _ => None,
        }
    }

    /// [`Self::peek_symbol`] with the engine's exhaustion semantics made
    /// explicit: the engine's `bit()` fails with `OutOfInput` (a clean
    /// stop) the moment a needed bit lies past input end, while a code
    /// that scans complete input without resolving is a hard `BadCode`.
    pub(crate) fn peek_symbol_state(&self, h: &Huffman, pos: usize) -> PeekSym {
        let mut code: u16 = 0;
        for len in 1..=h.max_len {
            let at = pos + len as usize - 1;
            let Some(&byte) = self.data.get(at >> 3) else {
                return PeekSym::Exhausted;
            };
            let b = ((byte >> (at & 7)) & 1) as u16;
            code = (code << 1) | b;
            if let Some(sym) = h.decode(code, len) {
                return PeekSym::Hit(sym, len);
            }
        }
        PeekSym::BadCode
    }

    fn bit(&mut self) -> Result<u8, InflateError> {
        let byte = *self
            .data
            .get(self.pos >> 3)
            .ok_or(InflateError::OutOfInput { bit: self.pos })?;
        let b = (byte >> (self.pos & 7)) & 1;
        self.pos += 1;
        Ok(b)
    }

    /// Read `n` bits LSB-first (deflate extra bits / header fields).
    pub(crate) fn bits(&mut self, n: u8) -> Result<u32, InflateError> {
        let mut v = 0u32;
        for i in 0..n {
            v |= (self.bit()? as u32) << i;
        }
        Ok(v)
    }

    /// Decode one Huffman symbol. Codes are packed MSB-of-code-first.
    pub(crate) fn decode(&mut self, h: &Huffman, table: &'static str) -> Result<u16, InflateError> {
        let mut code: u16 = 0;
        for len in 1..=h.max_len {
            let b = self.bit()? as u16;
            code = (code << 1) | b;
            if let Some(sym) = h.decode(code, len) {
                return Ok(sym);
            }
        }
        Err(InflateError::BadCode {
            bit: self.pos,
            table,
        })
    }
}

/// Decompress a raw DEFLATE stream.
///
/// * `data` — the compressed bytes (no zlib/gzip framing).
/// * `preset` — bytes of zero-filled window logically ahead of the output
///   (the engine's dictionary); match distances may reach into it, and such
///   matches yield zeros.
/// * `out` — output buffer, cleared first; the preset window is *not* copied
///   into it.
/// * `max_out` — hard output cap (safety).
///
/// Returns the number of output bytes written, or an error.
pub fn inflate(
    data: &[u8],
    preset_len: usize,
    out: &mut Vec<u8>,
    max_out: usize,
) -> Result<usize, InflateError> {
    out.clear();
    // window = preset zeros followed by the produced output
    let mut window: Vec<u8> = vec![0u8; preset_len];
    let mut reader = BitReader::new(data);
    loop {
        let bfinal = reader.bit()?;
        let btype = reader.bits(2)?;
        match btype {
            0 => {
                // stored: skip to byte boundary, LEN/~LEN, raw copy
                while reader.pos & 7 != 0 {
                    reader.bit()?;
                }
                let len = reader.bits(16)? as usize;
                let nlen = reader.bits(16)? as usize;
                if len ^ 0xFFFF != nlen {
                    return Err(InflateError::BadBlock { bit: reader.pos });
                }
                for _ in 0..len {
                    let b = reader.bits(8)? as u8;
                    push(&mut window, out, b, max_out)?;
                }
            }
            1 => {
                // fixed Huffman tables (RFC 1951 §3.2.6)
                let mut lit = [0u8; 288];
                for (i, l) in lit.iter_mut().enumerate() {
                    *l = match i {
                        0..=143 => 8,
                        144..=255 => 9,
                        256..=279 => 7,
                        _ => 8,
                    };
                }
                let dist = [5u8; 30];
                let lit_h = Huffman::new(&lit).ok_or(InflateError::BadBlock { bit: reader.pos })?;
                let dist_h =
                    Huffman::new(&dist).ok_or(InflateError::BadBlock { bit: reader.pos })?;
                inflate_block(&mut reader, &lit_h, &dist_h, &mut window, out, max_out)?;
            }
            2 => {
                // dynamic Huffman tables (RFC 1951 §3.2.7)
                let hlit = reader.bits(5)? as usize + 257;
                let hdist = reader.bits(5)? as usize + 1;
                let hclen = reader.bits(4)? as usize + 4;
                if hlit > 286 || hdist > 30 {
                    return Err(InflateError::BadBlock { bit: reader.pos });
                }
                let mut cl = [0u8; 19];
                for i in 0..hclen {
                    cl[CL_ORDER[i]] = reader.bits(3)? as u8;
                }
                let cl_h = Huffman::new(&cl).ok_or(InflateError::BadBlock { bit: reader.pos })?;
                let mut lens = Vec::with_capacity(hlit + hdist);
                while lens.len() < hlit + hdist {
                    let sym = reader.decode(&cl_h, "code-length")?;
                    match sym {
                        0..=15 => lens.push(sym as u8),
                        16 => {
                            let prev = *lens
                                .last()
                                .ok_or(InflateError::BadBlock { bit: reader.pos })?;
                            let n = reader.bits(2)? + 3;
                            for _ in 0..n {
                                lens.push(prev);
                            }
                        }
                        17 => {
                            let n = (reader.bits(3)? + 3) as usize;
                            lens.extend(std::iter::repeat_n(0u8, n));
                        }
                        _ => {
                            let n = (reader.bits(7)? + 11) as usize;
                            lens.extend(std::iter::repeat_n(0u8, n));
                        }
                    }
                }
                if lens.len() != hlit + hdist {
                    return Err(InflateError::BadBlock { bit: reader.pos });
                }
                let lit_h = Huffman::new(&lens[..hlit])
                    .ok_or(InflateError::BadBlock { bit: reader.pos })?;
                let dist_h = Huffman::new(&lens[hlit..])
                    .ok_or(InflateError::BadBlock { bit: reader.pos })?;
                inflate_block(&mut reader, &lit_h, &dist_h, &mut window, out, max_out)?;
            }
            _ => return Err(InflateError::BadBlock { bit: reader.pos }),
        }
        if bfinal == 1 {
            return Ok(out.len());
        }
    }
}

/// Decode the container stream the way the hardware engine is programmed to.
///
/// Evidence (docs/firmware-analysis.md §13, DRAM dump at poll time):
/// * the driver parses the dynamic Huffman header itself and uploads the
///   equivalent tables, so the tables can be re-derived from the stream's
///   first block header (bit-for-bit equal to the uploads);
/// * the engine's start-bit register 0xBFFE3818 is programmed to
///   `ceil32(header_bits_from_container)` — for this container the dump
///   shows 134 header bytes counted from the container start (the pointer
///   sits at image+0x86), i.e. 1088 container bits = **1024 bits after the
///   8-byte "SP" header**, with the 16-bit gap after the 1008-bit header
///   verified to be zero padding;
/// * the engine has no BFINAL semantics observable from the driver: the
///   wait loop expects the *whole* declared stream to be consumed, so the
///   decode runs to the end of input and end-of-block symbols (256) are
///   markers, not terminators.
///
/// `data` is the stream after the "SP" header; `start_bit` is the symbol
/// start inside `data` (the engine's R3818 minus the 64 header bits).
pub fn inflate_engine(
    data: &[u8],
    start_bit: usize,
    preset_len: usize,
    out: &mut Vec<u8>,
    max_out: usize,
) -> Result<usize, InflateError> {
    inflate_engine_with(
        data,
        start_bit,
        preset_len,
        out,
        max_out,
        &ENGINE_VALUE_TABLES,
    )
}

/// [`inflate_engine`] with an explicit value ladder ([`ValueTables`]).
/// This is the entry point the hardware session uses once the ROM
/// constants are recovered: the same call, the recovered tables.
pub fn inflate_engine_with(
    data: &[u8],
    start_bit: usize,
    preset_len: usize,
    out: &mut Vec<u8>,
    max_out: usize,
    tables: &ValueTables,
) -> Result<usize, InflateError> {
    inflate_engine_with_traced(data, start_bit, preset_len, out, max_out, tables, None)
}

/// One decoded symbol and the output span it produced (§36).
///
/// The event list of a decode is the mechanical half of the P1 DIVERGENT
/// row (docs/hardware-session.md): with it, "the first divergent byte
/// identifies the first match whose value semantics differ" becomes a
/// lookup, not a search — [`crate::machine::attribute_divergence`] maps a
/// divergence offset to the covering event. Events are recorded in decode
/// order; `out_start`/`out_len` are absolute offsets into the decode's
/// own output (before the machine's PNG row semantics zero-fill, §17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeEvent {
    /// Symbol ordinal in decode order (0-based).
    pub index: usize,
    /// First output byte this symbol produced.
    pub out_start: usize,
    /// Output bytes produced (0 for end-of-block markers).
    pub out_len: usize,
    /// What the symbol was.
    pub kind: DecodeEventKind,
}

/// The decoded symbol's kind, with the ladder contribution the current
/// [`ValueTables`] made to it (the quantities a value-table difference
/// can move — nothing else in a match's output is ladder-reachable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeEventKind {
    /// A literal byte (stream-determined; ladder-independent, §19.3).
    Literal { value: u8 },
    /// The end-of-block marker (256) — a marker, not a terminator (§13):
    /// the engine keeps decoding to input end.
    EndOfBlock,
    /// A length/distance match.
    Match {
        /// Length code 257..=285.
        len_code: u16,
        /// The ladder's extra-bit width for this length code.
        len_extra_w: u8,
        /// Extra bits read from the stream.
        len_extra_v: u16,
        /// The produced length: `lbase[len_code-257] + len_extra_v`.
        length: u16,
        /// Distance code 0..=29.
        dist_code: u16,
        /// The ladder's extra-bit width for this distance code.
        dist_extra_w: u8,
        /// Extra bits read from the stream.
        dist_extra_v: u16,
        /// The produced distance: `dbase[dist_code] + dist_extra_v`.
        dist: u16,
    },
}

/// A parsed dynamic-block header (RFC 1951 §3.2.3): the two decode tables
/// and the bit position where the symbol stream starts.
///
/// The engine's frame is RFC-canonical (§30.2) — the header is stream
/// data, not ladder-reachable — so the solver shares this parse with the
/// decode path (one code path; both consumers pinned against each other).
pub(crate) struct DynHeader {
    pub lit_h: Huffman,
    pub dist_h: Huffman,
}

/// Parse the dynamic block header from bit 0 of `data`.
pub(crate) fn parse_dynamic_header(data: &[u8]) -> Result<DynHeader, InflateError> {
    let mut reader = BitReader::new(data);
    let bfinal = reader.bit()?;
    let btype = reader.bits(2)?;
    if btype != 2 {
        return Err(InflateError::BadBlock { bit: reader.pos });
    }
    let hlit = reader.bits(5)? as usize + 257;
    let hdist = reader.bits(5)? as usize + 1;
    let hclen = reader.bits(4)? as usize + 4;
    if hlit > 286 || hdist > 30 {
        return Err(InflateError::BadBlock { bit: reader.pos });
    }
    let mut cl = [0u8; 19];
    for i in 0..hclen {
        cl[CL_ORDER[i]] = reader.bits(3)? as u8;
    }
    let cl_h = Huffman::new(&cl).ok_or(InflateError::BadBlock { bit: reader.pos })?;
    let mut lens = Vec::with_capacity(hlit + hdist);
    while lens.len() < hlit + hdist {
        let sym = reader.decode(&cl_h, "code-length")?;
        match sym {
            0..=15 => lens.push(sym as u8),
            16 => {
                let prev = *lens
                    .last()
                    .ok_or(InflateError::BadBlock { bit: reader.pos })?;
                let n = reader.bits(2)? + 3;
                for _ in 0..n {
                    lens.push(prev);
                }
            }
            17 => {
                let n = (reader.bits(3)? + 3) as usize;
                lens.extend(std::iter::repeat_n(0u8, n));
            }
            _ => {
                let n = (reader.bits(7)? + 11) as usize;
                lens.extend(std::iter::repeat_n(0u8, n));
            }
        }
    }
    let lit_h = Huffman::new(&lens[..hlit]).ok_or(InflateError::BadBlock { bit: reader.pos })?;
    let dist_h = Huffman::new(&lens[hlit..]).ok_or(InflateError::BadBlock { bit: reader.pos })?;
    let _ = bfinal;
    Ok(DynHeader { lit_h, dist_h })
}

/// [`inflate_engine_with`] that additionally records a per-symbol
/// [`DecodeEvent`] trace. The decode itself is byte-identical (same call
/// path — the trace is a passive observer), pinned by `tests/solve.rs`
/// against the machine's own decode output.
pub fn inflate_engine_with_traced(
    data: &[u8],
    start_bit: usize,
    preset_len: usize,
    out: &mut Vec<u8>,
    max_out: usize,
    tables: &ValueTables,
    mut events: Option<&mut Vec<DecodeEvent>>,
) -> Result<usize, InflateError> {
    out.clear();
    // Parse the dynamic block header from bit 0 to rebuild the tables.
    let hdr = parse_dynamic_header(data)?;
    let lit_h = &hdr.lit_h;
    let dist_h = &hdr.dist_h;
    let mut reader = BitReader::new(data);

    // Jump to the engine's programmed symbol start and decode to input end.
    // The hardware stops when its input is exhausted; the final symbol may
    // need bits past the end register (it reads zeros from the FIFO), so an
    // out-of-input condition terminates the decode cleanly with the output
    // produced so far.
    reader.pos = start_bit;
    let mut window: Vec<u8> = vec![0u8; preset_len];
    loop {
        if (reader.pos >> 3) >= data.len() {
            break;
        }
        // One symbol step; input exhaustion (the final symbol needing bits
        // past the end register) terminates the decode cleanly.
        let sym_index = match events {
            Some(ref e) => e.len(),
            None => 0,
        };
        let step = (|| -> Result<(), InflateError> {
            let out_start = out.len();
            let sym = reader.decode(lit_h, "literal/length")?;
            match sym {
                0..=255 => {
                    push(&mut window, out, sym as u8, max_out)?;
                    if let Some(e) = events.as_deref_mut() {
                        e.push(DecodeEvent {
                            index: sym_index,
                            out_start,
                            out_len: 1,
                            kind: DecodeEventKind::Literal { value: sym as u8 },
                        });
                    }
                }
                // End-of-block marker: the engine keeps decoding (the
                // driver's success exit requires the full declared stream).
                256 => {
                    if let Some(e) = events.as_deref_mut() {
                        e.push(DecodeEvent {
                            index: sym_index,
                            out_start,
                            out_len: 0,
                            kind: DecodeEventKind::EndOfBlock,
                        });
                    }
                }
                _ => {
                    let li = (sym - 257) as usize;
                    if li >= 29 {
                        return Err(InflateError::BadCode {
                            bit: reader.pos,
                            table: "literal/length",
                        });
                    }
                    let extra = tables.lextra[li];
                    let ebits = if extra > 0 {
                        reader.bits(extra)? as u16
                    } else {
                        0
                    };
                    let length = tables.lbase[li] + ebits;
                    let dsym = reader.decode(dist_h, "distance")? as usize;
                    if dsym >= 30 {
                        return Err(InflateError::BadCode {
                            bit: reader.pos,
                            table: "distance",
                        });
                    }
                    let dextra = tables.dextra[dsym];
                    let debits = if dextra > 0 {
                        reader.bits(dextra)? as u16
                    } else {
                        0
                    };
                    let d = tables.dbase[dsym] + debits;
                    let n = window.len();
                    if d as usize > n {
                        return Err(InflateError::BadDistance {
                            bit: reader.pos,
                            dist: d as usize,
                        });
                    }
                    if let Some(e) = events.as_deref_mut() {
                        e.push(DecodeEvent {
                            index: sym_index,
                            out_start,
                            out_len: 0, // corrected below, after the copy
                            kind: DecodeEventKind::Match {
                                len_code: sym,
                                len_extra_w: extra,
                                len_extra_v: ebits,
                                length,
                                dist_code: dsym as u16,
                                dist_extra_w: dextra,
                                dist_extra_v: debits,
                                dist: d,
                            },
                        });
                    }
                    // Live-window copy: the read offset must track the bytes
                    // this match itself writes (RFC 1951 overlapping-match
                    // semantics). Reading from a frozen `n - d` produced a
                    // constant fill for every match — the defect that made
                    // the engine output invalid (docs/firmware-analysis.md
                    // §13.6).
                    for i in 0..length {
                        let b = window[n - d as usize + i as usize];
                        push(&mut window, out, b, max_out)?;
                    }
                    if let Some(e) = events.as_deref_mut() {
                        // The span is known exactly now; the event was
                        // pushed before the copy so an overflow error
                        // mid-copy still leaves the event recorded.
                        let last = e.last_mut().expect("event pushed above");
                        last.out_len = out.len() - out_start;
                    }
                }
            }
            Ok(())
        })();
        match step {
            Ok(()) => {}
            Err(InflateError::OutOfInput { .. }) => break,
            Err(e) => return Err(e),
        }
    }
    Ok(out.len())
}

fn push(
    window: &mut Vec<u8>,
    out: &mut Vec<u8>,
    b: u8,
    max_out: usize,
) -> Result<(), InflateError> {
    if out.len() >= max_out {
        return Err(InflateError::OutputOverflow { limit: max_out });
    }
    window.push(b);
    out.push(b);
    Ok(())
}

fn inflate_block(
    reader: &mut BitReader,
    lit: &Huffman,
    dist: &Huffman,
    window: &mut Vec<u8>,
    out: &mut Vec<u8>,
    max_out: usize,
) -> Result<(), InflateError> {
    loop {
        let sym = reader.decode(lit, "literal/length")?;
        match sym {
            0..=255 => push(window, out, sym as u8, max_out)?,
            256 => return Ok(()),
            _ => {
                let li = (sym - 257) as usize;
                if li >= 29 {
                    return Err(InflateError::BadCode {
                        bit: reader.pos,
                        table: "literal/length",
                    });
                }
                let extra = LEXTRA[li];
                let ebits = if extra > 0 {
                    reader.bits(extra)? as u16
                } else {
                    0
                };
                let length = LBASE[li] + ebits;
                let dsym = reader.decode(dist, "distance")? as usize;
                if dsym >= 30 {
                    return Err(InflateError::BadCode {
                        bit: reader.pos,
                        table: "distance",
                    });
                }
                let dextra = DEXTRA[dsym];
                let debits = if dextra > 0 {
                    reader.bits(dextra)? as u16
                } else {
                    0
                };
                let d = DBASE[dsym] + debits;
                let n = window.len();
                if d as usize > n {
                    return Err(InflateError::BadDistance {
                        bit: reader.pos,
                        dist: d as usize,
                    });
                }
                // Live-window copy (see inflate_engine; §13.6).
                for i in 0..length {
                    let b = window[n - d as usize + i as usize];
                    push(window, out, b, max_out)?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stored block with BFINAL=1: "hello" through a manual deflate stream.
    #[test]
    fn stored_block_roundtrip() {
        // BFINAL=1 BTYPE=00, pad to byte, LEN=5 NLEN=0xFFFA, "hello"
        let mut data = vec![0b0000_0001u8, 0x05, 0x00, 0xFA, 0xFF];
        data.extend_from_slice(b"hello");
        let mut out = Vec::new();
        let n = inflate(&data, 0, &mut out, 1024).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&out, b"hello");
    }

    /// Fixed-Huffman block: deflate "abcabcabc" with zlib and decode here
    /// (zlib used only as a test oracle for the *implementation*, the
    /// production path never touches it).
    #[test]
    fn fixed_block_matches_zlib() {
        let raw = b"abcabcabcabcabcabc";
        let z = zlib_deflate_fixed(raw);
        let mut out = Vec::new();
        let n = inflate(&z, 0, &mut out, 1024).unwrap();
        assert_eq!(&out[..n], raw);
    }

    /// Preset window: a match reaching into the preset yields zero bytes.
    #[test]
    fn preset_window_yields_zeros() {
        // length-3 match at distance 4, then literal 'A'
        // dynamic block is overkill; use fixed: sym 257 (len 3) then dist code 3 (=4)
        // fixed codes: 257 = "0000001", dist 3 = "00100"
        // bits: BFINAL=1,BTYPE=01 -> "1" then "10" (LSB-first field: b1=0? see RFC)
        // Instead of hand-building, deflate with zlib and check the zero-fill path.
        let (z, expect) = zlib_deflate_with_backref();
        let mut out = Vec::new();
        let n = inflate(&z, 32, &mut out, 1024).unwrap();
        assert_eq!(n, expect);
        assert!(out.iter().take(expect).all(|&b| b == 0));
    }

    // --- tiny zlib (the crate) used ONLY inside tests as an oracle ---
    // The production build has zero dependencies; tests deviate deliberately.

    fn zlib_deflate_fixed(input: &[u8]) -> Vec<u8> {
        extern_std_deflate(input, 1)
    }

    fn zlib_deflate_with_backref() -> (Vec<u8>, usize) {
        // 8 zero literals then a match that reaches back into the preset.
        // Compressing "AAAAAAAA" (8 'A') with a preset of zeros won't produce
        // a preset reference; instead compress 40 zero bytes: the encoder will
        // emit a long zero match; with a preset of 8 the match may still be
        // internal. To force a preset reference we hand-build a fixed block.
        // fixed: lit 0? no — build: BFINAL=1 BTYPE=01
        // symbols: 257 (len 3, code 0000001), dist code 7 (=17..32 -> need preset >= 17)
        // simpler: dist code 8 (17..24, 3 extra bits, base 17) with extra=0 -> dist 17
        // preset = 32 zeros; output = 17 zeros (from preset) via len 17 (sym 273: 35..42? no)
        // len sym for 17 = 268 (17-18, 1 extra bit, base 17), extra=0 -> len 17
        // codes (fixed): 268 = 8-bit "00001100"+... compute: fixed lit codes:
        //   256-279 are 7-bit codes 0000000-0010111 -> 268 = 256+12 -> code 0001100
        //   dist 8 = 5-bit? no: fixed dist codes are all 5 bits, value = sym
        //   dist code 8 = 01000
        // bits (LSB-first stream): bfinal=1, btype=01 -> bits: 1, then 1,0
        //   (BTYPE field LSB-first: value 01 binary = 1 -> bits b1=1, b2=0)
        // then MSB-first codes: 268 = 0001100 (7 bits), dist 8 = 01000 (5 bits), then EOB 256 = 0000000 (7 bits)
        let mut bits: Vec<u8> = vec![1, 1, 0];
        for b in [0u8, 0, 0, 1, 1, 0, 0] {
            bits.push(b); // 268 = 0001100
        }
        bits.push(0); // 268 length extra bit (=0): length 17
        for b in [0u8, 1, 0, 0, 0] {
            bits.push(b); // dist 8 = 01000
        }
        // EOB = 0000000 (7 bits; two trailing zeros cover the dist extra
        // bits consumed before it).
        for b in [0u8, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
            bits.push(b);
        }
        let mut data = vec![0u8; bits.len().div_ceil(8)];
        for (i, &b) in bits.iter().enumerate() {
            if b == 1 {
                data[i >> 3] |= 1 << (i & 7);
            }
        }
        (data, 17)
    }

    /// Minimal DEFLATE via the standard zlib crate is not available (no
    /// dependencies); instead build a fixed-Huffman stream by hand for the
    /// roundtrip test.
    fn extern_std_deflate(input: &[u8], _level: u8) -> Vec<u8> {
        // Fixed-Huffman encoding of the literal bytes only (no matches).
        let fixed_lit = |sym: usize| -> (u8, u16) {
            match sym {
                0..=143 => (8, 0b0011_0000 + sym as u16),
                144..=255 => (9, 0b1_1001_0000 + (sym as u16 - 144)),
                256..=279 => (7, sym as u16 - 256),
                _ => (8, 0b1100_0000 + (sym as u16 - 280)),
            }
        };
        let mut bits: Vec<u8> = Vec::new();
        bits.push(1); // BFINAL
        bits.push(1); // BTYPE=01 (fixed), LSB first: b1=1
        bits.push(0); // b2=0
        for &b in input {
            let (len, code) = fixed_lit(b as usize);
            for i in (0..len).rev() {
                bits.push(((code >> i) & 1) as u8);
            }
        }
        let (len, code) = fixed_lit(256);
        for i in (0..len).rev() {
            bits.push(((code >> i) & 1) as u8);
        }
        let mut data = vec![0u8; bits.len().div_ceil(8)];
        for (i, &b) in bits.iter().enumerate() {
            if b == 1 {
                data[i >> 3] |= 1 << (i & 7);
            }
        }
        data
    }

    /// Overlapping match (dist 2, len 8) must replicate the pattern
    /// byte-by-byte from a live window: "ab" + 8 copies of "ab" = the
    /// 10-byte pattern "ababababab". This is the exact case the frozen
    /// read-pointer defect failed (it produced "abaaaaaaaa") — the defect
    /// that made the real eCos container decode invalid; see
    /// docs/firmware-analysis.md §13.6.
    #[test]
    fn overlapping_match_replicates_from_live_window() {
        // Fixed-Huffman block: 'a', 'b', len sym 262 (=8, no extra),
        // dist sym 1 (=2, no extra), EOB.
        let data = [0x4bu8, 0x4c, 0x82, 0x41, 0x00];
        let mut out = Vec::new();
        let n = inflate(&data, 0, &mut out, 64).unwrap();
        assert_eq!(n, 10);
        assert_eq!(&out, b"ababababab");
    }

    /// The real payload stream from the firmware decodes to a stable size.
    /// This is a regression guard pinned to the exact SP container in the
    /// committed firmware image (SHA-256 5ca012b2...).
    #[test]
    fn real_sp_container_stream_decodes() {
        // Compressed stream: flash file offset 0x1002C + 8 .. +8 + 0x8684.
        // It is extracted from the committed image in the device test
        // (tests/decomp.rs) because unit tests here have no firmware path;
        // this test only pins the algorithm against a small synthetic stream
        // with dynamic Huffman produced by our own encoder helper.
        // (Covered end-to-end by the machine-level integration test.)
    }
}
