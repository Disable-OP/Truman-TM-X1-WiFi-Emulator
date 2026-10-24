//! Q8 escalation — the loader's own checksum code path, observed live
//! (docs/firmware-analysis.md §22, Milestone T).
//!
//! Q8 (§15.2, §16.2) is the decisive missing oracle: two descriptor
//! checksum anchors (0xB56FB05D for the driver payload, 0xB5EBF9FF for the
//! eCos container) that no standard checksum family reproduces. §21.6
//! names "the loader's own checksum code path (if any) and per-descriptor
//! accumulation schemes" as the next escalation. This example performs
//! exactly that observation: it boots the REAL firmware with physical-
//! range watchpoints over
//!
//!   * the stage-1 data-area descriptor words (file 0x1200..0x1280),
//!   * the 0x10000-package eCos descriptor entry (file 0x10010..0x1002C),
//!   * the driver payload bytes (file 0x2F6C..0x6948),
//!   * the eCos container bytes (file 0x1002C..0x96B8) — the two flash
//!     windows (XIP 0x1FC00000 and the Q2 linear mirror 0x18000000) are
//!     watched separately so the trace also shows WHICH window the loader
//!     actually reads,
//!   * the driver payload's DRAM destination (phys 0x02000000..0x02003ADC,
//!     reads and writes — writes are the copy, reads after it would be a
//!     digest pass over the copied bytes).
//!
//! Every hit is attributed to the issuing instruction (step, PC) by the
//! machine's run loop. The report prints per-range hit counts, PC
//! histograms with flash-file offset attribution, and the full ordered
//! trace of the descriptor-word reads — whoever computes the checksum
//! must appear there. If nothing ever reads a checksum word, that is the
//! finding (the loader does not validate; Q8's premise weakens).
//!
//! Usage:
//!   cargo run --release --example q8_watch <flashfile.bin> \
//!       [--max-steps N] [--detail K]
use std::path::PathBuf;
use std::process::ExitCode;

use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig, StopReason};
use truman_emulator::memory::{WatchHit, WatchKind, WatchKinds};

/// The eCos entry: the loader's `jr v0` target (§13.3, §19.1).
const ENTRY: u32 = 0xA160_0000;

/// Watched flash-file ranges: (name, offset, length).
const FLASH_RANGES: [(&str, u32, u32); 4] = [
    ("stage1-desc", 0x1200, 0x80),
    ("ecos-desc", 0x10010, 0x1C),
    ("driver-payload", 0x2F6C, 0x3ADC),
    ("ecos-container", 0x1002C, 0x868C),
];

/// The driver payload's DRAM destination (load address word 0x1214 = 0xA2000000).
const DRAM_DEST: u32 = 0x0200_0000;

/// Map a virtual PC to a file/region offset for readable attribution.
fn pc_loc(pc: u32) -> String {
    let phys = pc & 0x1FFF_FFFF;
    if (0x1FC0_0000..0x2000_0000).contains(&phys) {
        format!("file 0x{:06x}", phys - 0x1FC0_0000)
    } else if (0x1800_0000..0x1840_0000).contains(&phys) {
        format!("file 0x{:06x} (mirror)", phys - 0x1800_0000)
    } else if phys < 0x0400_0000 {
        format!("dram 0x{:08x}", phys)
    } else {
        format!("phys 0x{:08x}", phys)
    }
}

fn histogram(hits: &[&WatchHit], top: usize) -> Vec<(u32, usize)> {
    let mut counts: Vec<(u32, usize)> = Vec::new();
    for h in hits {
        match counts.iter_mut().find(|(pc, _)| *pc == h.pc) {
            Some((_, n)) => *n += 1,
            None => counts.push((h.pc, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    counts.truncate(top);
    counts
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: q8_watch <flashfile.bin> [--max-steps N] [--detail K]");
        return ExitCode::FAILURE;
    }
    let mut max_steps: u64 = 4_000_000;
    let mut detail: usize = 40;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--max-steps" => {
                i += 1;
                max_steps = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(max_steps);
            }
            "--detail" => {
                i += 1;
                detail = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(detail);
            }
            other => {
                let _ = other;
            }
        }
        i += 1;
    }

    let img = match firmware::load(&PathBuf::from(&args[1])) {
        Ok(img) => img,
        Err(e) => {
            eprintln!("load: {e}");
            return ExitCode::FAILURE;
        }
    };
    let cfg = MachineConfig::default();
    let mut m = Machine::new(cfg.clone(), &img);

    // Watch both flash windows for every interesting file range.
    let windows = [("xip", cfg.flash_base), ("mirror", cfg.win_180_base)];
    for (wname, base) in windows {
        for (rname, off, len) in FLASH_RANGES {
            println!(
                "watch {wname}/{rname}: phys 0x{:08x}..0x{:08x} (file 0x{:x}+0x{:x}) READ",
                base + off,
                base + off + len,
                off,
                len
            );
            m.add_watch(base + off, len, WatchKinds::READ);
        }
    }
    // The driver payload's DRAM destination: writes = the copy, reads after
    // it = a digest pass over the copied bytes.
    println!(
        "watch dram-dest: phys 0x{DRAM_DEST:08x}..0x{:08x} READ_WRITE",
        DRAM_DEST + 0x3ADC
    );
    m.add_watch(DRAM_DEST, 0x3ADC, WatchKinds::READ_WRITE);

    truman_emulator::boot::reset_with_diagnostics(&mut m, false);
    let res = m.run_until_pc(max_steps, ENTRY);
    println!("boot: {} steps, stop: {}", res.steps, res.stop);
    let hits = m.take_watch_hits();
    println!(
        "hits: {} recorded (overflow: {})",
        hits.len(),
        m.bus.watch_overflow()
    );

    // Group by watched range label.
    let mut groups: Vec<(String, Vec<&WatchHit>)> = Vec::new();
    for h in &hits {
        let phys = h.phys & 0x1FFF_FFFF;
        let label = if (DRAM_DEST..DRAM_DEST + 0x3ADC).contains(&phys) {
            "dram-dest".to_string()
        } else {
            let file = if phys >= cfg.flash_base {
                phys - cfg.flash_base
            } else {
                phys - cfg.win_180_base
            };
            let wname = if phys >= cfg.flash_base {
                "xip"
            } else {
                "mirror"
            };
            match FLASH_RANGES
                .iter()
                .find(|(_, off, len)| file >= *off && file < off + len)
            {
                Some((rname, _, _)) => format!("{wname}/{rname}"),
                None => format!("{wname}/?"),
            }
        };
        match groups.iter_mut().find(|(n, _)| *n == label) {
            Some((_, v)) => v.push(h),
            None => groups.push((label, vec![h])),
        }
    }

    for (label, ghits) in &groups {
        let reads = ghits.iter().filter(|h| h.kind == WatchKind::Read).count();
        let writes = ghits.iter().filter(|h| h.kind == WatchKind::Write).count();
        println!(
            "\n== {label}: {} hits ({reads} reads, {writes} writes)",
            ghits.len()
        );
        for (pc, n) in histogram(ghits, 24) {
            println!("   pc 0x{pc:08x} ({}) x{n}", pc_loc(pc));
        }
    }

    // The decisive ordered trace: descriptor-word reads.
    for (label, ghits) in &groups {
        if !label.contains("desc") {
            continue;
        }
        println!("\n-- ordered trace {label} (first {detail}):");
        for h in ghits.iter().take(detail) {
            println!(
                "   step {:>8} pc 0x{:08x} ({}) {} 0x{:08x} <- 0x{:x} ({}B) [{}]",
                h.step,
                h.pc,
                pc_loc(h.pc),
                match h.kind {
                    WatchKind::Read => "read ",
                    WatchKind::Write => "write",
                    WatchKind::Fetch => "fetch",
                },
                h.vaddr,
                h.value,
                h.size.bytes(),
                h.region
            );
        }
    }

    // Ordered trace of dram-dest READS (a digest pass over the copied
    // payload would show up here as ~0x3ADC reads; anything else is
    // something other than checksum validation).
    if let Some((_, ghits)) = groups.iter().find(|(n, _)| n == "dram-dest") {
        let reads: Vec<&&WatchHit> = ghits.iter().filter(|h| h.kind == WatchKind::Read).collect();
        println!(
            "\n-- ordered trace dram-dest READS ({} of {}):",
            reads.len(),
            ghits.len()
        );
        for h in reads.iter().take(detail) {
            println!(
                "   step {:>8} pc 0x{:08x} ({}) read 0x{:08x} <- 0x{:x} ({}B)",
                h.step,
                h.pc,
                pc_loc(h.pc),
                h.vaddr,
                h.value,
                h.size.bytes()
            );
        }
    }

    // Payload read pattern: interleaving of the two reader PCs and the
    // exact source window actually read (vaddr first/last).
    if let Some((_, ghits)) = groups
        .iter()
        .find(|(n, _)| n == "mirror/driver-payload" || n == "xip/driver-payload")
    {
        println!(
            "\n-- payload source window: first 0x{:08x} last 0x{:08x}",
            ghits.first().map(|h| h.vaddr).unwrap_or(0),
            ghits.last().map(|h| h.vaddr).unwrap_or(0)
        );
        println!("-- payload reads, first {detail} in order:");
        for h in ghits.iter().take(detail) {
            println!(
                "   step {:>8} pc 0x{:08x} read 0x{:08x} <- 0x{:x} ({}B)",
                h.step,
                h.pc,
                h.vaddr,
                h.value,
                h.size.bytes()
            );
        }
    }

    // Phase 2: continue PAST the eCos entry. A checksum validator in the
    // decompressed stage would show up as a bulk data-read pass over its
    // own image (the pixel buffer), the copied payload or the container.
    // The pixel buffer is watched for data reads only — executing it shows
    // up as fetches, which a validator would not need.
    let phase2_limit: u64 = 2_000_000;
    println!("\nphase 2: continuing {phase2_limit} steps past the entry (pixel-buf data-read watch added)");
    m.add_watch(0x0160_0000, 79_872, WatchKinds::READ);
    let res2 = m.run(phase2_limit);
    println!("phase 2: {} steps, stop: {}", res2.steps, res2.stop);
    let hits2 = m.take_watch_hits();
    let mut g2: Vec<(String, usize)> = Vec::new();
    for h in &hits2 {
        if h.kind == WatchKind::Fetch {
            continue; // execution of the eCos image
        }
        let phys = h.phys & 0x1FFF_FFFF;
        let label = if (0x0160_0000..0x0161_3800).contains(&phys) {
            "pixel-buf".to_string()
        } else if phys < 0x0400_0000 {
            "dram-dest".to_string()
        } else if phys >= cfg.win_180_base {
            "mirror".to_string()
        } else {
            "xip".to_string()
        };
        match g2.iter_mut().find(|(n, _)| *n == label) {
            Some((_, n)) => *n += 1,
            None => g2.push((label, 1)),
        }
    }
    if g2.is_empty() {
        println!("phase 2 data-read hits over watched ranges: NONE");
    } else {
        for (label, n) in g2 {
            println!("phase 2 data-read hits {label}: {n}");
        }
        println!("-- phase 2 ordered trace (first {detail}):");
        for h in hits2
            .iter()
            .filter(|h| h.kind != WatchKind::Fetch)
            .take(detail)
        {
            println!(
                "   step {:>8} pc 0x{:08x} ({}) read 0x{:08x} <- 0x{:x} ({}B)",
                h.step,
                h.pc,
                pc_loc(h.pc),
                h.vaddr,
                h.value,
                h.size.bytes()
            );
        }
    }

    match res.stop {
        StopReason::EntryReached { .. } => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}
