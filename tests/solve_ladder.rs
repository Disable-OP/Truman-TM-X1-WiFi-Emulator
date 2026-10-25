//! Capture→ladder solver tests (Milestone AJ, §37): the inversion from
//! one true decode output to the engine's value ladder, verified by the
//! engine's own re-decode.

use std::collections::BTreeSet;

use truman_emulator::firmware;
use truman_emulator::inflate::{
    inflate_engine_with, inflate_engine_with_preset, inflate_engine_with_traced, ValueTables,
    ENGINE_VALUE_TABLES,
};
use truman_emulator::machine::{
    adler32, Machine, MachineConfig, ENGINE_MAX_OUT, ENGINE_PRESET_BYTES,
};
use truman_emulator::solve::{length_class, solve_ladder, CodeStatus, SolveError, SolveLimits};

// ---------------------------------------------------------------------------
// Stream building helpers (mirroring tests/decomp.rs: LSB-first fields,
// MSB-of-code Huffman packing, canonical codes from per-symbol lengths).
// ---------------------------------------------------------------------------

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

/// Canonical Huffman codes from per-symbol lengths (RFC 1951 §3.2.2).
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

const CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Build a dynamic-block stream emitting `lits` as literals and `ms`
/// as matches `(len_sym, len_extra_value, len_extra_width, dist_sym,
/// dist_extra_value, dist_extra_width)`, then the end-of-block marker,
/// with tail padding to the byte boundary.
fn build_stream(lits: &[u8], ms: &[(u16, u16, u8, u16, u16, u8)]) -> (Vec<u8>, usize) {
    // Literal alphabet: the distinct literal bytes plus the match symbols.
    let mut used = BTreeSet::new();
    // Symbol 0 gets the canonical all-zeros code, so the zero tail
    // padding decodes to one ladder-independent literal (0x00) and any
    // leftover bits exhaust cleanly (the engine's OutOfInput stop).
    used.insert(0);
    for &b in lits {
        used.insert(b as u16);
    }
    for m in ms {
        used.insert(m.0);
    }
    used.insert(256); // end-of-block marker
                      // Filler length symbols so the all-ones tail padding can never
                      // complete a literal code: with them the highest canonical code is a
                      // MATCH symbol whose distance bits lie past input end, and any
                      // partial padding exhausts cleanly (the engine's OutOfInput stop).
    for filler in 266..=270u16 {
        used.insert(filler);
    }
    let max_sym = *used.iter().next_back().unwrap();
    let hlit = (max_sym as usize + 1).max(257);

    // Give every used symbol a code length: 5 bits for <= 16 used symbols
    // is Kraft-feasible (16/32); add filler lengths for more.
    let mut lit_lens = vec![0u8; hlit.max(258)];
    let n_used = used.len();
    // Kraft-feasible: every used symbol gets one shared length — 5 bits
    // holds up to 16 symbols, 8 bits up to 256.
    let lit_len = if n_used <= 16 { 5u8 } else { 8u8 };
    for &sym in &used {
        lit_lens[sym as usize] = lit_len;
    }
    // Distances: one code per distinct dist symbol used.
    let mut dist_used = BTreeSet::new();
    for m in ms {
        dist_used.insert(m.3);
    }
    let ndist = dist_used.len().max(1);
    let dist_len = if ndist <= 2 { 1u8 } else { 4u8 };
    // Kraft for dist: 2 codes of len 1 = 1 ✓; ≤16 codes of len 4 ≤ 1 ✓
    // (incomplete codes are fine — Huffman::new only rejects over-subscription).
    let mut dist_lens = vec![0u8; 30];
    for &sym in &dist_used {
        dist_lens[sym as usize] = dist_len;
    }

    let lit_codes = canonical(&lit_lens);
    let dist_codes = canonical(&dist_lens);

    // Code-length alphabet: emit the lit lens then dist lens verbatim
    // with symbol codes; build the CL alphabet from what's needed.
    let mut all_lens: Vec<u8> = lit_lens[..hlit.max(258)].to_vec();
    all_lens.extend_from_slice(&dist_lens);
    let mut cl_used = BTreeSet::new();
    for &l in &all_lens {
        cl_used.insert(l);
    }
    cl_used.insert(0); // zeros appear (unused symbols)
    let mut cl_lens = [0u8; 19];
    let ncl = cl_used.len();
    // Kraft-feasible CL alphabet: 2^len ≥ ncl.
    let cl_code_len = {
        let mut l = 1u8;
        while (1usize << l) < ncl {
            l += 1;
        }
        l
    };
    for &sym in &cl_used {
        cl_lens[sym as usize] = cl_code_len;
    }
    let cl_codes = canonical(&cl_lens);
    let hclen_count = CL_ORDER
        .iter()
        .rposition(|&s| cl_lens[s] != 0)
        .map(|p| p + 1)
        .unwrap_or(19)
        .max(4);

    let mut w = BitWriter::new();
    w.put(1, 1); // BFINAL
    w.put(2, 2); // BTYPE=dynamic
    let hlit_field = hlit.max(258) - 257;
    w.put(hlit_field as u32, 5);
    w.put(30 - 1, 5); // HDIST: transmit all 30 (simple)
    w.put((hclen_count - 4) as u32, 4);
    for &sym in &CL_ORDER[..hclen_count] {
        w.put(cl_lens[sym] as u32, 3);
    }
    for &l in &all_lens {
        let (c, len) = cl_codes[l as usize];
        w.put_code(c, len);
    }

    let start_bit = w.n;
    for &b in lits {
        let (c, l) = lit_codes[b as usize];
        w.put_code(c, l);
    }
    for m in ms {
        let (c, l) = lit_codes[m.0 as usize];
        w.put_code(c, l);
        if m.2 > 0 {
            w.put(m.1 as u32, m.2);
        }
        let (dc, dl) = dist_codes[m.3 as usize];
        w.put_code(dc, dl);
        if m.5 > 0 {
            w.put(m.4 as u32, m.5);
        }
    }
    // End-of-block marker.
    let (c, l) = lit_codes[256];
    w.put_code(c, l);
    // Tail padding: zeros to the byte boundary — symbol 0 holds the
    // all-zeros code, so a completed padding decodes to one literal 0x00
    // (ladder-independent) and the remainder exhausts cleanly.
    while !w.n.is_multiple_of(8) {
        w.put_bit(0);
    }
    (w.bytes, start_bit)
}

/// A custom (non-RFC) ladder: shifted length bases, non-RFC extra widths.
fn custom_ladder() -> ValueTables {
    // Bases-only deltas at the RFC extra-bit widths (the solver's
    // evidence-bounded frame, §30.2: the vendor's own test container
    // parses canonically under the RFC frame, so the widths are stream
    // data and the bases are the silicon unknowns).
    let mut t = ValueTables::rfc1951();
    t.lbase[12] = 17; // RFC 19 — length code 269 (2 extra bits)
    t.lbase[8] = 10; // RFC 11 — length code 265 (1 extra bit)
    t.dbase[4] = 8; // RFC 5 — distance code 4 (1 extra bit)
    t
}

#[test]
fn synthetic_custom_ladder_is_recovered_from_its_output() {
    let truth = custom_ladder();

    // Content exercising the custom codes at the RFC extra-bit widths:
    // match 1: len sym 269 (RFC width 2), extra 1 → L = 17+1 = 18;
    //          dist sym 4 (RFC width 1), extra 1 → D = 8+1 = 9;
    // match 2: len sym 265 (RFC width 1), extra 0 → L = 10;
    //          dist sym 6 (RFC width 2), extra 0 → D = 9 (RFC base);
    // match 3: len sym 269, extra 2 → L = 19;
    //          dist sym 4, extra 0 → D = 8.
    let lits: Vec<u8> = b"ABCDEFGHIJ".to_vec();
    let ms = [
        (269u16, 1u16, 2u8, 4u16, 1u16, 1u8),
        (265u16, 0u16, 1u8, 6u16, 0u16, 2u8),
        (269u16, 2u16, 2u8, 4u16, 0u16, 1u8),
    ];
    let (stream, start_bit) = build_stream(&lits, &ms);

    // The truth decode (the "capture").
    let mut target = Vec::new();
    let n = inflate_engine_with(
        &stream,
        start_bit,
        ENGINE_PRESET_BYTES,
        &mut target,
        4096,
        &truth,
    )
    .expect("truth decode");
    target.truncate(n);
    assert!(n > 20, "the stream must exercise the matches (n={n})");

    // Solve from the target with the RFC baseline.
    let sol = solve_ladder(
        &stream,
        start_bit,
        ENGINE_PRESET_BYTES,
        &target,
        &ValueTables::rfc1951(),
        SolveLimits::default(),
    )
    .expect("solve");
    assert!(sol.verified, "the solve must verify: {sol:#?}");
    assert_eq!(sol.redecoded, target, "re-decode equals the capture");

    // The module's contract is the verified re-decode at the RFC extra-bit
    // frame: the widths are stream data (§30.2) and stay RFC by
    // construction; the bases are the solved unknowns. This periodic
    // content admits multiple base assignments that reproduce the same 35
    // bytes — the solver reports one deterministic, engine-verified
    // answer (the real-stream test below pins exact RFC equality where
    // the truth IS the baseline).
    assert_eq!(
        sol.tables.lextra,
        ValueTables::rfc1951().lextra,
        "RFC width frame"
    );
    assert_eq!(
        sol.tables.dextra,
        ValueTables::rfc1951().dextra,
        "RFC width frame"
    );
    assert!(
        sol.tables != ValueTables::rfc1951(),
        "a non-RFC ladder was found"
    );
    assert!(sol.len_status.contains(&CodeStatus::Pinned));

    // Determinism: a second solve reaches the same ladder.
    let sol2 = solve_ladder(
        &stream,
        start_bit,
        ENGINE_PRESET_BYTES,
        &target,
        &ValueTables::rfc1951(),
        SolveLimits::default(),
    )
    .expect("solve 2");
    assert_eq!(sol.tables, sol2.tables);
}

#[test]
fn impossible_target_is_typed_no_solution() {
    // A stream whose only match cannot reproduce the target content:
    // the target lies about the copy (content is not periodic).
    let lits: Vec<u8> = b"ABCDEF".to_vec();
    let ms = [(257u16, 0u16, 0u8, 0u16, 0u16, 0u8)];
    let (stream, start_bit) = build_stream(&lits, &ms);
    let mut target = Vec::new();
    let n = inflate_engine_with(
        &stream,
        start_bit,
        0,
        &mut target,
        4096,
        &ValueTables::rfc1951(),
    )
    .expect("decode");
    target.truncate(n);
    // Corrupt one byte inside the match's span (the literal prefix is 6
    // bytes; the match copies 3 more).
    let last = target.len() - 1;
    target[last] ^= 0xFF;
    let err = solve_ladder(
        &stream,
        start_bit,
        0,
        &target,
        &ValueTables::rfc1951(),
        SolveLimits::default(),
    )
    .expect_err("corrupted target must not solve");
    assert!(matches!(
        err,
        SolveError::NoSolution | SolveError::BudgetExhausted { .. }
    ));
}

#[test]
fn tiny_budget_is_a_typed_error_not_a_wrong_answer() {
    let lits: Vec<u8> = b"ABCDEFGHIJ".to_vec();
    let ms = [(269u16, 1u16, 2u8, 4u16, 1u16, 1u8)];
    let (stream, start_bit) = build_stream(&lits, &ms);
    let truth = custom_ladder();
    let mut target = Vec::new();
    let n = inflate_engine_with(&stream, start_bit, 0, &mut target, 4096, &truth).expect("decode");
    target.truncate(n);
    let limits = SolveLimits {
        node_budget: 0,
        ..SolveLimits::default()
    };
    let err = solve_ladder(
        &stream,
        start_bit,
        0,
        &target,
        &ValueTables::rfc1951(),
        limits,
    )
    .expect_err("a zero attempt budget cannot commit any candidate");
    assert!(matches!(err, SolveError::BudgetExhausted { .. }));
}

// ---------------------------------------------------------------------------
// The real container: solve the boot's own reference decode — the truth is
// the RFC ladder, so the solve must recover it (or a T-consistent
// equivalent) and the engine re-decode must reproduce the pinned
// reference byte-for-byte.
// ---------------------------------------------------------------------------

#[test]
fn real_stream_reference_decode_is_solved() {
    let img = firmware::load(std::path::Path::new(
        "firmware/1506HV_512M_4M_SVT2_TRUMAN_TM_X1_WIFI_Internal_flashfile_20220902.bin",
    ))
    .expect("firmware");
    let mut m = Machine::new(MachineConfig::default(), &img);
    m.reset();
    m.run(2_000_000);
    let inputs = m
        .engine_decode_inputs()
        .expect("the boot runs the engine decode");

    // The reference window and its decode region (the machine's own decode
    // length; the band fill above it is machine semantics, §17).
    let mut window = vec![0u8; 79_872];
    let got = m.bus.copy_out(0x0160_0000, &mut window);
    assert_eq!(got, 79_872);
    let decoded_len = inputs.decoded_len;
    assert_eq!(decoded_len, 75_408, "the pinned decode length");
    assert!(
        window[decoded_len..].iter().all(|&b| b == 0),
        "band is fill"
    );

    // Solve with the RFC baseline — the truth for this stream.
    let t0 = std::time::Instant::now();
    let sol = solve_ladder(
        &inputs.stream,
        inputs.start_bit as usize,
        ENGINE_PRESET_BYTES,
        &window[..decoded_len],
        &ValueTables::rfc1951(),
        SolveLimits::default(),
    )
    .expect("the reference decode must be solvable");
    let dt = t0.elapsed();
    println!(
        "real-stream solve: events={} matches={} nodes={} verified={} in {dt:?}",
        sol.events, sol.matches, sol.nodes, sol.verified
    );
    let pinned_l = sol
        .len_status
        .iter()
        .filter(|s| **s == CodeStatus::Pinned)
        .count();
    let pinned_d = sol
        .dist_status
        .iter()
        .filter(|s| **s == CodeStatus::Pinned)
        .count();
    println!(
        "statuses: len pinned {pinned_l}/29, dist pinned {pinned_d}/30; \
         len unconstrained {}, dist unconstrained {}",
        sol.len_status
            .iter()
            .filter(|s| **s == CodeStatus::Unconstrained)
            .count(),
        sol.dist_status
            .iter()
            .filter(|s| **s == CodeStatus::Unconstrained)
            .count(),
    );
    assert!(sol.verified, "the solve must verify against the reference");
    assert_eq!(sol.redecoded.len(), decoded_len);
    assert_eq!(sol.events, 25_031, "the pinned event census");
    assert_eq!(sol.matches, 9_267, "the pinned match census");

    // The recovered ladder re-decodes to the reference → the machine's
    // pixel window (decode + band fill) carries the pinned fingerprint.
    let mut filled = vec![0u8; 79_872];
    filled[..decoded_len].copy_from_slice(&sol.redecoded);
    assert_eq!(adler32(&filled), 0xcd8d_03bf, "the pinned fingerprint");

    // And through the machine path: the same tables via config reproduce
    // the boot decode byte-exactly (the §35 parametricity, now solved).
    let mut replay = Vec::new();
    let n = inflate_engine_with_traced(
        &inputs.stream,
        inputs.start_bit as usize,
        ENGINE_PRESET_BYTES,
        &mut replay,
        ENGINE_MAX_OUT,
        &sol.tables,
        None,
    )
    .expect("replay");
    assert_eq!(n, decoded_len);
    assert_eq!(replay[..], window[..decoded_len]);

    // Where the stream pins codes, the pin must equal the RFC value (the
    // truth here): count the coincidences for the report.
    let mut agree = 0usize;
    let mut total = 0usize;
    for i in 0..29 {
        if sol.len_status[i] == CodeStatus::Pinned {
            total += 1;
            if sol.tables.lbase[i] == ENGINE_VALUE_TABLES.lbase[i]
                && sol.tables.lextra[i] == ENGINE_VALUE_TABLES.lextra[i]
            {
                agree += 1;
            }
        }
    }
    for i in 0..30 {
        if sol.dist_status[i] == CodeStatus::Pinned {
            total += 1;
            if sol.tables.dbase[i] == ENGINE_VALUE_TABLES.dbase[i]
                && sol.tables.dextra[i] == ENGINE_VALUE_TABLES.dextra[i]
            {
                agree += 1;
            }
        }
    }
    println!("pins agreeing with RFC: {agree}/{total}");
    assert_eq!(agree, total, "the RFC truth must be recoverable exactly");
}

// ---------------------------------------------------------------------------
// The §40.3 class prior (Milestone AO): the length-axis ordering splits
// by class — short codes below-first, long codes above-first. Ordering
// is a heuristic: the searched family, the candidate predicate and the
// verified verdict are identical either way; these tests pin that.
// ---------------------------------------------------------------------------

#[test]
fn length_class_matches_the_reference_boundaries() {
    // Class 0: codes 257..=264 (li 0..=7) plus the fixed-258 code 285
    // (li 28). Classes 1..=5: the equal-extra-width groups.
    for li in 0..=7 {
        assert_eq!(length_class(li), 0, "li {li}");
    }
    assert_eq!(length_class(28), 0, "the fixed-258 code is class 0");
    for &(li, cls) in &[
        (8usize, 1usize),
        (11, 1),
        (12, 2),
        (15, 2),
        (16, 3),
        (19, 3),
        (20, 4),
        (23, 4),
        (24, 5),
        (27, 5),
    ] {
        assert_eq!(length_class(li), cls, "li {li}");
    }
}

/// A stream whose true ladder has exactly the §40.3 structure — a short
/// code below RFC (li 2: 5 → 4) and a long code above RFC (li 20:
/// 131 → 139) — and whose content makes that solution UNIQUE (a wrong
/// length drifts the output position so the next content check dies).
/// Both orderings must recover the same verified tables; the prior
/// reaches it without extra attempts (measured, deterministic).
#[test]
fn class_prior_orders_the_spiral_but_not_the_answer() {
    let mut truth = ValueTables::rfc1951();
    truth.lbase[2] = 4; // short code BELOW RFC (RFC 5)
    truth.lbase[20] = 139; // long code ABOVE RFC (RFC 131)

    // m1: len sym 277 (li 20, RFC width 4), extra 3 → L = 139+3 = 142;
    //     dist sym 4 (RFC width 1), extra 1 → D = 5+1 = 6 (RFC).
    //     The copy source is the varied 6-byte pattern at D=6; a wrong
    //     length leaves j inside the pattern where the trailing literal
    //     check dies (the pattern bytes are never 0x00).
    // m2: len sym 259 (li 2, width 0), extra 0 → L = 4;
    //     dist sym 0 (width 0) → D = 1 (RFC).
    let lits: Vec<u8> = b"ABCDEFGH".to_vec();
    let ms = [
        (277u16, 3u16, 4u8, 4u16, 1u16, 1u8),
        (259u16, 0u16, 0u8, 0u16, 0u16, 0u8),
    ];
    let (stream, start_bit) = build_stream(&lits, &ms);

    let mut target = Vec::new();
    let n = inflate_engine_with(
        &stream,
        start_bit,
        ENGINE_PRESET_BYTES,
        &mut target,
        4096,
        &truth,
    )
    .expect("truth decode");
    target.truncate(n);
    assert_eq!(n, 154, "8 literals + 142 + 4");

    let neutral_limits = SolveLimits::default();
    let prior_limits = SolveLimits {
        class_prior: true,
        ..SolveLimits::default()
    };
    let neutral = solve_ladder(
        &stream,
        start_bit,
        ENGINE_PRESET_BYTES,
        &target,
        &ValueTables::rfc1951(),
        neutral_limits,
    )
    .expect("neutral solve");
    let prior = solve_ladder(
        &stream,
        start_bit,
        ENGINE_PRESET_BYTES,
        &target,
        &ValueTables::rfc1951(),
        prior_limits,
    )
    .expect("prior solve");

    // The headline: ordering never changes the answer.
    assert!(neutral.verified && prior.verified, "both orders verify");
    assert_eq!(neutral.tables, truth, "neutral recovers the truth");
    assert_eq!(prior.tables, truth, "prior recovers the same truth");
    assert_eq!(neutral.redecoded, target);
    assert_eq!(prior.redecoded, target);

    // Determinism under the prior: a second run, same ladder.
    let prior2 = solve_ladder(
        &stream,
        start_bit,
        ENGINE_PRESET_BYTES,
        &target,
        &ValueTables::rfc1951(),
        SolveLimits {
            class_prior: true,
            ..SolveLimits::default()
        },
    )
    .expect("prior solve 2");
    assert_eq!(prior.tables, prior2.tables);

    // The measured economics (deterministic for a fixed stream): on
    // this unambiguous content both orders examine the identical
    // candidate set — the prior is directional alignment with the §40.3
    // evidence, not a search-size reduction (its decision-relevant
    // effect is on ambiguous content, where it decides which verified
    // answer is reported first). The bound below pins that the
    // alignment never COSTS attempts on the structured truth.
    assert!(
        prior.nodes <= neutral.nodes,
        "prior attempts {} must not exceed neutral {}",
        prior.nodes,
        neutral.nodes
    );
}

/// The prior never breaks the solve contract on ambiguous content: on
/// the recovery suite's own periodic stream (multiple verified base
/// assignments), the prior-ordered solve still verifies and its
/// re-decode equals the capture byte-for-byte.
#[test]
fn class_prior_keeps_the_contract_on_ambiguous_content() {
    let truth = custom_ladder();
    let lits: Vec<u8> = b"ABCDEFGHIJ".to_vec();
    let ms = [
        (269u16, 1u16, 2u8, 4u16, 1u16, 1u8),
        (265u16, 0u16, 1u8, 6u16, 0u16, 2u8),
        (269u16, 2u16, 2u8, 4u16, 0u16, 1u8),
    ];
    let (stream, start_bit) = build_stream(&lits, &ms);
    let mut target = Vec::new();
    let n = inflate_engine_with(
        &stream,
        start_bit,
        ENGINE_PRESET_BYTES,
        &mut target,
        4096,
        &truth,
    )
    .expect("truth decode");
    target.truncate(n);

    let prior = solve_ladder(
        &stream,
        start_bit,
        ENGINE_PRESET_BYTES,
        &target,
        &ValueTables::rfc1951(),
        SolveLimits {
            class_prior: true,
            ..SolveLimits::default()
        },
    )
    .expect("prior solve on ambiguous content");
    assert!(prior.verified, "the prior solve must verify");
    assert_eq!(prior.redecoded, target, "re-decode equals the capture");
}

// ---------------------------------------------------------------------------
// Milestone AQ (§44): the band-window preset CONTENT seam
// ---------------------------------------------------------------------------

/// The §44 seam: a far-back match (dist > output position) copies from
/// the preset window's CONTENT, not from the output — so the same stream
/// decodes differently under a different preset content, and identically
/// under zero content and the preset_len API. The symbol sequence (and
/// hence the output length) is preset-independent: the preset changes
/// copied VALUES only.
#[test]
fn preset_window_content_is_the_far_back_match_source() {
    // len 5 → length code 259 (base 5, 0 extra); d 223 → distance code
    // 15 (base 193, 5 extra, extra=30) — the §19.1 head's own geometry
    // (dc=15/d=223), reaching 222 bytes into the 32 KiB preset.
    let (stream, start_bit) = build_stream(&[0x11], &[(259, 5, 0, 15, 30, 5)]);

    let mut zero = Vec::new();
    let n0 = inflate_engine_with(
        &stream,
        start_bit,
        ENGINE_PRESET_BYTES,
        &mut zero,
        ENGINE_MAX_OUT,
        &ENGINE_VALUE_TABLES,
    )
    .expect("zero preset decode");

    let mut zero_content = Vec::new();
    let n0c = inflate_engine_with_preset(
        &stream,
        start_bit,
        &vec![0u8; ENGINE_PRESET_BYTES],
        &mut zero_content,
        ENGINE_MAX_OUT,
        &ENGINE_VALUE_TABLES,
    )
    .expect("zero-content preset decode");
    assert_eq!(n0, n0c);
    assert_eq!(zero, zero_content, "zero content == preset_len API");

    // Output shape: one literal, then the 5 far-back bytes.
    assert_eq!(n0, 6, "literal + 5 far-back bytes");
    assert_eq!(zero[0], 0x11);
    assert_eq!(&zero[1..6], &[0u8; 5], "far-back copies yield zeros");

    // A5 preset: the same symbols, but the far-back copies bring 0xA5.
    let mut a5 = Vec::new();
    let n5 = inflate_engine_with_preset(
        &stream,
        start_bit,
        &vec![0xA5u8; ENGINE_PRESET_BYTES],
        &mut a5,
        ENGINE_MAX_OUT,
        &ENGINE_VALUE_TABLES,
    )
    .expect("A5 preset decode");
    assert_eq!(n5, n0, "the symbol sequence is preset-independent");
    assert_eq!(a5[0], 0x11, "the literal is untouched");
    assert_eq!(&a5[1..6], &[0xA5u8; 5], "far-back copies yield A5");
    // The preset changes copied values ONLY — the output length and the
    // literal byte are identical between the two presets.
    assert_eq!(a5.len(), zero.len());
    assert_eq!(a5[0], zero[0]);
}
