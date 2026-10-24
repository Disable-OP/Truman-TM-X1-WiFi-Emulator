//! Hardware-session ingestion (Milestone Y, docs/hardware-session.md):
//! the §22.5 handoff as one command. Ingests a real device's DRAM capture
//! and runs the pre-registered decision tree against the emulator.
//!
//! Experiments (docs/firmware-analysis.md §22.5, §26.4):
//! - `--post` (P1): boot the real firmware in the emulator, take the
//!   engine's pixel-buffer reference (phys 0x01600000, W×H from the
//!   engine's own registers) and compare it byte-for-byte with the
//!   device capture. Divergence here is the decisive constraint for the
//!   LZ value-semantics solve (§14/§26-bounded space).
//! - `--pre` (P2): classify a pre-decode capture against the all-zero
//!   expectation (§18.3 proves the emulator boot never writes below the
//!   pixel base). Non-zero residue = the §21.6 item-2 answer.
//! - `--ref`: file-vs-file compare (e.g. a saved `pixel_dump` output vs
//!   a capture), no boot needed.
//!
//! Usage:
//!   dram_compare --post <flashfile.bin> <capture.bin>
//!   dram_compare --pre <capture.bin>
//!   dram_compare --ref <reference.bin> <capture.bin>
//!
//! Post/Ref reports also print the reference's Adler-32 — the
//! emulator-side fingerprint for the P3 probe-window read (§34.2).
//! Options:
//!   --out <map.txt>   write the 1024-byte block map to a file
//!   --offset <phys>   post mode: pixel base override (default 0x01600000)
//!   --len <bytes>     post mode: reference length override (default W*H)
//!   --value-tables <f> post mode: engine value ladder from a file (§35,
//!                     the post-capture swap as data, no recompile)
//!
//! Exit codes: 0 = identical / zero residue; 1 = divergent / non-zero
//! residue (actionable data); 2 = usage, IO or empty-overlap error.

use std::path::PathBuf;

use truman_emulator::firmware;
use truman_emulator::machine::{adler32, classify_residue, compare_dumps, Machine, MachineConfig};
use truman_emulator::memory::AccessSize;

const DEFAULT_PIXEL_BASE: u32 = 0x0160_0000;
const ENGINE_IMG_WIDTH_REG: u32 = 0x1FFE_3828;
const ENGINE_IMG_HEIGHT_REG: u32 = 0x1FFE_382C;

fn usage() -> ! {
    eprintln!(
        "usage:\n  \
         dram_compare --post <flashfile.bin> <capture.bin>\n  \
         dram_compare --pre <capture.bin>\n  \
         dram_compare --ref <reference.bin> <capture.bin>\n\
         options:\n  \
         --out <map.txt>   write the 1024-byte block map\n  \
         --offset <phys>   post mode: pixel base override (default 0x01600000)\n  \
         --len <bytes>     post mode: reference length override (default W*H)\n  \
         --value-tables <f> post mode: engine value ladder from a file (§35)\n\
         exit codes: 0 identical/zero residue, 1 divergent/non-zero, 2 error"
    );
    std::process::exit(2);
}

struct Args {
    mode: Mode,
    flash: Option<PathBuf>,
    reference_file: Option<PathBuf>,
    capture: PathBuf,
    out: Option<PathBuf>,
    offset: u32,
    len_override: Option<usize>,
    /// The `--value-tables` file, when given (§35). Post mode only: Pre
    /// and Ref do not run the engine decode, so a value there is a usage
    /// error (rejected in parse_args).
    value_tables: Option<PathBuf>,
}

enum Mode {
    Post,
    Pre,
    Ref,
}

fn parse_args() -> Args {
    let mut mode: Option<Mode> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut out = None;
    let mut offset = DEFAULT_PIXEL_BASE;
    let mut len_override = None;
    let mut value_tables: Option<PathBuf> = None;
    let a: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < a.len() {
        match a[i].as_str() {
            "--post" => mode = Some(Mode::Post),
            "--pre" => mode = Some(Mode::Pre),
            "--ref" => mode = Some(Mode::Ref),
            "--out" => {
                i += 1;
                out = a.get(i).map(PathBuf::from);
            }
            "--offset" => {
                i += 1;
                offset = u32::from_str_radix(
                    a.get(i).unwrap_or_else(|| usage()).trim_start_matches("0x"),
                    16,
                )
                .unwrap_or_else(|_| usage());
            }
            "--len" => {
                i += 1;
                len_override = Some(
                    a.get(i)
                        .unwrap_or_else(|| usage())
                        .parse::<usize>()
                        .unwrap_or_else(|_| usage()),
                );
            }
            "--value-tables" => {
                i += 1;
                value_tables = a.get(i).map(PathBuf::from);
                if value_tables.is_none() {
                    usage();
                }
            }
            other => positional.push(other.to_string()),
        }
        i += 1;
    }
    let (flash, reference_file, capture) = match mode {
        Some(Mode::Post) => {
            if positional.len() != 2 {
                usage();
            }
            (
                Some(PathBuf::from(&positional[0])),
                None,
                PathBuf::from(&positional[1]),
            )
        }
        Some(Mode::Pre) => {
            if positional.len() != 1 {
                usage();
            }
            (None, None, PathBuf::from(&positional[0]))
        }
        Some(Mode::Ref) => {
            if positional.len() != 2 {
                usage();
            }
            (
                None,
                Some(PathBuf::from(&positional[0])),
                PathBuf::from(&positional[1]),
            )
        }
        None => usage(),
    };
    if value_tables.is_some() && !matches!(mode, Some(Mode::Post)) {
        eprintln!("error: --value-tables applies to --post only (pre/ref modes do not decode)");
        usage();
    }
    Args {
        mode: mode.unwrap(),
        flash,
        reference_file,
        capture,
        out,
        offset,
        len_override,
        value_tables,
    }
}

fn write_map(out: &Option<PathBuf>, title: &str, blocks: &[usize]) {
    if let Some(p) = out {
        let mut s = format!("{title}\n");
        for &b in blocks {
            s.push_str(&format!(
                "block {} byte 0x{:08x}..0x{:08x}\n",
                b,
                b * 1024,
                (b + 1) * 1024
            ));
        }
        std::fs::write(p, s).expect("write map");
    }
}

fn main() {
    let args = parse_args();
    let capture = std::fs::read(&args.capture).unwrap_or_else(|e| {
        eprintln!("error: cannot read capture {}: {e}", args.capture.display());
        std::process::exit(2);
    });
    if capture.is_empty() {
        eprintln!("error: capture is empty");
        std::process::exit(2);
    }

    match args.mode {
        Mode::Pre => {
            let r = classify_residue(&capture);
            println!("P2 pre-decode residue classification (all-zero oracle, §18.3)");
            println!("  capture            : {} bytes", r.len);
            println!(
                "  non-zero bytes     : {} ({}‰)",
                r.nonzero_bytes, r.nonzero_permille
            );
            println!(
                "  first non-zero     : {}",
                r.first_nonzero
                    .map(|i| format!("byte 0x{i:08x}"))
                    .unwrap_or_else(|| "none".into())
            );
            println!("  longest nz run     : {}", r.longest_nonzero_run);
            println!("  non-zero blocks    : {}", r.nonzero_blocks.len());
            write_map(&args.out, "non-zero blocks (1024-byte):", &r.nonzero_blocks);
            if r.nonzero_bytes == 0 {
                println!("VERDICT: ZERO — matches the emulator-side expectation (§18.3); residue ruled out in this window");
                std::process::exit(0);
            }
            println!("VERDICT: RESIDUE PRESENT — the §21.6 item-2 answer; load it via AddressSpace::copy_in and re-run the decode compare");
            std::process::exit(1);
        }
        Mode::Post | Mode::Ref => {
            let reference: Vec<u8> = match &args.mode {
                Mode::Ref => {
                    std::fs::read(args.reference_file.as_ref().unwrap()).unwrap_or_else(|e| {
                        eprintln!("error: cannot read reference: {e}");
                        std::process::exit(2);
                    })
                }
                Mode::Post => {
                    let flash = args.flash.as_ref().unwrap();
                    let img = firmware::load(flash).unwrap_or_else(|e| {
                        eprintln!("error: cannot load firmware: {e}");
                        std::process::exit(2);
                    });
                    let mut config = MachineConfig::default();
                    if let Some(vt_path) = &args.value_tables {
                        let raw = std::fs::read(vt_path).unwrap_or_else(|e| {
                            eprintln!("error: cannot read value tables {}: {e}", vt_path.display());
                            std::process::exit(2);
                        });
                        let tables = truman_emulator::inflate::ValueTables::from_bytes(&raw)
                            .unwrap_or_else(|e| {
                                eprintln!("error: value tables {}: {e}", vt_path.display());
                                std::process::exit(2);
                            });
                        println!(
                            "value tables: {} (non-default engine ladder, §35)",
                            vt_path.display()
                        );
                        config.value_tables = tables;
                    }
                    let mut m = Machine::new(config, &img);
                    m.reset();
                    let res = m.run(2_000_000);
                    println!(
                        "boot: stop={:?} steps={} pc={:08x}",
                        res.stop, res.steps, res.pc
                    );
                    let total = match args.len_override {
                        Some(n) => n,
                        None => {
                            let w = m
                                .bus
                                .peek_phys(ENGINE_IMG_WIDTH_REG, AccessSize::Word)
                                .unwrap_or(0x400);
                            let h = m
                                .bus
                                .peek_phys(ENGINE_IMG_HEIGHT_REG, AccessSize::Word)
                                .unwrap_or(78);
                            (w as usize) * (h as usize)
                        }
                    };
                    let mut reference = vec![0u8; total];
                    let got = m.bus.copy_out(args.offset, &mut reference);
                    if got != total {
                        eprintln!("error: reference window short read ({got}/{total})");
                        std::process::exit(2);
                    }
                    println!(
                        "reference: phys 0x{:08x}, {} bytes (engine W×H)",
                        args.offset, total
                    );
                    reference
                }
                _ => unreachable!(),
            };
            let c = compare_dumps(&reference, &capture);
            println!("P1 post-decode comparison (emulator reference vs device capture)");
            println!("  reference          : {} bytes", c.reference_len);
            // Milestone AG (§34.2/P3): the vendor probe window exposes the
            // decode's Adler-32 as two 16-bit halves; this line is the
            // emulator-side fingerprint to compare a P3 probe read against.
            println!("  reference adler-32 : 0x{:08x}", adler32(&reference));
            println!("  captured           : {} bytes", c.captured_len);
            println!("  compared           : {} bytes", c.compared_len);
            println!(
                "  matching           : {} ({}‰)",
                c.matching_bytes, c.match_permille
            );
            println!(
                "  first divergence   : {}",
                c.first_divergence
                    .map(|i| format!("byte 0x{i:08x}"))
                    .unwrap_or_else(|| "none".into())
            );
            println!(
                "  divergent blocks   : {} (1024-byte)",
                c.divergent_blocks.len()
            );
            println!("  longest match run  : {}", c.longest_match_run);
            if c.captured_len > c.reference_len {
                println!(
                    "  size delta         : capture is +{} bytes vs reference",
                    c.captured_len - c.reference_len
                );
            } else if c.captured_len < c.reference_len {
                println!(
                    "  size delta         : capture is -{} bytes vs reference",
                    c.reference_len - c.captured_len
                );
            }
            write_map(
                &args.out,
                "divergent blocks (1024-byte):",
                &c.divergent_blocks,
            );
            if c.first_divergence.is_none() {
                if c.captured_len == c.reference_len {
                    println!("VERDICT: IDENTICAL — the engine model is validated end-to-end; if §19's head dies in this reference, suspect the capture point first (docs/hardware-session.md)");
                } else {
                    println!("VERDICT: PREFIX-IDENTICAL — sizes differ (see delta); record both lengths (§26.4)");
                }
                std::process::exit(0);
            }
            println!("VERDICT: DIVERGENT — first_divergence and the block map are the decisive constraint for the LZ-semantics solve (§14/§26)");
            std::process::exit(1);
        }
    }
}
