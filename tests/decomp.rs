//! Integration tests: the hardware decompression engine against the real
//! firmware's compressed payload.
//!
//! Evidence basis: docs/firmware-analysis.md §13 (register program), §13.3
//! (the driver's success path: the poll's bit-5 branch acks with 0x2000,
//! counts the event against the limit mirrored from the container header's
//! u16@2, and routes to the block that returns 0x13800 — non-zero = success;
//! the bit-6 branch is the failure epilogue). The engine decode semantics
//! modeled here: dynamic-block tables re-derived from the stream header
//! (bit-for-bit equal to the uploaded tables), symbols from the programmed
//! start bit (R3818 = 1088 container bits = 1024 stream bits for this
//! container; the 16-bit gap after the 1008-bit header is zero padding,
//! dump-verified) and decode to the end of the input length register, with
//! end-of-block symbols as markers (the driver expects the full declared
//! stream). The exact LZ value semantics of the hardware engine are still
//! being pinned down — see §13.4 for the open items.

use truman_emulator::firmware;
use truman_emulator::inflate::{
    inflate, inflate_engine, inflate_engine_with, ValueTables, ENGINE_VALUE_TABLES,
};

/// Locate the committed real firmware (if present).
fn real_firmware_path() -> Option<std::path::PathBuf> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let fw_dir = std::path::Path::new(manifest).join("firmware");
    std::fs::read_dir(fw_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.extension().map(|x| x == "bin").unwrap_or(false)
                && p.file_name()
                    .map(|n| n.to_string_lossy().contains("1506HV"))
                    .unwrap_or(false)
        })
}

/// The first deflate block of the real payload stream decodes to a stable
/// size under standard RFC 1951 semantics (zlib cross-checked during the
/// reverse-engineering session; pinned as a regression guard).
#[test]
fn real_sp_container_first_block_inflates_to_stable_size() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let image = firmware::load(&path).expect("load real firmware");

    // The loader copies 0x868C bytes from file offset 0x1002C into DRAM and
    // the engine inflates from image+8 (the firmware parses the 8-byte
    // "SP" header itself).
    let base = 0x1002Cusize;
    let container = &image.bytes[base..base + 0x868C];
    assert_eq!(&container[0..2], b"SP", "container magic");
    let be_len = u32::from_be_bytes([container[4], container[5], container[6], container[7]]);
    assert_eq!(be_len, 0x8684, "declared stream length");

    let stream = &container[8..];
    let mut out = Vec::new();
    let n = inflate(stream, 0x8000, &mut out, 0x8_0000).expect("inflate must succeed");
    assert_eq!(n, 28_256, "first deflate block output size");
}

/// The engine-programmed decode of the real container: symbols from the
/// programmed start bit (R3818 = 1088 container bits = 1024 stream bits)
/// through the end of the input length register (0x8692 bytes from the
/// source; the stream here is the container minus the 8-byte header, so
/// the emulator reads 0x868A stream bytes), end-of-block ignored. The
/// output size is pinned as a regression guard for the decode path the
/// machine service loop performs.
#[test]
fn real_sp_container_engine_decode_is_deterministic() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let image = firmware::load(&path).expect("load real firmware");
    let base = 0x1002Cusize;
    let container = &image.bytes[base..base + 0x8692];
    let stream = &container[8..];
    assert_eq!(&container[0..2], b"SP", "container magic");

    let mut out = Vec::new();
    let n = inflate_engine(stream, 1024, 0x8000, &mut out, 0x4_0000)
        .expect("engine decode must run to input end");
    // 75,398 bytes from the declared 0x8684-byte stream; the emulator feeds
    // the stream up to the engine's length register (0x8692 from the
    // source = 0x868A stream bytes), and the 6 zero tail bytes decode to
    // 259 more bytes before input exhaustion — pinned as observed.
    assert_eq!(n, 75_657, "engine decode output size (start bit 1024)");
}

/// The machine-level handshake: running the real firmware boots through
/// "LD eCos v1.16 1506f", the engine completes, the wait loop takes the
/// bit-5 event branch, and the loader's success path prints "Go eCos" and
/// "Boot from SPI" and jumps to the decompressed eCos entry at 0xA1600000.
#[test]
fn real_firmware_engine_handshake_boots_to_ecos_entry() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let image = firmware::load(&path).expect("load real firmware");
    let mut machine = truman_emulator::machine::Machine::new(
        truman_emulator::machine::MachineConfig::default(),
        &image,
    );
    truman_emulator::boot::reset_with_diagnostics(&mut machine, false);
    let result = machine.run(1_500_000);
    let console = machine.uart_console();
    assert!(console.contains("LD eCos v1.16 1506f"));
    assert!(
        console.contains("Go eCos") && console.contains("Boot from SPI"),
        "loader success path must run; console:\n{console}"
    );
    assert!(
        !console.contains("LD eCos fail"),
        "failure path must not be taken anymore"
    );
    // The final PC must be inside the decompressed eCos image page
    // (0xA1600000 kseg1 = phys 0x01600000).
    assert_eq!(
        result.pc & 0xFFF0_0000,
        0xA160_0000,
        "boot must reach the eCos entry page, got pc={:#x}",
        result.pc
    );
    // Engine state: one completed decode.
    let sh = machine.decomp.borrow();
    assert!(
        matches!(sh.last_result, Some(Ok(n)) if n > 70_000),
        "engine completed one full decode, got {:?}",
        sh.last_result
    );
    assert!(!sh.busy);
    let _ = result; // step count asserted in tests/boot.rs
}

/// LSB-first bit writer mirroring the decoder's `BitReader`: header
/// fields and extra bits go out LSB-first, Huffman codes MSB-of-code
/// first (RFC 1951 §3.1.1 packing rules).
struct BitWriter {
    bytes: Vec<u8>,
    n: usize,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            n: 0,
        }
    }

    fn put_bit(&mut self, b: u8) {
        let byte = self.n >> 3;
        if byte == self.bytes.len() {
            self.bytes.push(0);
        }
        self.bytes[byte] |= (b & 1) << (self.n & 7);
        self.n += 1;
    }

    /// Field bits, LSB-first (deflate header fields and extra bits).
    fn put(&mut self, value: u32, count: u8) {
        for i in 0..count {
            self.put_bit(((value >> i) & 1) as u8);
        }
    }

    /// Huffman code, MSB-of-code first (RFC 1951 §3.1.1).
    fn put_code(&mut self, code: u32, len: u8) {
        for i in (0..len).rev() {
            self.put_bit(((code >> i) & 1) as u8);
        }
    }
}

/// Canonical Huffman codes from per-symbol lengths (RFC 1951 §3.2.2):
/// codes assigned in symbol order within each length.
fn canonical(lengths: &[u8]) -> Vec<(u32, u8)> {
    let max_len = *lengths.iter().max().unwrap_or(&0) as usize;
    let mut bl_count = vec![0u32; max_len + 1];
    for &l in lengths {
        if l > 0 {
            bl_count[l as usize] += 1;
        }
    }
    let mut next_code = vec![0u32; max_len + 2];
    let mut code = 0u32;
    for bits in 1..=max_len {
        code = (code + bl_count[bits - 1]) << 1;
        next_code[bits] = code;
    }
    lengths
        .iter()
        .map(|&l| {
            if l == 0 {
                (0, 0)
            } else {
                let c = next_code[l as usize];
                next_code[l as usize] += 1;
                (c, l)
            }
        })
        .collect()
}

/// The value-table seam: the SAME bits decode differently under
/// different ladders, and the engine decode path consumes the injected
/// tables (not hard-coded RFC constants).
///
/// Hand-encodes a minimal dynamic block — ten literals "0123456789" then
/// one match (length code 257, distance code 0, no extra bits) — so the
/// bit layout carries no value information. Under the RFC ladder the
/// match is length 3 / distance 1; under the custom ladder
/// (lbase[0]=10, dbase[0]=5) it is length 10 / distance 5. This is
/// exactly the hardware situation of §30 (RFC frame, ROM ladder): once
/// the capture yields the real constants, `inflate_engine_with` with
/// those tables is the whole decode edit.
#[test]
fn engine_value_tables_are_parametric() {
    // Literal alphabet: 48..=52 length 3, 53..=57 length 4, symbol 257
    // length 4 (Kraft: 5/8 + 6/16 = 1); end-of-block (256) unused.
    // HLIT=258 transmits codes 0..=257 so the match code fits.
    let mut lit_lens = vec![0u8; 258];
    for (i, l) in lit_lens.iter_mut().enumerate() {
        *l = match i {
            48..=52 => 3,
            53..=57 => 4,
            257 => 4,
            _ => 0,
        };
    }
    let dist_lens = [1u8];
    let lit_codes = canonical(&lit_lens);
    let dist_codes = canonical(&dist_lens);

    // Code-length alphabet: symbols 1,3,4 (len 2), 0 (len 3), 17,18
    // (len 4); Kraft: 3/4 + 1/8 + 2/16 = 1. HCLEN must cover symbol 1
    // (CL_ORDER index 17), so 18 code-length lengths are transmitted.
    let cl_order = [
        16usize, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut cl_lens = [0u8; 19];
    cl_lens[1] = 2;
    cl_lens[3] = 2;
    cl_lens[4] = 2;
    cl_lens[0] = 3;
    cl_lens[17] = 4;
    cl_lens[18] = 4;
    let cl_codes = canonical(&cl_lens);
    let hclen_count = 18;

    let mut w = BitWriter::new();
    // Block header: BFINAL=1, BTYPE=2 (dynamic); HLIT field 1 (=258
    // codes), HDIST field 0 (=1 code), HCLEN field 14 (=18 lengths).
    w.put(1, 1);
    w.put(2, 2);
    w.put(1, 5);
    w.put(0, 5);
    w.put((hclen_count - 4) as u32, 4);
    for &sym in &cl_order[..hclen_count] {
        w.put(cl_lens[sym] as u32, 3);
    }
    // Code-length stream for the 258 transmitted lengths: 48 zeros
    // (codes 0..=47), 5x "length 3" (codes 48..=52), 5x "length 4"
    // (codes 53..=57), 199 zeros (codes 58..=256), "length 4"
    // (code 257), "length 1" (dist 0).
    let cl_put = |w: &mut BitWriter, sym: u8, run: u32, extra_bits: u8| {
        let (c, l) = cl_codes[sym as usize];
        w.put_code(c, l);
        if extra_bits > 0 {
            w.put(run, extra_bits);
        }
    };
    cl_put(&mut w, 18, 48 - 11, 7);
    for _ in 0..5 {
        cl_put(&mut w, 3, 0, 0);
    }
    for _ in 0..5 {
        cl_put(&mut w, 4, 0, 0);
    }
    cl_put(&mut w, 18, 138 - 11, 7); // zeros for codes 58..=195
    cl_put(&mut w, 18, 61 - 11, 7); // zeros for codes 196..=256
    cl_put(&mut w, 4, 0, 0); // code 257 length 4
    cl_put(&mut w, 1, 0, 0); // dist code 0 length 1

    let start_bit = w.n;
    // Symbol bits: ten literals, then the match (length code 257,
    // distance code 0).
    for b in b"0123456789" {
        let (c, l) = lit_codes[*b as usize];
        w.put_code(c, l);
    }
    let (mc, ml) = lit_codes[257];
    w.put_code(mc, ml);
    let (dc, dl) = dist_codes[0];
    w.put_code(dc, dl);
    // Tail padding: all-ones bits to the byte boundary. A partial tail
    // cannot complete a literal code (input exhaustion), and a 4-bit
    // '1111' reads code 257 whose distance bit is past input end — the
    // engine-mode loop breaks cleanly either way, output unaffected.
    while !w.n.is_multiple_of(8) {
        w.put_bit(1);
    }

    let data = w.bytes;
    // Custom ladder: length 10, distance 5, overlapping live-window copy.
    let mut custom = ValueTables::rfc1951();
    assert_eq!(
        custom.lextra[0], 0,
        "test assumes no extra bits for code 257"
    );
    assert_eq!(custom.dextra[0], 0, "test assumes no extra bits for dist 0");
    custom.lbase[0] = 10;
    custom.dbase[0] = 5;
    let mut out = Vec::new();
    let n_custom = inflate_engine_with(&data, start_bit, 0, &mut out, 64, &custom)
        .expect("custom-ladder decode");
    assert_eq!(
        &out[..n_custom],
        b"01234567895678956789",
        "custom ladder output (len 10, dist 5, overlap)"
    );

    // RFC ladder: the same bits give length 3 / distance 1.
    let mut out = Vec::new();
    let n_rfc = inflate_engine_with(&data, start_bit, 0, &mut out, 64, &ValueTables::rfc1951())
        .expect("rfc-ladder decode");
    assert_eq!(&out[..n_rfc], b"0123456789999", "rfc ladder output");

    // The machine's decode seam pins the current default model (RFC),
    // and the plain wrapper agrees with the explicit-rfc call.
    assert_eq!(
        ENGINE_VALUE_TABLES,
        ValueTables::rfc1951(),
        "ENGINE_VALUE_TABLES must pin the current default model"
    );
    let mut out = Vec::new();
    let n_default = inflate_engine(&data, start_bit, 0, &mut out, 64).expect("default decode");
    assert_eq!(
        n_default, n_rfc,
        "default path equals the rfc-ladder decode"
    );
}
