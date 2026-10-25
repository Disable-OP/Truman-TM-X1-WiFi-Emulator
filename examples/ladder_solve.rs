//! The capture→ladder solver as one command (Milestone AJ, §37): the
//! hardware session's last manual step — turning a P1 capture into the
//! recovered `SPHEVTBL` value-table file — is now mechanical.
//!
//! Boots the real firmware, extracts the engine decode inputs and the
//! pixel-window reference (the `dram_compare --post` semantics), compares
//! against the capture, and on divergence runs the inversion: the ladder
//! whose engine re-decode reproduces the capture's decode region is
//! recovered and (when verified) written as a `SPHEVTBL` container
//! accepted by `--value-tables` on `dram_compare --post`, `map` and
//! `ladder_eval`.
//!
//! The route follows the pre-registered P1 tree (docs/hardware-session.md):
//!
//! * identical window → the P1 IDENTICAL row (suspect the capture point);
//!   no solve is run;
//! * non-zero capture bytes at or beyond the decode's own output length →
//!   the §26.4 band/length-Δ family; the solver needs the decode region
//!   alone — route through the tree first (or pass `--target-len` after
//!   establishing the true length);
//! * divergence inside the decode region → the solve. The solve's verdict
//!   is the engine's own re-decode under the recovered tables
//!   (`verified`); an unverified solve is reported, never written.
//!
//! Usage:
//!   ladder_solve <flashfile.bin> <capture.bin>
//!       [--value-tables <baseline.spvt>] [--offset <phys>] [--len <bytes>]
//!       [--out <solved.spvt>] [--target-len <bytes>] [--max-nodes <n>]
//!
//! Exit codes: 0 = identical window (P1 IDENTICAL) or solved AND
//! engine-verified; 1 = divergent but not solved (report only — the
//! attribution is the actionable data; see `solve_divergence`); 2 =
//! usage, IO or stream error.

use std::path::PathBuf;

use truman_emulator::firmware;
use truman_emulator::inflate::ValueTables;
use truman_emulator::machine::{
    adler32, compare_dumps, Machine, MachineConfig, ENGINE_PRESET_BYTES,
};
use truman_emulator::memory::AccessSize;
use truman_emulator::solve::{solve_ladder, CodeStatus, SolveLimits};

const DEFAULT_PIXEL_BASE: u32 = 0x0160_0000;
const ENGINE_IMG_WIDTH_REG: u32 = 0x1FFE_3828;
const ENGINE_IMG_HEIGHT_REG: u32 = 0x1FFE_382C;

fn usage() -> ! {
    eprintln!(
        "usage:\n  \
         ladder_solve <flashfile.bin> <capture.bin>\n\
         options:\n  \
         --value-tables <f>  baseline ladder from a file (default RFC)\n  \
         --offset <phys>     pixel base override (default 0x01600000)\n  \
         --len <bytes>       window length override (default W*H)\n  \
         --out <file>        write the recovered SPHEVTBL container (verified solves only)\n  \
         --target-len <n>    decode-region length override (default: the boot's own)\n  \
         --max-nodes <n>     candidate-attempt budget (default 2000000)\n\
         exit codes: 0 identical/verified solve, 1 divergent-unsolved (report), 2 error"
    );
    std::process::exit(2);
}

struct Args {
    flash: PathBuf,
    capture: PathBuf,
    value_tables: Option<PathBuf>,
    offset: u32,
    len_override: Option<usize>,
    out: Option<PathBuf>,
    target_len: Option<usize>,
    max_nodes: u64,
}

fn parse_args() -> Args {
    let mut positional: Vec<String> = Vec::new();
    let mut value_tables = None;
    let mut offset = DEFAULT_PIXEL_BASE;
    let mut len_override = None;
    let mut out = None;
    let mut target_len = None;
    let mut max_nodes = 2_000_000;
    let a: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < a.len() {
        match a[i].as_str() {
            "--value-tables" => {
                value_tables = Some(PathBuf::from(a.get(i + 1).unwrap_or_else(|| usage())));
                i += 1;
            }
            "--offset" => {
                offset = u32::from_str_radix(
                    a.get(i + 1)
                        .unwrap_or_else(|| usage())
                        .trim_start_matches("0x"),
                    16,
                )
                .unwrap_or_else(|_| usage());
                i += 1;
            }
            "--len" => {
                len_override = Some(
                    a.get(i + 1)
                        .unwrap_or_else(|| usage())
                        .parse::<usize>()
                        .unwrap_or_else(|_| usage()),
                );
                i += 1;
            }
            "--out" => {
                out = Some(PathBuf::from(a.get(i + 1).unwrap_or_else(|| usage())));
                i += 1;
            }
            "--target-len" => {
                target_len = Some(
                    a.get(i + 1)
                        .unwrap_or_else(|| usage())
                        .parse::<usize>()
                        .unwrap_or_else(|_| usage()),
                );
                i += 1;
            }
            "--max-nodes" => {
                max_nodes = a
                    .get(i + 1)
                    .unwrap_or_else(|| usage())
                    .parse::<u64>()
                    .unwrap_or_else(|_| usage());
                i += 1;
            }
            other => positional.push(other.to_string()),
        }
        i += 1;
    }
    if positional.len() != 2 {
        usage();
    }
    Args {
        flash: PathBuf::from(&positional[0]),
        capture: PathBuf::from(&positional[1]),
        value_tables,
        offset,
        len_override,
        out,
        target_len,
        max_nodes,
    }
}

fn main() {
    let args = parse_args();

    let img = firmware::load(&args.flash).unwrap_or_else(|e| {
        eprintln!("error: cannot load firmware: {e}");
        std::process::exit(2);
    });
    let capture = std::fs::read(&args.capture).unwrap_or_else(|e| {
        eprintln!("error: cannot read capture {}: {e}", args.capture.display());
        std::process::exit(2);
    });

    let baseline = match &args.value_tables {
        None => ValueTables::rfc1951(),
        Some(p) => {
            let raw = std::fs::read(p).unwrap_or_else(|e| {
                eprintln!("error: cannot read {}: {e}", p.display());
                std::process::exit(2);
            });
            ValueTables::from_bytes(&raw).unwrap_or_else(|e| {
                eprintln!("error: value tables {}: {e}", p.display());
                std::process::exit(2);
            })
        }
    };

    let mut m = Machine::new(MachineConfig::default(), &img);
    m.reset();
    let res = m.run(2_000_000);
    println!(
        "boot: stop={:?} steps={} pc={:08x}",
        res.stop, res.steps, res.pc
    );

    let inputs = m.engine_decode_inputs().unwrap_or_else(|| {
        eprintln!("error: the engine never ran this boot — no decode to solve against");
        std::process::exit(2);
    });

    let w = m
        .bus
        .peek_phys(ENGINE_IMG_WIDTH_REG, AccessSize::Word)
        .unwrap_or(0x400);
    let h = m
        .bus
        .peek_phys(ENGINE_IMG_HEIGHT_REG, AccessSize::Word)
        .unwrap_or(78);
    let total = match args.len_override {
        Some(n) => n,
        None => (w as usize) * (h as usize),
    };
    let mut window = vec![0u8; total];
    let got = m.bus.copy_out(args.offset, &mut window);
    if got != total {
        eprintln!("error: window short read ({got}/{total})");
        std::process::exit(2);
    }
    let decoded_len = inputs.decoded_len;
    println!(
        "window: phys 0x{:08x}, {} bytes (engine W×H = {}×{}); boot decode {} bytes, \
         adler-32 0x{:08x}",
        args.offset,
        total,
        w,
        h,
        decoded_len,
        adler32(&window)
    );

    // The P1 comparison (the dram_compare --post semantics).
    let cmp = compare_dumps(&window, &capture);
    println!(
        "compare: compared={} matching={} ({}‰) first_divergence={:?}",
        cmp.compared_len, cmp.matching_bytes, cmp.match_permille, cmp.first_divergence
    );

    if cmp.first_divergence.is_none()
        && cmp.compared_len == cmp.reference_len
        && cmp.compared_len == cmp.captured_len
    {
        println!(
            "verdict: P1 IDENTICAL — the emulator's decode equals the capture. Per the \
             pre-registered tree: suspect the capture point FIRST (re-capture at the \
             `jr v0` handoff, kseg1 view) before revisiting the engine model. No solve run."
        );
        std::process::exit(0);
    }

    // The decode region: the capture bytes the solve must reproduce. The
    // band fill above the boot decode's own length is machine semantics
    // (§17); non-zero content there is the §26.4 length-Δ family.
    let target_len = args.target_len.unwrap_or(decoded_len);
    if args.target_len.is_none() && capture.len() > decoded_len {
        let band_nonzero = capture[decoded_len..].iter().any(|&b| b != 0);
        if band_nonzero {
            println!(
                "verdict: the capture has non-zero bytes at or beyond the boot decode's \
                 own length ({decoded_len}) — the zero-filled band region (§17) is not \
                 fill on the device: the §26.4 length-accounting family. Establish the \
                 true decode length first and re-run with --target-len."
            );
            std::process::exit(1);
        }
    }
    if target_len > capture.len() {
        eprintln!(
            "error: target length {target_len} exceeds the capture ({})",
            capture.len()
        );
        std::process::exit(2);
    }
    let target = &capture[..target_len];
    println!(
        "solve: target {} bytes (boot decode region), baseline {}",
        target.len(),
        if baseline == ValueTables::rfc1951() {
            "RFC ladder"
        } else {
            "from file"
        }
    );

    let limits = SolveLimits {
        node_budget: args.max_nodes,
        ..SolveLimits::default()
    };
    let sol = match solve_ladder(
        &inputs.stream,
        inputs.start_bit as usize,
        ENGINE_PRESET_BYTES,
        target,
        &baseline,
        limits,
    ) {
        Ok(sol) => sol,
        Err(e) => {
            println!("solve: FAILED — {e}");
            println!(
                "verdict: divergent, unsolved. The attribution (solve_divergence) is the \
                 actionable data; the caps may also need widening for this capture."
            );
            std::process::exit(1);
        }
    };

    let pinned = |st: &[CodeStatus]| st.iter().filter(|s| **s == CodeStatus::Pinned).count();
    let lp = pinned(&sol.len_status);
    let dp = pinned(&sol.dist_status);
    let lu = 29 - lp;
    let du = 30 - dp;

    println!(
        "solved: verified={} events={} matches={} attempts={} \
         [len pinned {lp}/29 (unconstrained {lu}); dist pinned {dp}/30 (unconstrained {du})]",
        sol.verified, sol.events, sol.matches, sol.nodes
    );

    if !sol.verified {
        println!(
            "verdict: divergent, unsolved — the search's best assignment does not \
             re-decode to the capture (re-decode {} bytes vs target {}). The capture may \
             combine several effects (§26.4 length accounting + value semantics); \
             route through the P1 tree.",
            sol.redecoded.len(),
            target.len()
        );
        std::process::exit(1);
    }

    println!(
        "verification: the engine's own re-decode under the recovered ladder reproduces \
         the capture byte-for-byte (adler-32 0x{:08x}); filled-window fingerprint 0x{:08x}",
        adler32(&sol.redecoded),
        adler32(&window)
    );

    match &args.out {
        Some(p) => {
            let enc = sol.tables.to_bytes();
            std::fs::write(p, &enc).unwrap_or_else(|e| {
                eprintln!("error: cannot write {}: {e}", p.display());
                std::process::exit(2);
            });
            println!(
                "wrote {} ({} bytes, SPHEVTBL) — verify with: \
                 dram_compare --post <flash> <capture> --value-tables {}",
                p.display(),
                enc.len(),
                p.display()
            );
            println!("verdict: solved. The post-capture swap is data; re-run the boot pins.");
            std::process::exit(0);
        }
        None => {
            println!("verdict: solved (no --out given; pass one to write the SPHEVTBL container).");
            std::process::exit(0);
        }
    }
}
