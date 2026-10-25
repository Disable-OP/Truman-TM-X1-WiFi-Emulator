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
//! emulator-side fingerprint for the P3 probe-window read (§34.2) —
//! together with the expected two-probe-readout halves (addr 0 = high,
//! addr 1 = low; Milestone AP), so the session's four debugger
//! operations have their expected values printed, not derived by hand.
//!
//! Post mode's `--preset-window` is the §44 band-window seam (the
//! sibling of `--value-tables`): the engine's far-back matches copy
//! from the 32 KiB window ahead of the output, which the model
//! zero-fills (§13/§18.3). A real device's pre-decode band capture (the
//! P2b experiment, §21.6 item 2) is a file here, not a recompile — the
//! run reports the preset's adler so the window under test is recorded
//! with the verdict.
//!
//! Pre mode's `--reboot-residue` is the §45 residue re-boot seam (the
//! last manual step of the P2 decision-tree row, now one command): it
//! boots the real firmware twice — once clean, once with the capture's
//! bytes copy_in'd at the P2 window base BEFORE the boot chain runs —
//! and compares the two decode outputs byte-for-byte. RESIDUE INERT
//! (identical) measures the model's §18.3 statement on the window;
//! RESIDUE ACTIVE (divergent) localizes the channels the solve must
//! account for. The compare window is always the decode output (the
//! `--post` artifact); the injection base is the capture's first byte.
//!
//! Options:
//!   --out <map.txt>   write the 1024-byte block map to a file
//!   --offset <phys>   post mode: pixel base override (default 0x01600000)
//!   --len <bytes>     post mode: reference length override (default W*H)
//!   --value-tables <f> post mode: engine value ladder from a file (§35,
//!                     the post-capture swap as data, no recompile)
//!   --preset-window <f> post mode: band-window preset CONTENT from a
//!                     file (§44; exactly 0x8000 bytes, or a larger
//!                     dump's tail)
//!   --reboot-residue <f> pre mode: flashfile for the §45 two-boot
//!                     residue experiment (the P2 RESIDUE PRESENT row,
//!                     mechanical)
//!   --base <phys>     pre mode: the capture's window base for the
//!                     injection (default 0x01600000; the wide P2
//!                     variant starts 1 MiB lower, 0x01500000)
//!
//! Exit codes: 0 = identical / zero residue; 1 = divergent / non-zero
//! residue (actionable data); 2 = usage, IO or empty-overlap error.

use std::path::PathBuf;

use truman_emulator::firmware;
use truman_emulator::machine::{
    adler32, adler32_probe_halves, classify_residue, compare_dumps, Machine, MachineConfig,
};
use truman_emulator::memory::AccessSize;

const DEFAULT_PIXEL_BASE: u32 = 0x0160_0000;
const ENGINE_IMG_WIDTH_REG: u32 = 0x1FFE_3828;
const ENGINE_IMG_HEIGHT_REG: u32 = 0x1FFE_382C;
const BOOT_STEP_BUDGET: u64 = 2_000_000;

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
         --preset-window <f> post mode: band-window preset content from a\n\
                           file (§44; 0x8000 bytes or a larger dump's tail)\n\
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
    /// The `--preset-window` file, when given (§44). Post mode only, same
    /// rejection rule as `value_tables`.
    preset_window: Option<PathBuf>,
    /// The `--reboot-residue` flashfile, when given (§45). Pre mode only:
    /// post and ref do not run the P2 classification, so a value there is
    /// a usage error (rejected in parse_args).
    reboot_residue: Option<PathBuf>,
    /// The `--base` phys address: the capture's first byte, where the
    /// §45 injection lands. Pre mode only, and only meaningful with
    /// `reboot_residue` (rejected without it).
    base: u32,
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
    let mut preset_window: Option<PathBuf> = None;
    let mut reboot_residue: Option<PathBuf> = None;
    let mut base = DEFAULT_PIXEL_BASE;
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
            "--preset-window" => {
                i += 1;
                preset_window = a.get(i).map(PathBuf::from);
                if preset_window.is_none() {
                    usage();
                }
            }
            "--reboot-residue" => {
                i += 1;
                reboot_residue = a.get(i).map(PathBuf::from);
                if reboot_residue.is_none() {
                    usage();
                }
            }
            "--base" => {
                i += 1;
                base = u32::from_str_radix(
                    a.get(i).unwrap_or_else(|| usage()).trim_start_matches("0x"),
                    16,
                )
                .unwrap_or_else(|_| usage());
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
    if preset_window.is_some() && !matches!(mode, Some(Mode::Post)) {
        eprintln!("error: --preset-window applies to --post only (pre/ref modes do not decode)");
        usage();
    }
    if reboot_residue.is_some() && !matches!(mode, Some(Mode::Pre)) {
        eprintln!(
            "error: --reboot-residue applies to --pre only (post/ref modes have no residue to inject)"
        );
        usage();
    }
    if reboot_residue.is_none() && base != DEFAULT_PIXEL_BASE {
        eprintln!(
            "error: --base applies to --pre --reboot-residue only (it is the injection base)"
        );
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
        preset_window,
        reboot_residue,
        base,
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

/// The §45 residue re-boot experiment (the P2 RESIDUE PRESENT decision
/// row, mechanical): boot the real firmware clean, boot it again with
/// the capture's bytes copy_in'd at the P2 window base BEFORE the boot
/// chain runs, extract both decode-output windows (the `--post`
/// artifact: PNG_IMG_WIDTH × PNG_IMG_HEIGHT at the pixel base) and
/// compare them byte-for-byte.
///
/// RESIDUE INERT (byte-identical) is the measured form of the model's
/// §18.3 statement for this window; RESIDUE ACTIVE (divergent)
/// localizes the channels any solve must account for. The boots are
/// deterministic, so the verdict is reproducible from the capture file
/// alone.
fn reboot_residue_experiment(args: &Args, capture: &[u8]) {
    let flash = args.reboot_residue.as_ref().unwrap();
    let img = firmware::load(flash).unwrap_or_else(|e| {
        eprintln!("error: cannot load firmware {}: {e}", flash.display());
        std::process::exit(2);
    });

    // Boot A: the clean reference (the model's own §18.3 zero state).
    let mut a = Machine::new(MachineConfig::default(), &img);
    a.reset();
    let res_a = a.run(BOOT_STEP_BUDGET);

    // Boot B: the same boot with the capture injected pre-decode. A
    // short write means the capture does not fit the mapped window at
    // this base — a named error, not a silent truncation.
    let mut b = Machine::new(MachineConfig::default(), &img);
    b.reset();
    let written = b.bus.copy_in(args.base, capture);
    if written != capture.len() {
        eprintln!(
            "error: residue injection short write ({written}/{} bytes at phys 0x{:08x}) — \
             the capture does not fit the mapped window at this base",
            capture.len(),
            args.base
        );
        std::process::exit(2);
    }
    let res_b = b.run(BOOT_STEP_BUDGET);

    // Extract the decode-output window from both boots (the same
    // artifact `--post` compares: engine W×H at the pixel base).
    let window_of = |m: &mut Machine| {
        let w = m
            .bus
            .peek_phys(ENGINE_IMG_WIDTH_REG, AccessSize::Word)
            .unwrap_or(0x400);
        let h = m
            .bus
            .peek_phys(ENGINE_IMG_HEIGHT_REG, AccessSize::Word)
            .unwrap_or(78);
        let total = (w as usize) * (h as usize);
        let mut buf = vec![0u8; total];
        let got = m.bus.copy_out(DEFAULT_PIXEL_BASE, &mut buf);
        if got != total {
            eprintln!("error: decode window short read ({got}/{total})");
            std::process::exit(2);
        }
        buf
    };
    let win_a = window_of(&mut a);
    let win_b = window_of(&mut b);

    println!("residue re-boot experiment (§45, one command):");
    println!(
        "  injected           : {} bytes at phys 0x{:08x} (adler 0x{:08x})",
        capture.len(),
        args.base,
        adler32(capture)
    );
    println!(
        "  clean boot         : stop={:?} steps={}",
        res_a.stop, res_a.steps
    );
    println!(
        "  residue boot       : stop={:?} steps={}",
        res_b.stop, res_b.steps
    );
    println!(
        "  clean decode       : {} bytes, adler 0x{:08x} (the boot pin is 0xcd8d03bf for this firmware)",
        win_a.len(),
        adler32(&win_a)
    );
    println!(
        "  residue decode     : {} bytes, adler 0x{:08x}",
        win_b.len(),
        adler32(&win_b)
    );
    let c = compare_dumps(&win_a, &win_b);
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
    if c.first_divergence.is_none() && c.captured_len == c.reference_len {
        println!(
            "  REBOOT VERDICT     : RESIDUE INERT — the decode under this pre-decode content is \
             byte-identical to the clean boot (the §21.6 item-2 answer, measured: no decode delta \
             through this window)"
        );
    } else {
        println!(
            "  REBOOT VERDICT     : RESIDUE ACTIVE — the pre-decode content changes the decode \
             (first divergence and the block map above localize the channels; the solve must \
             account for them)"
        );
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
                if args.reboot_residue.is_some() {
                    println!("  reboot experiment : skipped — the capture is all-zero, the injection equals the clean state (vacuous, §45)");
                }
                std::process::exit(0);
            }
            if args.reboot_residue.is_some() {
                println!("VERDICT: RESIDUE PRESENT — the §21.6 item-2 answer; the §45 residue re-boot experiment follows");
                reboot_residue_experiment(&args, &capture);
            } else {
                println!(
                    "VERDICT: RESIDUE PRESENT — the §21.6 item-2 answer; re-run under it (one command, §45): \
                     dram_compare --pre <capture> --reboot-residue <flashfile.bin> [--base <phys>]"
                );
            }
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
                    if let Some(pw_path) = &args.preset_window {
                        let raw = std::fs::read(pw_path).unwrap_or_else(|e| {
                            eprintln!(
                                "error: cannot read preset window {}: {e}",
                                pw_path.display()
                            );
                            std::process::exit(2);
                        });
                        // The band window is the 32 KiB immediately ahead
                        // of the decode output (§13). A capture of exactly
                        // that window is used as-is; a larger dump (the P2b
                        // band capture may be taken wide) contributes its
                        // TAIL; anything shorter cannot be anchored and is
                        // a named error, not a guess.
                        if raw.len() < truman_emulator::machine::ENGINE_PRESET_BYTES {
                            eprintln!(
                                "error: preset window {} is {} bytes; the band window is \
                                 0x{:x} — capture [0x015F8000,0x01600000) or pass the tail \
                                 of a larger dump",
                                pw_path.display(),
                                raw.len(),
                                truman_emulator::machine::ENGINE_PRESET_BYTES
                            );
                            std::process::exit(2);
                        }
                        let preset = if raw.len() == truman_emulator::machine::ENGINE_PRESET_BYTES {
                            raw
                        } else {
                            println!(
                                "preset window: using the last 0x{:x} bytes of {} (the band \
                                 window immediately before the pixel base)",
                                truman_emulator::machine::ENGINE_PRESET_BYTES,
                                pw_path.display()
                            );
                            raw[raw.len() - truman_emulator::machine::ENGINE_PRESET_BYTES..]
                                .to_vec()
                        };
                        println!(
                            "preset window: {} (0x{:x} bytes, non-default band fill, adler \
                             0x{:08x}, §44)",
                            pw_path.display(),
                            preset.len(),
                            adler32(&preset)
                        );
                        config.preset_window = Some(preset);
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
            let ref_adler = adler32(&reference);
            println!("  reference adler-32 : 0x{:08x}", ref_adler);
            // Milestone AP: the §34.2 half order in code — probe address 0
            // reads the high half, address 1 the low half. These are the
            // two `mdw` values the session's four debugger operations
            // should produce when the emulator's decode IS the device's.
            let (hi, lo) = adler32_probe_halves(ref_adler);
            println!(
                "  expected probe     : addr0 (high) = 0x{:04x}, addr1 (low) = 0x{:04x}",
                hi, lo
            );
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
