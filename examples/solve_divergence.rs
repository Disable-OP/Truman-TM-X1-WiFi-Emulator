//! The P1 DIVERGENT row executor (Milestone AI, §36): the pre-registered
//! "feed the solve" step of docs/hardware-session.md as one command.
//!
//! boots the real firmware, extracts the engine's pixel-window reference
//! (the same window `dram_compare --post` compares), and on divergence
//! maps the first divergent byte to the decode event that produced it —
//! mechanically, with no search: the traced replay of the boot's own
//! decode (same stream bytes, start bit, preset and ladder, captured at
//! service time) attributes the byte to its symbol, and the identical
//! prefix pins what that symbol's output must still satisfy.
//!
//! The report is attribution only. Per the P1 tree it asserts nothing
//! about the true decode; per §19.3 a divergence ON a literal byte means
//! value tables alone cannot explain it (a literal is stream-determined),
//! a divergence INSIDE a match span names the one match whose length or
//! source the ladder must move, and a divergence at or beyond the decode's
//! own output length is band-fill territory (a length-accounting
//! difference, the §26.4 family).
//!
//! Usage:
//!   solve_divergence <flashfile.bin> <capture.bin>
//!       [--value-tables <f>] [--offset <phys>] [--len <bytes>]
//!       [--out <events.tsv>] [--context <bytes>]
//!
//! Options:
//!   --value-tables <f>  engine value ladder from a file (§35) — boot and
//!                       replay both use it (the post-capture candidate)
//!   --offset <phys>     pixel base override (default 0x01600000)
//!   --len <bytes>       window length override (default W*H)
//!   --out <file>        write the full per-symbol event trace (TSV)
//!   --context <bytes>   reference-vs-capture hex context around the
//!                       divergence (default 16, 0..=64)
//!
//! Exit codes: 0 = identical (P1 IDENTICAL row); 1 = divergent or
//! size-delta (attribution report produced — actionable data); 2 =
//! usage, IO, engine-did-not-run, or replay-integrity error.

use std::path::PathBuf;

use truman_emulator::firmware;
use truman_emulator::inflate::{
    inflate_engine_with_traced, inflate_engine_with_traced_preset, DecodeEvent, DecodeEventKind,
};
use truman_emulator::machine::{
    adler32, attribute_divergence, compare_dumps, Machine, MachineConfig, ENGINE_MAX_OUT,
    ENGINE_PRESET_BYTES,
};
use truman_emulator::memory::AccessSize;

const DEFAULT_PIXEL_BASE: u32 = 0x0160_0000;
const ENGINE_IMG_WIDTH_REG: u32 = 0x1FFE_3828;
const ENGINE_IMG_HEIGHT_REG: u32 = 0x1FFE_382C;

fn usage() -> ! {
    eprintln!(
        "usage:\n  \
         solve_divergence <flashfile.bin> <capture.bin>\n\
         options:\n  \
         --value-tables <f>  engine value ladder from a file (§35)\n  \
         --offset <phys>     pixel base override (default 0x01600000)\n  \
         --len <bytes>       window length override (default W*H)\n  \
         --out <file>        write the per-symbol event trace (TSV)\n  \
         --context <bytes>   hex context around the divergence (default 16)\n\
         exit codes: 0 identical, 1 divergent/size-delta (report), 2 error"
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
    context: usize,
}

fn parse_args() -> Args {
    let mut positional: Vec<String> = Vec::new();
    let mut value_tables = None;
    let mut offset = DEFAULT_PIXEL_BASE;
    let mut len_override = None;
    let mut out = None;
    let mut context = 16usize;
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
            "--context" => {
                context = a
                    .get(i + 1)
                    .unwrap_or_else(|| usage())
                    .parse::<usize>()
                    .unwrap_or_else(|_| usage());
                if context > 64 {
                    usage();
                }
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
        context,
    }
}

fn kind_fields(kind: &DecodeEventKind) -> String {
    match kind {
        DecodeEventKind::Literal { value } => format!("literal value=0x{value:02x}"),
        DecodeEventKind::EndOfBlock => "end_of_block".to_string(),
        DecodeEventKind::Match {
            len_code,
            len_extra_w,
            len_extra_v,
            length,
            dist_code,
            dist_extra_w,
            dist_extra_v,
            dist,
        } => format!(
            "match len_code={len_code} len_extra_w={len_extra_w} len_extra_v={len_extra_v} \
             length={length} dist_code={dist_code} dist_extra_w={dist_extra_w} \
             dist_extra_v={dist_extra_v} dist={dist}"
        ),
    }
}

fn write_events_tsv(path: &PathBuf, events: &[DecodeEvent]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    writeln!(f, "index\tout_start\tout_len\tkind_fields")?;
    for e in events {
        writeln!(
            f,
            "{}\t{}\t{}\t{}",
            e.index,
            e.out_start,
            e.out_len,
            kind_fields(&e.kind)
        )?;
    }
    f.flush()
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

    let config = match &args.value_tables {
        None => MachineConfig::default(),
        Some(p) => {
            let raw = std::fs::read(p).unwrap_or_else(|e| {
                eprintln!("error: cannot read {}: {e}", p.display());
                std::process::exit(2);
            });
            let tables =
                truman_emulator::inflate::ValueTables::from_bytes(&raw).unwrap_or_else(|e| {
                    eprintln!("error: value tables {}: {e}", p.display());
                    std::process::exit(2);
                });
            println!(
                "value tables: {} ({})",
                p.display(),
                if tables == truman_emulator::inflate::ValueTables::rfc1951() {
                    "the RFC ladder"
                } else {
                    "non-default candidate ladder"
                }
            );
            MachineConfig {
                value_tables: tables,
                ..MachineConfig::default()
            }
        }
    };

    let mut m = Machine::new(config.clone(), &img);
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
    println!(
        "window: phys 0x{:08x}, {} bytes (engine W×H = {}×{}), adler-32 0x{:08x}",
        args.offset,
        total,
        w,
        h,
        adler32(&window)
    );

    // The P1 comparison (the dram_compare --post semantics, subset).
    let cmp = compare_dumps(&window, &capture);
    println!(
        "compare: compared={} matching={} ({}‰) longest_run={}",
        cmp.compared_len, cmp.matching_bytes, cmp.match_permille, cmp.longest_match_run
    );
    let Some(k) = cmp.first_divergence else {
        if cmp.compared_len == cmp.reference_len && cmp.compared_len == cmp.captured_len {
            println!(
                "verdict: P1 IDENTICAL — the emulator's decode equals the capture. Per the \
                 pre-registered tree: suspect the capture point FIRST (re-capture at the \
                 `jr v0` handoff, kseg1 view) before revisiting the engine model."
            );
            std::process::exit(0);
        }
        println!(
            "verdict: P1 PREFIX-IDENTICAL, size Δ (reference {} vs captured {} bytes) — same \
             decode, different allocation/length accounting; record both lengths and re-run \
             the size-anchor algebra with the true length (the §26.4 family).",
            cmp.reference_len, cmp.captured_len
        );
        std::process::exit(1);
    };
    println!("first divergence: byte {k}");

    // Traced replay of the boot's own decode, byte-verified against the
    // machine's window before any attribution is reported.
    let inputs = m.engine_decode_inputs().unwrap_or_else(|| {
        eprintln!("error: the engine never ran this boot — no decode to attribute against");
        std::process::exit(2);
    });
    let mut replay = Vec::new();
    let mut events: Vec<DecodeEvent> = Vec::new();
    // The §36 replay uses the exact preset the boot's decode used (§44):
    // empty = the documented zero fill; non-empty = the loaded
    // `MachineConfig::preset_window` content, byte-for-byte. Either way
    // the replay must be byte-identical to the boot decode or the
    // integrity gate below refuses to attribute.
    let replay_res = if inputs.preset_window.is_empty() {
        inflate_engine_with_traced(
            &inputs.stream,
            inputs.start_bit,
            ENGINE_PRESET_BYTES,
            &mut replay,
            ENGINE_MAX_OUT,
            &config.value_tables,
            Some(&mut events),
        )
    } else {
        inflate_engine_with_traced_preset(
            &inputs.stream,
            inputs.start_bit,
            &inputs.preset_window,
            &mut replay,
            ENGINE_MAX_OUT,
            &config.value_tables,
            Some(&mut events),
        )
    };
    let decoded_len = replay_res.unwrap_or_else(|e| {
        eprintln!("error: replay decode failed: {e}");
        std::process::exit(2);
    });
    if decoded_len != inputs.decoded_len || replay[..] != window[..decoded_len] {
        eprintln!(
            "error: replay integrity failure (replay {decoded_len} bytes vs boot {} — \
             instrument state drifted; refusing to attribute)",
            inputs.decoded_len
        );
        std::process::exit(2);
    }
    println!(
        "replay: byte-identical to the boot decode ({decoded_len} bytes; band fill {} bytes; \
         {} events)",
        total.saturating_sub(decoded_len),
        events.len()
    );

    let attr = attribute_divergence(&events, decoded_len, &window, &capture, k);
    match &attr.event {
        None => {
            println!(
                "attribution: byte {k} is NOT decode output — it lies in the zero-filled band \
                 region (decoded_len={decoded_len}). A non-zero capture byte here is a \
                 length-accounting difference (the §26.4 family): the true decode produced \
                 more output than the modeled one."
            );
        }
        Some(ev) => {
            println!(
                "attribution: byte {k} is output byte {} of event #{} (span {}..{}, \
                 offset-in-event {}):",
                ev.offset_in_event,
                ev.event_index,
                ev.out_start,
                ev.out_start + ev.out_len,
                ev.offset_in_event
            );
            println!("  symbol: {}", kind_fields(&ev.kind));
            match &ev.kind {
                DecodeEventKind::Literal { value } => {
                    println!(
                        "  meaning: the divergent byte is a LITERAL (stream-determined byte \
                         0x{value:02x} under the active ladder). Value tables cannot move a \
                         literal (§19.3): under a symbol-identical parse this divergence is \
                         not a value-table difference — the parse itself differs from the \
                         modeled one. Per the P1 tree: record the capture-point suspicion \
                         check first, then treat the parse (header/tables path), not the \
                         ladder, as the variable."
                    );
                }
                DecodeEventKind::EndOfBlock => {
                    println!("  meaning: unreachable — end-of-block events span no bytes.");
                }
                DecodeEventKind::Match { .. } => {
                    let l = ev.out_len;
                    let j = ev.offset_in_event;
                    println!(
                        "  meaning: the divergent byte is INSIDE a match's fill. The match's \
                         first {j} output bytes are capture-identical (first-divergence \
                         definition), so its referenced source bytes were correct for that \
                         prefix; the ladder must move THIS match (length > {j} for the span \
                         to extend differently, or the symbol sequence around it differs). \
                         This is the pre-registered decisive constraint: the first match \
                         whose value semantics differ."
                    );
                    let _ = l;
                }
            }
        }
    }

    // Reference-vs-capture context around the divergence.
    let ctx = args.context;
    let lo = k.saturating_sub(ctx);
    let hi = (k + ctx + 1).min(cmp.compared_len);
    print!("context [{lo}..{hi}) ref :");
    for b in &window[lo..hi] {
        print!(" {b:02x}");
    }
    println!();
    print!("context [{lo}..{hi}) cap :");
    for b in &capture[lo..hi] {
        print!(" {b:02x}");
    }
    println!();

    if let Some(out_path) = &args.out {
        write_events_tsv(out_path, &events).unwrap_or_else(|e| {
            eprintln!("error: cannot write {}: {e}", out_path.display());
            std::process::exit(2);
        });
        println!("events: {} rows -> {}", events.len(), out_path.display());
    }
    println!(
        "verdict: P1 DIVERGENT — attribution above; feed it to the solve per the \
         pre-registered tree (docs/hardware-session.md). This report asserts nothing about \
         the true decode."
    );
    std::process::exit(1);
}
