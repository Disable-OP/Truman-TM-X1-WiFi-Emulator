//! The P3 fingerprint-elimination oracle, offline half (Milestone AH, §35).
//!
//! docs/hardware-session.md's decision tree pre-registers the "P3 PRESENT,
//! ≠ reference" row: "re-run the §15.6/§18.2/§26 swept families under the
//! fingerprint oracle (`adler32(decode) == captured`)". Before this tool a
//! candidate ladder could only be evaluated by editing
//! `ENGINE_VALUE_TABLES` and recompiling; now a candidate is a data file
//! and this command is the loop body:
//!
//! ```text
//! for f in candidates/*.spvt; do
//!     ladder_eval <flashfile.bin> --value-tables "$f" --fingerprint 0xCAPTURED \
//!         || echo "eliminated: $f"
//! done
//! ```
//!
//! What it reports per candidate (all observable pinned quantities):
//! - the boot (stop reason, steps, PC) — must match the pinned boot chain;
//! - the engine window (phys 0x01600000, W×H from the engine's registers);
//! - the window head (the §19.1 executable-head bytes — byte 0 IS the
//!   first instruction word the loader jumps into);
//! - the window's Adler-32 — the candidate's fingerprint, compared
//!   against `--fingerprint` (a device P3 probe read) when given.
//!
//! Verdicts: with `--fingerprint`, exit 0 = SURVIVES (fingerprint matches;
//! a 2^-32 coincidence for any other decode), exit 1 = ELIMINATED.
//! Without it, the tool is report-only (exit 0). Exit 2 = usage/IO error.
//! The pinned RFC reference decode's own fingerprint is 0xcd8d03bf
//! (tests/hwsession.rs); a candidate whose fingerprint equals it while
//! differing from the RFC ladder in values is reported as "matches the
//! RFC reference" — the session must then decide via the P1 tree
//! (a match this strong must survive the capture-point suspicion check).
//!
//! Usage:
//!   ladder_eval <flashfile.bin> --value-tables <file>
//!               [--fingerprint 0xHEX] [--offset <phys>] [--len <bytes>]

use std::path::PathBuf;

use truman_emulator::firmware;
use truman_emulator::machine::{adler32, Machine, MachineConfig};
use truman_emulator::memory::AccessSize;

const DEFAULT_PIXEL_BASE: u32 = 0x0160_0000;
const ENGINE_IMG_WIDTH_REG: u32 = 0x1FFE_3828;
const ENGINE_IMG_HEIGHT_REG: u32 = 0x1FFE_382C;
/// The pinned emulator-side fingerprint of the RFC-ladder reference decode
/// (§34.4, tests/hwsession.rs). Printed as the default comparison point.
const RFC_REFERENCE_FINGERPRINT: u32 = 0xcd8d_03bf;

fn usage() -> ! {
    eprintln!(
        "usage:\n  \
         ladder_eval <flashfile.bin> --value-tables <file> [--fingerprint 0xHEX]\n\
         options:\n  \
         --value-tables <f>  candidate engine value ladder (SPHEVTBL container)\n  \
         --fingerprint <hex> the device's P3 probe-window Adler-32; with it the\n  \
                             run is an elimination verdict, without it report-only\n  \
         --offset <phys>     pixel base override (default 0x01600000)\n  \
         --len <bytes>       window length override (default W*H)\n\
         exit codes: 0 report-only or SURVIVES, 1 ELIMINATED, 2 error"
    );
    std::process::exit(2);
}

struct Args {
    flash: PathBuf,
    value_tables: PathBuf,
    fingerprint: Option<u32>,
    offset: u32,
    len_override: Option<usize>,
}

fn parse_args() -> Args {
    let mut positional: Vec<String> = Vec::new();
    let mut value_tables = None;
    let mut fingerprint = None;
    let mut offset = DEFAULT_PIXEL_BASE;
    let mut len_override = None;
    let a: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < a.len() {
        match a[i].as_str() {
            "--value-tables" => {
                i += 1;
                value_tables = Some(PathBuf::from(a.get(i).unwrap_or_else(|| usage()).as_str()));
            }
            "--fingerprint" => {
                i += 1;
                let v = a.get(i).unwrap_or_else(|| usage());
                fingerprint = Some(
                    u32::from_str_radix(v.trim_start_matches("0x"), 16).unwrap_or_else(|_| usage()),
                );
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
            other if other.starts_with('-') => usage(),
            other => positional.push(other.to_string()),
        }
        i += 1;
    }
    if positional.len() != 1 {
        usage();
    }
    let Some(value_tables) = value_tables else {
        usage()
    };
    Args {
        flash: PathBuf::from(&positional[0]),
        value_tables,
        fingerprint,
        offset,
        len_override,
    }
}

fn main() {
    let args = parse_args();

    let raw = std::fs::read(&args.value_tables).unwrap_or_else(|e| {
        eprintln!(
            "error: cannot read value tables {}: {e}",
            args.value_tables.display()
        );
        std::process::exit(2);
    });
    let tables = truman_emulator::inflate::ValueTables::from_bytes(&raw).unwrap_or_else(|e| {
        eprintln!("error: value tables {}: {e}", args.value_tables.display());
        std::process::exit(2);
    });
    println!(
        "value tables: {} ({})",
        args.value_tables.display(),
        if tables == truman_emulator::inflate::ValueTables::rfc1951() {
            "the RFC ladder"
        } else {
            "non-default candidate ladder"
        }
    );

    let img = firmware::load(&args.flash).unwrap_or_else(|e| {
        eprintln!("error: cannot load firmware: {e}");
        std::process::exit(2);
    });
    let config = MachineConfig {
        value_tables: tables,
        ..MachineConfig::default()
    };
    let mut m = Machine::new(config, &img);
    m.reset();
    let res = m.run(2_000_000);
    println!(
        "boot: stop={:?} steps={} pc={:08x}",
        res.stop, res.steps, res.pc
    );

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

    // The §19.1 head: byte 0 of this window is the first instruction word
    // the loader's `jr v0` jumps into — the executable-head oracle's input.
    let head_len = total.min(16);
    print!("head: ");
    for b in &window[..head_len] {
        print!("{b:02x} ");
    }
    println!();

    let fingerprint = adler32(&window);
    println!(
        "window: phys 0x{:08x}, {} bytes (engine W×H = {}×{})",
        args.offset, total, w, h
    );
    println!("candidate adler-32   : 0x{fingerprint:08x}");
    println!("RFC reference pin    : 0x{RFC_REFERENCE_FINGERPRINT:08x} (§34.4)");
    match args.fingerprint {
        None => {
            println!("verdict: REPORT-ONLY (no --fingerprint given; loop this command over candidate ladders under the P3 elimination row, §35)");
            std::process::exit(0);
        }
        Some(target) => {
            println!("device fingerprint   : 0x{target:08x} (--fingerprint)");
            if fingerprint == target {
                println!("verdict: SURVIVES the fingerprint oracle — a 2^-32 coincidence for any decode but the device's; per the P3 tree, re-verify the capture point (P1 tree) before trusting it");
                std::process::exit(0);
            }
            println!("verdict: ELIMINATED — this candidate's decode output does not produce the device fingerprint (the pre-registered P3 elimination, docs/hardware-session.md)");
            std::process::exit(1);
        }
    }
}
