//! Milestone P probe 1 (§17.5): the length ladder as 29 unknown ROM values.
//!
//! Hypothesis: the engine's LZ implements length/distance as fixed ROM
//! ladders (a simple-hardware design). Under the m2 bit accounting
//! (length extra bits consumed per RFC counts, distance extra bits NOT
//! consumed — the only family that fills the 79,872-byte image exactly,
//! Milestone O) the length ladder values are unknown silicon constants.
//!
//! This tool searches the 29 length-ladder values against the
//! image-truncated strict validity oracle (§14.2), with:
//!   * coordinate descent from the RFC ladder (the H1 baseline),
//!   * a monotone-constrained variant (a real ROM ladder is ordered),
//!   * deterministic perturbation restarts.
//!
//! The oracle is the crate's own MIPS32r2 decoder (`cpu::mips32::describe`)
//! calibrated on the flash's known-good code regions — the same method as
//! the Python tools (§14.2), cross-checked by tools/fw_ladder_verify.py
//! (capstone) for the reported finalists.
//!
//! Usage: cargo run --release --example ladder_search -- <flash.bin>
use std::collections::HashMap;
use std::env;

use truman_emulator::cpu::mips32::describe;

// ---------------------------------------------------------------------------
// Stream container geometry (docs/firmware-analysis.md §13, §16)
// ---------------------------------------------------------------------------
const CONTAINER_OFF: usize = 0x1002C;
const CONTAINER_LEN: usize = 0x868C;
const STREAM_SKIP: usize = 8; // "SP" + length prefix
const START_BIT: usize = 1024; // verified data start (§14: header ends at 1008)
const IMG: usize = 79_872; // 78 rows x 1024 px
const PRESET: usize = 0x8000; // 32 KiB zero preset dictionary (driver zero-fill)

const LEXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DBASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];
const RFC_LBASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];

// ---------------------------------------------------------------------------
// Bit reader (LSB-first within a byte, as the driver uploads the tables)
// ---------------------------------------------------------------------------
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }
    fn bit(&mut self) -> Option<u32> {
        let byte = *self.data.get(self.pos >> 3)?;
        let b = ((byte >> (self.pos & 7)) & 1) as u32;
        self.pos += 1;
        Some(b)
    }
    fn bits(&mut self, n: u8) -> Option<u32> {
        let mut v = 0;
        for i in 0..n {
            v |= self.bit()? << i;
        }
        Some(v)
    }
}

// ---------------------------------------------------------------------------
// Canonical Huffman (counts / first-code / symbol lists, as the driver builds)
// ---------------------------------------------------------------------------
struct Huffman {
    maxbits: usize,
    counts: Vec<usize>,
    first: Vec<u32>,
    by_len: Vec<Vec<u16>>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Option<Self> {
        let maxbits = *lengths.iter().max()? as usize;
        if maxbits == 0 {
            return None;
        }
        let mut counts = vec![0usize; maxbits + 1];
        for &l in lengths {
            if l > 0 {
                counts[l as usize] += 1;
            }
        }
        let mut first = vec![0u32; maxbits + 1];
        let mut code = 0u32;
        for b in 1..=maxbits {
            code = (code + counts[b - 1] as u32) << 1;
            first[b] = code;
        }
        let mut by_len = vec![Vec::new(); maxbits + 1];
        for (s, &l) in lengths.iter().enumerate() {
            if l > 0 {
                by_len[l as usize].push(s as u16);
            }
        }
        Some(Self {
            maxbits,
            counts,
            first,
            by_len,
        })
    }
    fn decode(&self, br: &mut BitReader) -> Option<u16> {
        let mut code = 0u32;
        for l in 1..=self.maxbits {
            code = (code << 1) | br.bit()?;
            let c = self.counts[l];
            if c > 0 {
                let f = self.first[l];
                if code >= f && (code as usize) < f as usize + c {
                    return Some(self.by_len[l][(code - f) as usize]);
                }
            }
        }
        None
    }
}

/// Parse the dynamic block header at bit 0 of `stream`.
fn parse_header(stream: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut br = BitReader::new(stream, 0);
    let _bfinal = br.bit()?;
    let btype = br.bits(2)?;
    if btype != 2 {
        return None;
    }
    let hlit = br.bits(5)? as usize + 257;
    let hdist = br.bits(5)? as usize + 1;
    let hclen = br.bits(4)? as usize + 4;
    let mut cl = [0u8; 19];
    for i in 0..hclen {
        cl[CL_ORDER[i]] = br.bits(3)? as u8;
    }
    let clh = Huffman::new(&cl)?;
    let mut lens = Vec::with_capacity(hlit + hdist);
    while lens.len() < hlit + hdist {
        let s = clh.decode(&mut br)?;
        match s {
            0..=15 => lens.push(s as u8),
            16 => {
                let prev = *lens.last()?;
                let n = br.bits(2)? + 3;
                for _ in 0..n {
                    lens.push(prev);
                }
            }
            17 => {
                let n = br.bits(3)? + 3;
                lens.extend(std::iter::repeat_n(0u8, n as usize));
            }
            18 => {
                let n = br.bits(7)? + 11;
                lens.extend(std::iter::repeat_n(0u8, n as usize));
            }
            _ => return None,
        }
    }
    Some((lens[..hlit].to_vec(), lens[hlit..hlit + hdist].to_vec()))
}

// ---------------------------------------------------------------------------
// Validity oracle: the crate's decoder, calibrated on the flash's code
// ---------------------------------------------------------------------------
struct Oracle {
    common: std::collections::HashSet<String>,
    cache: HashMap<u32, bool>,
}

impl Oracle {
    /// Calibrate the common-mnemonic set on flash[0x0000..0x10000) (the
    /// stage-1 + stage-2 code regions, same as the Python tools).
    fn calibrate(flash: &[u8]) -> Self {
        let mut hist: HashMap<String, usize> = HashMap::new();
        let mut total = 0usize;
        for off in (0..0x10000).step_by(4) {
            let word =
                u32::from_le_bytes([flash[off], flash[off + 1], flash[off + 2], flash[off + 3]]);
            if let Some(m) = mnemonic(word) {
                *hist.entry(m).or_insert(0) += 1;
                total += 1;
            }
        }
        let common: std::collections::HashSet<String> = hist
            .into_iter()
            .filter(|(_, c)| *c >= 8 && (*c as f64) / (total as f64) > 0.0005)
            .map(|(m, _)| m)
            .collect();
        Self {
            common,
            cache: HashMap::new(),
        }
    }
    fn word_valid(&mut self, word: u32) -> bool {
        if let Some(&v) = self.cache.get(&word) {
            return v;
        }
        let v = mnemonic(word)
            .map(|m| self.common.contains(&m))
            .unwrap_or(false);
        self.cache.insert(word, v);
        v
    }
    /// Strict validity: fraction of 4-byte words in `buf` that decode to a
    /// common mnemonic.
    fn strict(&mut self, buf: &[u8]) -> f64 {
        let n = buf.len() / 4;
        if n == 0 {
            return 0.0;
        }
        let mut ok = 0usize;
        let (words, _) = buf.as_chunks::<4>();
        for w in words {
            if self.word_valid(u32::from_le_bytes(*w)) {
                ok += 1;
            }
        }
        ok as f64 / n as f64
    }
}

/// First token of the crate's instruction description, None if not decodable.
fn mnemonic(word: u32) -> Option<String> {
    let d = describe(word);
    let tok = d.split_whitespace().next()?;
    // The crate's fallbacks name the raw encoding ("special(0x..)", op
    // fallbacks); treat any parenthesized raw-encoding dump as undecodable.
    if tok.contains('(') {
        None
    } else {
        Some(tok.to_string())
    }
}

// ---------------------------------------------------------------------------
// The m2 walk with a parametric length ladder.
//
// Window model = the tools' model (§13.1): the reference buffer is the
// driver's 32 KiB zero preset followed by the entire output (appending,
// not sliding) — distances up to 24577 always resolve, and a distance
// beyond the output start reads the zero preset.
// ---------------------------------------------------------------------------
struct Walk {
    out: Vec<u8>,
    n_sym: usize,
    image_full: bool,
}

/// Symbol-statistics of the (value-independent) m2 parse: per-length-symbol
/// match counts and extra-bit sums, literal count, total bits.
struct ParseStats {
    count: [usize; 29],
    extra_sum: usize,
    n_lit: usize,
    total_bits: usize,
}

fn parse_stats(stream: &[u8], start_bit: usize, lith: &Huffman, disth: &Huffman) -> ParseStats {
    let mut br = BitReader::new(stream, start_bit);
    let mut count = [0usize; 29];
    let mut extra_sum = 0usize;
    let mut n_lit = 0usize;
    while let Some(s) = lith.decode(&mut br) {
        if s <= 255 {
            n_lit += 1;
            continue;
        }
        if s == 256 {
            continue;
        }
        let li = (s - 257) as usize;
        if li >= 29 {
            break;
        }
        count[li] += 1;
        let eb = LEXTRA[li];
        if eb > 0 {
            match br.bits(eb) {
                Some(v) => extra_sum += v as usize,
                None => break,
            }
        }
        let ds = match disth.decode(&mut br) {
            Some(d) => d as usize,
            None => break,
        };
        if ds >= 30 {
            break;
        }
    }
    ParseStats {
        count,
        extra_sum,
        n_lit,
        total_bits: br.pos,
    }
}

fn walk_m2(
    stream: &[u8],
    start_bit: usize,
    lith: &Huffman,
    disth: &Huffman,
    ladder: &[u16; 29],
) -> Walk {
    let mut br = BitReader::new(stream, start_bit);
    let mut buf = vec![0u8; PRESET]; // reference: zero preset + all output
    let mut out = Vec::with_capacity(IMG + 64);
    let mut n_sym = 0usize;
    let mut image_full = false;
    while (br.pos >> 3) < stream.len() {
        let s = match lith.decode(&mut br) {
            Some(s) => s,
            None => break,
        };
        n_sym += 1;
        if s <= 255 {
            buf.push(s as u8);
            out.push(s as u8);
            if out.len() >= IMG {
                image_full = true;
                break;
            }
            continue;
        }
        if s == 256 {
            continue;
        }
        let li = (s - 257) as usize;
        if li >= 29 {
            break;
        }
        let mut ln = ladder[li] as usize;
        let eb = LEXTRA[li];
        if eb > 0 {
            if let Some(v) = br.bits(eb) {
                ln += v as usize;
            } else {
                break;
            }
        }
        let ds = match disth.decode(&mut br) {
            Some(d) => d as usize,
            None => break,
        };
        if ds >= 30 {
            break;
        }
        let d = DBASE[ds] as usize;
        for _j in 0..ln {
            // Overlapping-copy semantics: the source is d bytes behind the
            // CURRENT end, so a match longer than its distance re-reads the
            // bytes it just produced (as the tools' reference walk does).
            let src = buf.len() - d;
            let b = buf[src];
            buf.push(b);
            out.push(b);
        }
        if out.len() >= IMG {
            image_full = true;
            break;
        }
    }
    Walk {
        out,
        n_sym,
        image_full,
    }
}

/// Size band for a plausible semantics (the linear size anchor):
/// the full-parse decode length S(L) = n_lit + extra_sum + sum(count[s]*L[s])
/// must not materially overshoot the image (the engine stops at image end;
/// only the final match can spill past it) and must substantially fill it
/// (the vendor tool encoded ~75 kB of content; a true decode far below that
/// would make the container mostly waste).
const SIZE_MAX: usize = IMG + 258;
const SIZE_MIN: usize = 70_000;

fn score(
    ladder: &[u16; 29],
    stream: &[u8],
    lith: &Huffman,
    disth: &Huffman,
    o: &mut Oracle,
    stats: &ParseStats,
) -> f64 {
    let mut s_size = stats.n_lit + stats.extra_sum;
    for (count, base) in stats.count.iter().zip(ladder.iter()) {
        s_size += count * *base as usize;
    }
    if !(SIZE_MIN..=SIZE_MAX).contains(&s_size) {
        return 0.0; // size anchor violated: no walk needed
    }
    let w = walk_m2(stream, START_BIT, lith, disth, ladder);
    let img = &w.out[..w.out.len().min(IMG)];
    o.strict(img)
}

/// Full-parse decode length for a candidate ladder (no walk).
fn full_size(ladder: &[u16; 29], stats: &ParseStats) -> usize {
    let mut s_size = stats.n_lit + stats.extra_sum;
    for (count, base) in stats.count.iter().zip(ladder.iter()) {
        s_size += count * *base as usize;
    }
    s_size
}

/// Deterministic 64-bit LCG for restarts.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

fn main() {
    let path = env::args()
        .nth(1)
        .expect("usage: ladder_search <flash.bin>");
    let flash = std::fs::read(&path).expect("flash read");
    assert!(
        flash.len() >= CONTAINER_OFF + CONTAINER_LEN,
        "flash too small"
    );

    let cont = &flash[CONTAINER_OFF..CONTAINER_OFF + CONTAINER_LEN];
    let stream = &cont[STREAM_SKIP..];
    let (litlens, distlens) = parse_header(stream).expect("SP dynamic header");
    let lith = Huffman::new(&litlens).expect("litlen table");
    let disth = Huffman::new(&distlens).expect("dist table");
    let stats = parse_stats(stream, START_BIT, &lith, &disth);
    eprintln!(
        "m2 parse: {} literals, matches per len-symbol {:?}, extra-bit sum {}, {} bits ({:.1}%) of stream",
        stats.n_lit,
        stats.count,
        stats.extra_sum,
        stats.total_bits,
        stats.total_bits as f64 / (stream.len() * 8) as f64 * 100.0
    );
    eprintln!(
        "stream: header parsed ({} litlen / {} dist coded syms), data from bit {}",
        lith.by_len.iter().map(|v| v.len()).sum::<usize>(),
        disth.by_len.iter().map(|v| v.len()).sum::<usize>(),
        START_BIT
    );

    let mut oracle = Oracle::calibrate(&flash);
    eprintln!(
        "oracle: {} common mnemonics calibrated on flash[0..0x10000)",
        oracle.common.len()
    );

    let base = RFC_LBASE;
    let t0 = std::time::Instant::now();
    let base_score = score(&base, stream, &lith, &disth, &mut oracle, &stats);
    let base_walk = walk_m2(stream, START_BIT, &lith, &disth, &base);
    eprintln!(
        "baseline RFC ladder under m2: strict {:.1}% (out={} syms={} full={})",
        base_score * 100.0,
        base_walk.out.len(),
        base_walk.n_sym,
        base_walk.image_full
    );

    // The size anchor (new in Milestone P): the vendor tool encoded the
    // 79,872-byte image and the stream's parse consumes every bit to input
    // end, so under the engine's TRUE length ladder the full-parse decode
    // length must land at the image size. The RFC ladder overshoots to
    // 103,014 (Milestone O's "fills exactly" was the walk's image cap, not
    // an input-end property). This linear anchor prunes the whole
    // degenerate-exploitation region the beam search fell into.
    let s_rfc = full_size(&base, &stats);
    eprintln!(
        "size anchor: RFC full-parse size {} vs image {} (need sum(count*L) {})",
        s_rfc,
        IMG,
        IMG - stats.n_lit - stats.extra_sum
    );

    // Phase 0: in-band seed generation. Greedy count-weighted decrements
    // from the RFC ladder until the linear size lands in band, then jitter.
    let mut seeds: Vec<[u16; 29]> = Vec::new();
    let mut rng = Lcg(0x54_4D_58_31_50_32);
    let total_count: usize = stats.count.iter().sum();
    let mut cum = Vec::with_capacity(29);
    let mut acc = 0usize;
    for s in 0..29 {
        acc += stats.count[s];
        cum.push(acc);
    }
    let pick = |rng: &mut Lcg| -> usize {
        let r = (rng.next() as usize) % total_count;
        match cum.binary_search(&r) {
            Ok(i) => i,
            Err(i) => i,
        }
    };
    for seed_i in 0..600 {
        let mut l = base;
        let mut guard = 0;
        while full_size(&l, &stats) > SIZE_MAX && guard < 100_000 {
            guard += 1;
            let s = pick(&mut rng);
            if l[s] > 1 {
                l[s] -= 1;
            }
        }
        if full_size(&l, &stats) < SIZE_MIN {
            continue;
        }
        // jitter: a few band-preserving random +-1 moves
        for _ in 0..(seed_i % 5) {
            let s = pick(&mut rng);
            let up = rng.next() & 1 == 0;
            let mut c = l;
            if up {
                c[s] += 1;
            } else if c[s] > 1 {
                c[s] -= 1;
            }
            let sz = full_size(&c, &stats);
            if (SIZE_MIN..=SIZE_MAX).contains(&sz) {
                l = c;
            }
        }
        if !seeds.contains(&l) {
            seeds.push(l);
        }
    }
    eprintln!("seeds: {} in-band ladders generated", seeds.len());

    // Phase 1: score the seeds, keep the top few.
    let mut scored: Vec<(f64, [u16; 29])> = seeds
        .iter()
        .map(|l| (score(l, stream, &lith, &disth, &mut oracle, &stats), *l))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    scored.truncate(8);
    for (sc, l) in &scored {
        eprintln!("seed: {:.2}%  S={}", sc * 100.0, full_size(l, &stats));
    }

    // Phase 2: band-preserving coordinate descent from the top seeds.
    // Single-symbol moves first; pair moves (the size constraint couples
    // symbols, so single moves can plateau) over the high-count symbols.
    let mut best = scored[0].1;
    let mut best_score = scored[0].0;
    for (seed_sc, seed_l) in &scored {
        let mut cur = *seed_l;
        let mut cur_sc = *seed_sc;
        for _round in 0..8 {
            let mut improved = false;
            for s in 0..29 {
                let mut local_v = cur[s];
                let mut local_sc = cur_sc;
                for d in -12i32..=12 {
                    if d == 0 {
                        continue;
                    }
                    let v = (cur[s] as i32 + d).clamp(1, 1023) as u16;
                    if v == cur[s] {
                        continue;
                    }
                    let mut c = cur;
                    c[s] = v;
                    let sz = full_size(&c, &stats);
                    if !(SIZE_MIN..=SIZE_MAX).contains(&sz) {
                        continue; // stay in the size band
                    }
                    let sc = score(&c, stream, &lith, &disth, &mut oracle, &stats);
                    if sc > local_sc {
                        local_sc = sc;
                        local_v = v;
                    }
                }
                if local_v != cur[s] {
                    cur[s] = local_v;
                    cur_sc = local_sc;
                    improved = true;
                }
            }
            if !improved {
                // pair moves over the top-count symbols
                'pairs: for a in 0..8 {
                    for b in (a + 1)..9 {
                        // top-9 count symbols
                        let sa = top_count_symbol(&stats, a);
                        let sb = top_count_symbol(&stats, b);
                        for da in -3i32..=3 {
                            for db in -3i32..=3 {
                                if da == 0 && db == 0 {
                                    continue;
                                }
                                let mut c = cur;
                                let va = (c[sa] as i32 + da).clamp(1, 1023) as u16;
                                let vb = (c[sb] as i32 + db).clamp(1, 1023) as u16;
                                if va == c[sa] || vb == c[sb] {
                                    continue;
                                }
                                c[sa] = va;
                                c[sb] = vb;
                                let sz = full_size(&c, &stats);
                                if !(SIZE_MIN..=SIZE_MAX).contains(&sz) {
                                    continue;
                                }
                                let sc = score(&c, stream, &lith, &disth, &mut oracle, &stats);
                                if sc > cur_sc {
                                    cur = c;
                                    cur_sc = sc;
                                    improved = true;
                                    break 'pairs;
                                }
                            }
                        }
                    }
                }
            }
            if !improved {
                break;
            }
        }
        eprintln!(
            "descent from seed {:.2}% -> {:.2}%  S={}",
            seed_sc * 100.0,
            cur_sc * 100.0,
            full_size(&cur, &stats)
        );
        if cur_sc > best_score {
            best_score = cur_sc;
            best = cur;
        }
    }

    // Phase 3: monotone projection of the winner (a ROM ladder is ordered);
    // rescored only if the projection stays in band.
    let mut mono = best;
    for s in 1..29 {
        if mono[s] < mono[s - 1] {
            mono[s] = mono[s - 1];
        }
    }
    let mono_sz = full_size(&mono, &stats);
    let mono_score = if (SIZE_MIN..=SIZE_MAX).contains(&mono_sz) {
        score(&mono, stream, &lith, &disth, &mut oracle, &stats)
    } else {
        f64::NAN
    };

    // -- Report ------------------------------------------------------------
    let w = walk_m2(stream, START_BIT, &lith, &disth, &best);
    println!("== Milestone P probe 1: length-ladder ROM search (m2 accounting) ==");
    println!(
        "size anchor: image {} ; RFC ladder full-parse {} ; band [{}, {}]",
        IMG, s_rfc, SIZE_MIN, SIZE_MAX
    );
    println!(
        "baseline RFC ladder     : out-of-band under the anchor (S={})",
        s_rfc
    );
    println!(
        "best in-band ladder     : {:.2}% strict  ladder {:?} (monotone={})",
        best_score * 100.0,
        best,
        best.windows(2).all(|p| p[0] <= p[1])
    );
    println!(
        "monotone projection     : {:.2}%  S={} (monotone projection of the winner)",
        mono_score * 100.0,
        mono_sz
    );
    println!(
        "best walk: syms={} image_full={} out={} full_parse_size={}",
        w.n_sym,
        w.image_full,
        w.out.len(),
        full_size(&best, &stats)
    );
    println!(
        "bits/symbol (full parse): {:.2}",
        stats.total_bits as f64 / (stats.n_lit + stats.count.iter().sum::<usize>()) as f64
    );
    println!("elapsed: {:.1}s", t0.elapsed().as_secs_f64());
    println!();
    println!("Interpretation: a REAL semantics would (a) score far above the");
    println!("~48-56% baselines, (b) show structure (monotone, stable). An");
    println!("in-band ceiling near the baseline refutes the m2 length-ladder");
    println!("family (with the distance ladder at its RFC values).");
}

/// The i-th highest-count length symbol (for pair moves).
fn top_count_symbol(stats: &ParseStats, i: usize) -> usize {
    let mut idx: Vec<usize> = (0..29).collect();
    idx.sort_by(|&x, &y| stats.count[y].cmp(&stats.count[x]));
    idx[i]
}
