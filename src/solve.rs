//! The capture→ladder solver (Milestone AJ, §37): recover the engine's
//! length/distance value ladder from one true decode output.
//!
//! This is the mechanical solve step of the P1 DIVERGENT row
//! (docs/hardware-session.md): a real device's post-decode DRAM capture
//! (P1) plus the boot's own bit stream determine the ladder by inversion,
//! with no search over hypothesis families (§15.6/§18.2/§26 swept those
//! without the output and every class was refuted; the m4 accounting is
//! underdetermined *without* the true pattern, §30.3 — with it, every
//! match's content pins its length and distance).
//!
//! The search frame is evidence-bounded (§30.2): the vendor's own test
//! container parses **canonically** under the RFC frame — its literals
//! decode to readable text before the first distance violation — so the
//! extra-bit **widths** are the RFC frame's, and the silicon unknowns are
//! exactly the 29+30 **base values** (the §30.4 "59-constant" blocker).
//! Consequences the solver exploits:
//!
//! * Symbol positions depend only on the Huffman code lengths and the
//!   (now fixed) extra-bit widths: the whole symbol sequence is a
//!   deterministic function of the stream. A wrong base can never move
//!   the parse — it can only fail a content check. The search is a
//!   sequence of per-code base commitments with deferred contradictions,
//!   not a joint structure search.
//! * Per match, the length `L` and distance `D` must satisfy the target's
//!   own periodicity: `target[j+i] == src(j+i-D)` for all `i < L`, where
//!   `src(x)` is the zero preset for `x < 0` and `target[x]` otherwise —
//!   the engine's live-window copy semantics (the overlap case reads the
//!   bytes the copy itself wrote, which by the output invariant are
//!   target bytes).
//! * A code's first occurrence commits its base from the smallest
//!   content-consistent value; later occurrences check. Candidates are
//!   ordered with the baseline (RFC) values first — the null hypothesis
//!   this repo is framed against — then by Manhattan distance from that
//!   point. When [`SolveLimits::class_prior`] is set, the length axis
//!   follows the §40.3 class structure instead: short codes (class 0)
//!   try at-or-below the baseline first, longer classes try above it
//!   first (the unconditional opposite-sign fact — AM). Ordering is a
//!   heuristic only: every candidate passes the same content predicate,
//!   and coverage is bounded by the documented caps and the attempt
//!   budget, never by the heuristic — both orders search the identical
//!   family and (pinned by tests) recover the same verified answer.
//! * A step whose reads run past input end is the engine's clean stop
//!   (`OutOfInput` → break, §13): with fixed widths that boundary is
//!   stream-determined. If the target is fully explained there, the solve
//!   ends; codes exercised only by that final step keep the baseline
//!   bases and are reported as [`CodeStatus::Unconstrained`] — their
//!   base is unobservable (the copy never runs).
//! * Every committed constraint is derived from a copy that reproduces
//!   the target — the final tables re-decode the stream through the real
//!   engine path (`crate::inflate::inflate_engine_with_traced`), and that
//!   byte-comparison is the reported verdict (`verified`). No surrogate
//!   oracle: the verification IS the engine.
//!
//! Codes the stream never exercises stay at the baseline (RFC) values and
//! are reported as [`CodeStatus::Unconstrained`] — they cannot move this
//! stream's decode, and claiming otherwise would be a guess (§30.3's
//! discipline).
//!
//! Bounded by construction: [`SolveLimits`] caps the value ranges, the
//! per-occurrence distance scan, the candidate spiral's radius and the
//! total attempt budget; exhaustion is a typed error, never a silent
//! partial answer.

use crate::inflate::{
    inflate_engine_with_traced, parse_dynamic_header, BitReader, InflateError, PeekSym, ValueTables,
};

/// Bounded solve parameters. The bounds are the documented assumptions,
/// each forced by the engine model rather than by RFC convention where
/// they differ: lengths ≥ 1 (the engine's copy loop accepts any length;
/// RFC's ≥ 3 is a ladder property, not a decoder one — and the §30.3 m4
/// family's ladders emit shorter matches), length ≤ 258 (the RFC ladder's
/// own maximum — the engine is a DEFLATE decoder, `zlib_mode=1`, §30.1),
/// distance ≤ 32768 (the 32 KiB window the driver-side buffers implement,
/// §34.3).
#[derive(Debug, Clone)]
pub struct SolveLimits {
    /// Minimum match length the ladder can express (the engine model's
    /// own floor: the copy loop accepts any length).
    pub min_len: u16,
    /// Maximum match length the ladder can express.
    pub max_len: u16,
    /// Maximum match distance (the engine's window, preset included).
    pub max_dist: u16,
    /// Cap on the ascending distance scan per occurrence (the baseline
    /// candidate usually wins; the cap only bounds refuted-family walks).
    pub dscan_cap: u32,
    /// Cap on the candidate spiral's radius around the baseline point
    /// (Manhattan distance in (D, L) space). Targets whose truth lies
    /// farther from the baseline at some first occurrence are outside the
    /// enumerated family — a typed failure, never a silent guess.
    pub spiral_radius_cap: u32,
    /// Total candidate-attempt budget before giving up.
    pub node_budget: u64,
    /// Order the length-axis spiral by the §40.3 class prior: short
    /// codes (class 0 — 257..=264 and the fixed-258 285) try at-or-below
    /// the baseline first, longer classes try above the baseline first
    /// (the unconditional opposite-sign structure, Milestone AM: the
    /// vector needs its short-dominated decode shorter than RFC, Truman
    /// needs its net longer, so the surplus lives on the long codes).
    /// Ordering is a heuristic only — the candidate predicate, the caps
    /// and the searched family are identical either way, and the solved
    /// tables are pinned invariant in tests. Default false (the
    /// symmetric order); `ladder_solve` enables it.
    pub class_prior: bool,
}

impl Default for SolveLimits {
    fn default() -> Self {
        SolveLimits {
            min_len: 1,
            max_len: 258,
            max_dist: 32768,
            dscan_cap: 4096,
            spiral_radius_cap: 256,
            node_budget: 4_000_000,
            class_prior: false,
        }
    }
}

/// Solve failure, typed (never a silent partial answer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveError {
    /// The bit stream itself does not parse.
    Stream(InflateError),
    /// Empty targets are degenerate (a decode produces bytes or stops).
    EmptyTarget,
    /// Every base assignment was refuted by the target content.
    NoSolution,
    /// The candidate-attempt budget ran out before the space was
    /// exhausted.
    BudgetExhausted { nodes: u64 },
}

impl std::fmt::Display for SolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolveError::Stream(e) => write!(f, "stream parse: {e}"),
            SolveError::EmptyTarget => write!(f, "empty target"),
            SolveError::NoSolution => {
                write!(f, "no ladder reproduces the target within the limits")
            }
            SolveError::BudgetExhausted { nodes } => {
                write!(f, "candidate budget exhausted ({nodes} attempts)")
            }
        }
    }
}

impl std::error::Error for SolveError {}

/// The §40.3 length-class of a length code index (`li = sym − 257`,
/// 0..=28): class 0 covers codes 257..=264 plus the fixed-258 code 285;
/// classes 1..=5 are the equal-extra-width groups (RFC widths 1..5).
/// Matches the reference implementation's `class_of`
/// (`tools/fw_vector_probe.py`). The §40.3 joint anchor system assigns
/// this class structure to the true ladder: short codes at-or-below RFC,
/// long codes above — the solver's [`SolveLimits::class_prior`] ordering
/// encodes exactly that split.
pub fn length_class(li: usize) -> usize {
    if !(8..28).contains(&li) {
        0
    } else {
        (li - 8) / 4 + 1
    }
}

/// Per-code provenance in the solved ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeStatus {
    /// The stream never exercises this code — or exercises it only at the
    /// input-exhaustion boundary, where the copy never runs and the base
    /// is unobservable. The baseline (RFC) value is kept; it CANNOT have
    /// moved this decode (the honesty rule of §37).
    Unconstrained,
    /// Exactly one base value is consistent with the target (at the RFC
    /// extra-bit width).
    Pinned,
}

/// A solved ladder plus its verification record.
#[derive(Debug, Clone)]
pub struct LadderSolution {
    /// The recovered ladder (unconstrained codes at baseline values; the
    /// extra-bit widths are the RFC frame's by construction).
    pub tables: ValueTables,
    /// Per-code provenance, length codes 257..=285.
    pub len_status: [CodeStatus; 29],
    /// Per-code provenance, distance codes 0..=29.
    pub dist_status: [CodeStatus; 30],
    /// Symbols on the verification re-decode (the final path only).
    pub events: usize,
    /// Matches on the verification re-decode.
    pub matches: usize,
    /// Candidate attempts charged across the search.
    pub nodes: u64,
    /// The engine's own re-decode under the solved tables.
    pub redecoded: Vec<u8>,
    /// `redecoded == target` — the only verdict this module reports.
    pub verified: bool,
}

/// Per-code base commitment. `None` = unconstrained so far; `Some(base)`
/// after the first occurrence (eager pinning keeps one value; refuted
/// values having been backtracked).
type Bases<const N: usize> = [Option<u16>; N];

#[derive(Clone)]
struct Sets {
    lbase: Bases<29>,
    dbase: Bases<30>,
    /// `Σ count[k]·base[k]` over the pinned length codes (the guard's
    /// running left-hand side).
    committed: u64,
    /// `Σ count[k]·min_base[k]` / `·max_base[k]` over the UNPINNED
    /// exercised codes.
    min_remaining: u64,
    max_remaining: u64,
}

impl Sets {
    fn new() -> Self {
        Sets {
            lbase: Default::default(),
            dbase: Default::default(),
            committed: 0,
            min_remaining: 0,
            max_remaining: 0,
        }
    }
}

/// One candidate commit at a match: the (D, L) values it would pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Commit {
    d: u16,
    l: u16,
}

/// A search frame: one branching match step, with the (D, L) spiral
/// cursors. `saved` is the pre-apply state for backtracking.
struct Frame {
    p: usize,
    j: usize,
    li: usize,
    clen: u8,
    /// The step's fixed geometry: length extra width, distance symbol,
    /// its code length and extra width (all baseline-frame values).
    w_l: u8,
    dsym: u16,
    dcl: u8,
    w_d: u8,
    /// Spiral radius (Manhattan distance from the baseline point).
    r: u32,
    /// Position within the radius: dd = k - r (signed).
    k: u32,
    /// Sub-candidate within (r, k): 0 = l0 - dl, 1 = l0 + dl.
    sub: u8,
    /// Cached max run at the pinned distance, when the distance side is
    /// pinned.
    m_cache: Option<u32>,
    saved: Sets,
}

/// The solver's read of the target: `src(x)` is the engine's window byte
/// at output-relative offset `x` — the zero preset for `x < 0`, else the
/// target itself (the output invariant makes the live copy readable from
/// the target; see the module docs).
fn window_byte(target: &[u8], x: i64) -> u8 {
    if x < 0 {
        0
    } else {
        target[x as usize]
    }
}

/// Length the copy starting at output position `j` with distance `d` can
/// run before the first mismatch against the target, capped at `cap`.
fn run_len(target: &[u8], j: usize, d: u16, cap: usize) -> usize {
    let d = d as i64;
    let mut i = 0usize;
    while i < cap {
        if window_byte(target, j as i64 - d + i as i64) != target[j + i] {
            break;
        }
        i += 1;
    }
    i
}

/// Apply outcome: the step advanced, or it was rejected (this frame's
/// own candidate failures are retried locally by the rewind).
enum Apply {
    Advanced,
    Rejected(()),
}

/// Outcome of one advance step batch (see [`Solver::advance`]).
enum Flow {
    /// The target is fully explained at this path.
    Success,
    /// This path is refuted; rewind to the nearest branching frame.
    DeadEnd,
}

/// The fixed parse's length-side accounting (the bit-level parse is
/// base-independent with fixed widths, so this is exact stream data):
/// per length code, how many matches use it and the total extra bits it
/// reads. The decode's own output length is then a LINEAR equation in
/// the bases: `Σ count[k]·base[k] = target.len() − literals − Σ extra`.
#[derive(Debug, Clone)]
struct SumGuard {
    /// Match counts per length code 257..=285.
    count: [u64; 29],
    /// Per-code base lower bound: `max(0, min_len − max extra read)`.
    min_base: [u64; 29],
    /// Per-code base upper bound from the length cap.
    max_base: [u64; 29],
    /// The equation's right-hand side.
    rhs: u64,
    /// `Σ count[k]·min_base[k]` over the not-yet-pinned codes.
    min_remaining: u64,
    /// `Σ count[k]·max_base[k]` over the not-yet-pinned codes.
    max_remaining: u64,
}

impl SumGuard {
    /// Walk the fixed parse once and build the accounting. `Ok(None)`
    /// when the walk ends before the target's needs are knowable (the
    /// guard simply stays inert); `Err` when the equation is violated
    /// before any pin (globally unsolvable).
    fn new(
        solver: &Solver,
        reader: &BitReader,
        limits: &SolveLimits,
    ) -> Result<Option<SumGuard>, SolveError> {
        let mut count = [0u64; 29];
        let mut ext_sum = 0u64;
        let mut literals = 0u64;
        // Per-code ext VALUE bounds (the bases' feasibility is relative
        // to the values read, not the widths).
        let mut max_ext_v = [0u16; 29];
        let mut min_ext_v = [u16::MAX; 29];
        let mut p = solver.start_bit;
        let mut steps = 0usize;
        while (p >> 3) < solver.stream.len() {
            steps += 1;
            if steps > 4_000_000 {
                return Ok(None); // walk cap: stay inert rather than guess
            }
            let (sym, clen) = match reader.peek_symbol_state(&solver.hdr.lit_h, p) {
                PeekSym::Hit(s, l) => (s, l),
                _ => break, // the parse ends here (exhaustion or bad code)
            };
            match sym {
                0..=255 => {
                    literals += 1;
                    p += clen as usize;
                }
                256 => p += clen as usize,
                _ => {
                    let li = (sym - 257) as usize;
                    if li >= 29 {
                        break;
                    }
                    let w_l = solver.baseline.lextra[li];
                    let p1 = p + clen as usize;
                    let end = solver.stream.len() * 8;
                    if p1 + w_l as usize > end {
                        break; // the clean stop
                    }
                    let ext_l = reader.peek(p1, w_l) as u16;
                    let p2 = p1 + w_l as usize;
                    let (dsym, dcl) = match reader.peek_symbol_state(&solver.hdr.dist_h, p2) {
                        PeekSym::Hit(s, l) => (s, l),
                        _ => break,
                    };
                    if dsym as usize >= 30 {
                        break;
                    }
                    let w_d = solver.baseline.dextra[dsym as usize];
                    let p3 = p2 + dcl as usize;
                    if p3 + w_d as usize > end {
                        break;
                    }
                    // The match completes (all its reads are within input):
                    // exactly the engine's event accounting.
                    count[li] += 1;
                    ext_sum += ext_l as u64;
                    max_ext_v[li] = max_ext_v[li].max(ext_l);
                    min_ext_v[li] = min_ext_v[li].min(ext_l);
                    p = p3 + w_d as usize;
                }
            }
        }
        if count.iter().all(|&c| c == 0) {
            return Ok(None);
        }
        let rhs = solver.target.len() as i64 - literals as i64 - ext_sum as i64;
        if rhs < 0 {
            return Err(SolveError::NoSolution);
        }
        let mut min_base = [0u64; 29];
        let mut max_base = [0u64; 29];
        for k in 0..29 {
            if count[k] == 0 {
                continue;
            }
            min_base[k] = (limits.min_len as i64 - min_ext_v[k] as i64).max(0) as u64;
            max_base[k] =
                ((limits.max_len as i64 - max_ext_v[k] as i64).max(0) as u64).min(u16::MAX as u64);
        }
        let mut g = SumGuard {
            count,
            min_base,
            max_base,
            rhs: rhs as u64,
            min_remaining: 0,
            max_remaining: 0,
        };
        g.min_remaining = (0..29)
            .filter(|&k| count[k] > 0)
            .map(|k| count[k] * g.min_base[k])
            .sum();
        g.max_remaining = (0..29)
            .filter(|&k| count[k] > 0)
            .map(|k| count[k] * g.max_base[k])
            .sum();
        // The global feasibility check before any pin.
        if g.min_remaining > g.rhs || g.max_remaining < g.rhs {
            return Err(SolveError::NoSolution);
        }
        Ok(Some(g))
    }

    /// Whether pinning `base` for code `k` keeps the length-sum equation
    /// satisfiable. `committed`/`min_remaining`/`max_remaining` are the
    /// caller's running totals EXCLUDING code `k` (still unpinned).
    fn allows(
        &self,
        committed: u64,
        k: usize,
        base: u64,
        min_remaining: u64,
        max_remaining: u64,
    ) -> bool {
        let c = self.count[k];
        let sum = committed + c * base;
        sum + min_remaining <= self.rhs && sum + max_remaining >= self.rhs
    }
}

/// The search context (parsed header + limits + inputs + the length-sum
/// guard).
struct Solver<'a> {
    stream: &'a [u8],
    target: &'a [u8],
    preset_len: usize,
    start_bit: usize,
    limits: SolveLimits,
    baseline: &'a ValueTables,
    hdr: crate::inflate::DynHeader,
    guard: Option<SumGuard>,
}

/// Solve the engine ladder that reproduces `target` from `stream`.
///
/// `target` is the decode's own output region (NOT the machine's
/// post-fill pixel window — the band fill is machine semantics, §17/§26.4;
/// the caller routes a non-zero band through the P1 tree first).
/// `baseline` supplies the RFC frame (extra-bit widths, which the solve
/// holds fixed per §30.2) and the values for codes the stream never
/// constrains.
pub fn solve_ladder(
    stream: &[u8],
    start_bit: usize,
    preset_len: usize,
    target: &[u8],
    baseline: &ValueTables,
    limits: SolveLimits,
) -> Result<LadderSolution, SolveError> {
    if target.is_empty() {
        return Err(SolveError::EmptyTarget);
    }
    let hdr = parse_dynamic_header(stream).map_err(SolveError::Stream)?;
    let s = Solver {
        stream,
        target,
        preset_len,
        start_bit,
        limits,
        baseline,
        hdr,
        guard: None,
    };
    s.run()
}

impl<'a> Solver<'a> {
    fn run(mut self) -> Result<LadderSolution, SolveError> {
        let reader = BitReader::new(self.stream);
        // The length-sum guard: exact stream accounting (the parse is
        // base-independent), giving every base pin a global feasibility
        // check.
        self.guard = SumGuard::new(&self, &reader, &self.limits)?;
        let mut sets = Sets::new();
        if let Some(g) = &self.guard {
            sets.min_remaining = g.min_remaining;
            sets.max_remaining = g.max_remaining;
        }
        let mut stack: Vec<Frame> = Vec::new();
        let mut p = self.start_bit;
        let mut j = 0usize;
        let mut nodes: u64 = 0;

        loop {
            match self.advance(&mut sets, &reader, &mut p, &mut j, &mut nodes, &mut stack)? {
                Flow::Success => break,
                Flow::DeadEnd => {
                    if !self.rewind(&mut sets, &reader, &mut p, &mut j, &mut nodes, &mut stack)? {
                        return Err(SolveError::NoSolution);
                    }
                }
            }
        }

        self.build(&sets, nodes)
    }

    /// Process steps along the committed path until the target is
    /// explained, a path is refuted, or the budget is spent (typed Err).
    fn advance(
        &self,
        sets: &mut Sets,
        reader: &BitReader,
        p: &mut usize,
        j: &mut usize,
        nodes: &mut u64,
        stack: &mut Vec<Frame>,
    ) -> Result<Flow, SolveError> {
        loop {
            // Loop-top input exhaustion — the engine's own stop condition.
            // A stop before the target ends refutes the path (the same
            // tables would stop there again).
            if (*p >> 3) >= self.stream.len() {
                return Ok(if *j == self.target.len() {
                    Flow::Success
                } else {
                    Flow::DeadEnd
                });
            }
            let (sym, clen) = match reader.peek_symbol_state(&self.hdr.lit_h, *p) {
                PeekSym::Hit(s, l) => (s, l),
                PeekSym::Exhausted => {
                    return Ok(if *j == self.target.len() {
                        Flow::Success
                    } else {
                        Flow::DeadEnd
                    });
                }
                // A literal/length code that scans complete input without
                // resolving is base-independent (the bit-level parse is
                // fixed by the widths): no assignment can pass here.
                PeekSym::BadCode => return Err(SolveError::NoSolution),
            };
            match sym {
                0..=255 => {
                    if *j >= self.target.len() || self.target[*j] != sym as u8 {
                        return Ok(Flow::DeadEnd);
                    }
                    *p += clen as usize;
                    *j += 1;
                }
                // End-of-block marker: no output, the decode continues.
                256 => {
                    *p += clen as usize;
                }
                _ => {
                    let li = (sym - 257) as usize;
                    if li >= 29 {
                        return Ok(Flow::DeadEnd);
                    }
                    let w_l = self.baseline.lextra[li];
                    let p1 = *p + clen as usize;
                    let end = self.stream.len() * 8;
                    if p1 + w_l as usize > end {
                        // With fixed widths the boundary is stream data:
                        // terminating here with output remaining refutes
                        // the path; with the target explained it IS the
                        // clean stop (no bases are observable).
                        return Ok(if *j == self.target.len() {
                            Flow::Success
                        } else {
                            Flow::DeadEnd
                        });
                    }
                    let ext_l = reader.peek(p1, w_l) as u16;
                    if let Some(bl) = sets.lbase[li] {
                        let l = bl as u32 + ext_l as u32;
                        if l < self.limits.min_len as u32
                            || l > self.limits.max_len as u32
                            || l > (*j + self.target.len() - *j) as u32
                        {
                            return Ok(Flow::DeadEnd);
                        }
                    }
                    let p2 = p1 + w_l as usize;
                    let (dsym, dcl) = match reader.peek_symbol_state(&self.hdr.dist_h, p2) {
                        PeekSym::Hit(s, l) => (s, l),
                        PeekSym::Exhausted => {
                            return Ok(if *j == self.target.len() {
                                Flow::Success
                            } else {
                                Flow::DeadEnd
                            });
                        }
                        PeekSym::BadCode => return Err(SolveError::NoSolution),
                    };
                    let di = dsym as usize;
                    if di >= 30 {
                        return Err(SolveError::NoSolution);
                    }
                    let w_d = self.baseline.dextra[di];
                    let p3 = p2 + dcl as usize;
                    if p3 + w_d as usize > end {
                        return Ok(if *j == self.target.len() {
                            Flow::Success
                        } else {
                            Flow::DeadEnd
                        });
                    }
                    if *j == self.target.len() {
                        // The step completes: a match produces output the
                        // target does not contain.
                        return Ok(Flow::DeadEnd);
                    }
                    let ext_d = reader.peek(p3, w_d) as u16;
                    if let Some(bd) = sets.dbase[di] {
                        let d = bd as u32 + ext_d as u32;
                        if d < 1 || d > self.d_window(*j) {
                            return Ok(Flow::DeadEnd);
                        }
                    }

                    let mut frame = Frame {
                        p: *p,
                        j: *j,
                        li,
                        clen,
                        w_l,
                        dsym,
                        dcl,
                        w_d,
                        r: 0,
                        k: 0,
                        sub: 0,
                        m_cache: None,
                        saved: sets.clone(),
                    };
                    let Some(first) = self.next_candidate(&mut frame, sets, reader) else {
                        return Ok(Flow::DeadEnd);
                    };
                    let had_l = sets.lbase[li].is_some();
                    let had_d = sets.dbase[di].is_some();
                    stack.push(frame);
                    let _ = had_l;
                    let _ = had_d;
                    match self.apply(first, sets, reader, p, j, nodes, li)? {
                        // The frame STAYS on the stack: its remaining
                        // spiral candidates are the rewind's next tries.
                        Apply::Advanced => {}
                        Apply::Rejected(()) => return Ok(Flow::DeadEnd),
                    }
                }
            }
        }
    }

    /// Backtrack to the nearest branching frame, restore its state and
    /// apply its next candidate. The output position couples every
    /// earlier length into each check, so conflict-directed backjumping
    /// has no sound blame here; the sound global pruning lives in the
    /// length-sum constraint (see `SumGuard`), and the rewind is the
    /// plain innermost-frame order, which is complete. `false` = the
    /// space is exhausted (typed NoSolution). Err = budget.
    #[allow(clippy::too_many_arguments)]
    fn rewind(
        &self,
        sets: &mut Sets,
        reader: &BitReader,
        p: &mut usize,
        j: &mut usize,
        nodes: &mut u64,
        stack: &mut Vec<Frame>,
    ) -> Result<bool, SolveError> {
        while let Some(mut f) = stack.pop() {
            sets.clone_from(&f.saved);
            *p = f.p;
            *j = f.j;
            while let Some(commit) = self.next_candidate(&mut f, sets, reader) {
                match self.apply(commit, sets, reader, p, j, nodes, f.li)? {
                    Apply::Advanced => {
                        stack.push(f);
                        return Ok(true);
                    }
                    Apply::Rejected(()) => {} // the frame retries its next candidate
                }
            }
        }
        Ok(false)
    }

    /// The distance window at output position `j`: the engine window
    /// (preset included), in u32 to avoid truncation near 64 KiB.
    fn d_window(&self, j: usize) -> u32 {
        (self.limits.max_dist as u32).min((j + self.preset_len) as u32)
    }

    /// The next candidate commit for a match frame: the baseline (RFC)
    /// values first, then the family ordered by Manhattan distance from
    /// that point. A pinned side holds its value (the base is committed);
    /// the free side sweeps. Deterministic; `None` when exhausted.
    fn next_candidate(&self, f: &mut Frame, sets: &Sets, reader: &BitReader) -> Option<Commit> {
        let lim = &self.limits;
        let p1 = f.p + f.clen as usize;
        let p2 = p1 + f.w_l as usize;
        let p3 = p2 + f.dcl as usize;
        let ext_l = reader.peek(p1, f.w_l) as u16;
        let ext_d = reader.peek(p3, f.w_d) as u16;
        let d_window = self.d_window(f.j);
        let l_cap = (lim.max_len as u32).min((self.target.len() - f.j) as u32);
        let scan = d_window.min(lim.dscan_cap);
        let r_max = scan.min(lim.spiral_radius_cap);

        let di = f.dsym as usize;
        let l_pinned = sets.lbase[f.li].is_some();
        let d_pinned = sets.dbase[di].is_some();
        let bl = sets.lbase[f.li].unwrap_or(self.baseline.lbase[f.li]);
        let bd = sets.dbase[di].unwrap_or(self.baseline.dbase[di]);
        let l0 = bl as u32 + ext_l as u32;
        let d0 = bd as u32 + ext_d as u32;
        let l_floor = (lim.min_len as u32).max(ext_l as u32);

        // Stage 0: the baseline candidate (the null hypothesis).
        if f.r == 0 && f.k == 0 && f.sub == 0 {
            f.r = 1;
            if d0 >= 1
                && d0 <= d_window
                && d0 >= ext_d as u32
                && l0 >= l_floor
                && l0 <= l_cap
                && l0 <= u16::MAX as u32
                && d0 <= u16::MAX as u32
            {
                return Some(Commit {
                    d: d0 as u16,
                    l: l0 as u16,
                });
            }
        }

        // Stage 1: the spiral family, ordered by Manhattan distance r
        // from the baseline point (d0, l0). Per radius: dd from -r to +r,
        // d = d0 + dd, dl = r - |dd|; candidates (d, l0 - dl) then
        // (d, l0 + dl). A pinned coordinate holds its value (moving
        // candidates are skipped), so the spiral degenerates to the free
        // axis. The baseline point itself is skipped (tried above).
        // With the §40.3 class prior the length axis flips for the long
        // classes (above first); short codes keep below-first. Ordering
        // only — the predicate below is untouched.
        let above_first = self.limits.class_prior && length_class(f.li) > 0;
        while f.r <= r_max {
            let r = f.r;
            let dd = f.k as i64 - r as i64;
            let d = (d0 as i64 + dd) as u32;
            let dl = r - dd.unsigned_abs() as u32;
            while f.sub <= 1 {
                let below = f.sub == if above_first { 1 } else { 0 };
                let l = if below {
                    l0.saturating_sub(dl)
                } else {
                    l0 + dl
                };
                f.sub += 1;
                let is_baseline = d == d0 && l == l0;
                if is_baseline
                    || (l_pinned && l != l0)
                    || (d_pinned && d != d0)
                    || d < 1
                    || d > d_window
                    || d < ext_d as u32
                    || d > u16::MAX as u32
                    || l < l_floor
                    || l > l_cap
                    || l > u16::MAX as u32
                {
                    continue;
                }
                let m = if d_pinned {
                    *f.m_cache.get_or_insert_with(|| {
                        run_len(self.target, f.j, d0 as u16, l_cap as usize) as u32
                    })
                } else {
                    run_len(self.target, f.j, d as u16, l_cap as usize) as u32
                };
                if l > m {
                    continue;
                }
                return Some(Commit {
                    d: d as u16,
                    l: l as u16,
                });
            }
            f.sub = 0;
            f.k += 1;
            if f.k > 2 * r {
                f.r += 1;
                f.k = 0;
            }
        }
        None
    }

    /// Apply one candidate commit for length code `li`: check it against
    /// the target content, pin the unseen bases, advance. Checks run
    /// BEFORE any mutation: a failed apply leaves the sets untouched.
    #[allow(clippy::too_many_arguments)]
    fn apply(
        &self,
        commit: Commit,
        sets: &mut Sets,
        reader: &BitReader,
        p: &mut usize,
        j: &mut usize,
        nodes: &mut u64,
        li: usize,
    ) -> Result<Apply, SolveError> {
        let lim = &self.limits;
        *nodes += 1;
        if *nodes > lim.node_budget {
            return Err(SolveError::BudgetExhausted { nodes: *nodes });
        }
        let clen = match reader.peek_symbol(&self.hdr.lit_h, *p) {
            Some((_, l)) => l,
            None => return Ok(Apply::Rejected(())),
        };
        let p1 = *p + clen as usize;
        let w_l = self.baseline.lextra[li];
        let ext_l = reader.peek(p1, w_l) as u16;
        let p2 = p1 + w_l as usize;
        let (dsym, dcl) = match reader.peek_symbol_state(&self.hdr.dist_h, p2) {
            PeekSym::Hit(s, l) => (s, l),
            _ => return Ok(Apply::Rejected(())),
        };
        let di = dsym as usize;
        if di >= 30 {
            return Ok(Apply::Rejected(()));
        }
        let w_d = self.baseline.dextra[di];
        let p3 = p2 + dcl as usize;
        let ext_d = reader.peek(p3, w_d) as u16;

        // Effective bases: committed pins, or the candidate's implied base.
        let (bl, bd) = match (sets.lbase[li], sets.dbase[di]) {
            (Some(bl), Some(bd)) => (bl, bd),
            (Some(bl), None) => {
                let Some(bd) = commit.d.checked_sub(ext_d) else {
                    return Ok(Apply::Rejected(()));
                };
                (bl, bd)
            }
            (None, Some(bd)) => {
                let Some(bl) = commit.l.checked_sub(ext_l) else {
                    return Ok(Apply::Rejected(()));
                };
                (bl, bd)
            }
            (None, None) => {
                let Some(bl) = commit.l.checked_sub(ext_l) else {
                    return Ok(Apply::Rejected(()));
                };
                let Some(bd) = commit.d.checked_sub(ext_d) else {
                    return Ok(Apply::Rejected(()));
                };
                (bl, bd)
            }
        };

        // All checks before any mutation. A failing check blames the
        // pre-existing pin on that side (a CDB conflict); a free side's
        // failure is this frame's own candidate and is retried locally.
        let l = bl as u32 + ext_l as u32;
        let d = bd as u32 + ext_d as u32;
        let l_ok = l == commit.l as u32
            && l >= lim.min_len as u32
            && l <= (lim.max_len as u32).min((self.target.len() - *j) as u32);
        let d_ok = d == commit.d as u32 && d >= 1 && d <= self.d_window(*j);
        if !l_ok || !d_ok {
            return Ok(Apply::Rejected(()));
        }
        if run_len(self.target, *j, commit.d, commit.l as usize) != commit.l as usize {
            return Ok(Apply::Rejected(()));
        }

        // Commit the unseen pins. The length side passes through the
        // sum guard first (the decode's own output length is a linear
        // equation in the bases — a sound global check).
        if sets.lbase[li].is_none() {
            if let Some(g) = &self.guard {
                let min_r = sets.min_remaining - g.count[li] * g.min_base[li];
                let max_r = sets.max_remaining - g.count[li] * g.max_base[li];
                if !g.allows(sets.committed, li, bl as u64, min_r, max_r) {
                    return Ok(Apply::Rejected(()));
                }
                sets.committed += g.count[li] * bl as u64;
                sets.min_remaining = min_r;
                sets.max_remaining = max_r;
            }
            sets.lbase[li] = Some(bl);
        }
        if sets.dbase[di].is_none() {
            sets.dbase[di] = Some(bd);
        }

        // The step ends after the dist extra bits (the engine consumes
        // code + len-extra + dist code + dist-extra; p3 is where the
        // dist extra bits start).
        *p = p3 + w_d as usize;
        *j += commit.l as usize;
        Ok(Apply::Advanced)
    }

    /// Build the ladder and run the verification re-decode. The reported
    /// event/match counts come from that re-decode (the final path only).
    fn build(&self, sets: &Sets, nodes: u64) -> Result<LadderSolution, SolveError> {
        let mut tables = self.baseline.clone();
        let mut len_status = [CodeStatus::Unconstrained; 29];
        let mut dist_status = [CodeStatus::Unconstrained; 30];
        for (i, slot) in sets.lbase.iter().enumerate() {
            if let Some(b) = slot {
                tables.lbase[i] = *b;
                len_status[i] = CodeStatus::Pinned;
            }
        }
        for (i, slot) in sets.dbase.iter().enumerate() {
            if let Some(b) = slot {
                tables.dbase[i] = *b;
                dist_status[i] = CodeStatus::Pinned;
            }
        }

        // The verdict is the engine's own re-decode — one code path with
        // every boot pin; no surrogate oracle.
        let mut redecoded = Vec::new();
        let mut events = Vec::new();
        let verified = match inflate_engine_with_traced(
            self.stream,
            self.start_bit,
            self.preset_len,
            &mut redecoded,
            self.target.len(),
            &tables,
            Some(&mut events),
        ) {
            Ok(_) => redecoded[..] == self.target[..],
            Err(_) => false,
        };
        let matches = events
            .iter()
            .filter(|e| matches!(e.kind, crate::inflate::DecodeEventKind::Match { .. }))
            .count();

        Ok(LadderSolution {
            tables,
            len_status,
            dist_status,
            events: events.len(),
            matches,
            nodes,
            redecoded,
            verified,
        })
    }
}
