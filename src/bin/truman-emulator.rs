//! `truman-emulator` command-line frontend.
//!
//! Subcommands:
//! - `inspect <firmware>` — identify and validate the image, print findings
//! - `run <firmware> [--max-steps N] [--trace-tail N] [--dump PHYS,LEN,PATH]
//!   [-v]` — boot and execute
//! - `trace <firmware> [--max-steps N] [--limit N] [-v]` — boot and print a
//!   per-instruction trace of the first N steps
//! - `map <firmware> [--pre-steps N] [--post-steps N] [--head N]
//!   [--value-tables FILE]` — boot to the loader's `jr v0` handoff and map
//!   the post-handoff horizon: executed pages, MMIO over modeled device
//!   windows, DRAM data writes (the framebuffer question, Milestone AE
//!   §33), console delta, head trace (Milestone AD, §32). `--value-tables`
//!   swaps the engine's LZ value ladder from a data file (§35) — the
//!   post-capture procedure, no recompile.
//!
//! Exit codes: 0 = success, 1 = usage/IO error, 2 = emulation stopped on a
//! condition (fault/unknown instruction/step limit).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig, Tracer};
use truman_emulator::{boot, cpu, TARGET_NAME, VERSION};

fn usage() -> String {
    format!(
        "truman-emulator {VERSION} — {TARGET_NAME}\n\
         \n\
         USAGE:\n\
         \x20 truman-emulator inspect <firmware-path>\n\
         \x20 truman-emulator run <firmware-path> [--max-steps N] [--trace-tail N] [--dump PHYS,LEN,PATH] [-v]\n\
         \x20 truman-emulator trace <firmware-path> [--max-steps N] [--limit N] [-v]\n\
         \x20 truman-emulator map <firmware-path> [--pre-steps N] [--post-steps N] [--head N] [--value-tables FILE]\n\
         \n\
         OPTIONS:\n\
         \x20 --max-steps N   instruction budget (default: run = 1_000_000, trace = 1_000)\n\
         \x20 --trace-tail N  run: print last N traced instructions on stop (default 24)\n\
         \x20 --limit N       trace: print first N traced instructions (default 100)\n\
         \x20 --dump P,L,PATH run: write LEN bytes at physical PHYS to PATH (§57)\n\
         \x20 --pre-steps N   map: budget for booting to the handoff (default 2_000_000)\n\
         \x20 --post-steps N  map: post-handoff execution budget (default 1_000_000)\n\
         \x20 --head N        map: trace the first N post-handoff instructions (default 64)\n\
         \x20 --value-tables F  map: engine value ladder from a file (§35; default RFC)\n\
         \x20 -v, --verbose   verbose diagnostics"
    )
}

#[derive(Debug)]
enum ArgsError {
    MissingCommand,
    UnknownCommand(String),
    MissingPath,
    BadOption(String),
    /// The option is recognized but its value/spec is malformed.
    BadValue(String),
    /// A recognized option used on a subcommand that does not implement
    /// it (e.g. `--limit` on `run`). Rejected loudly — every accepted
    /// flag must have an observable effect (Milestone BG, §60).
    WrongCommand(String, String),
    MissingValue(String),
    /// An I/O or content failure on user-supplied input (file missing,
    /// unparseable). Reported as-is — the value was present, so no
    /// usage reprint and no "requires a value" contradiction.
    Io(String),
}

fn parse_path_arg(args: &[String], cmd: &str) -> Result<PathBuf, ArgsError> {
    args.first()
        .map(PathBuf::from)
        .ok_or(ArgsError::MissingPath)
        .inspect_err(|_e| {
            eprintln!("error: `{cmd}` requires a firmware path");
        })
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&raw) {
        Ok(code) => code,
        Err(ArgsError::MissingCommand) => {
            eprintln!("{}", usage());
            ExitCode::from(1)
        }
        Err(ArgsError::UnknownCommand(c)) => {
            eprintln!("error: unknown command `{c}`\n\n{}", usage());
            ExitCode::from(1)
        }
        Err(ArgsError::MissingPath) => ExitCode::from(1),
        Err(ArgsError::BadOption(o)) => {
            eprintln!("error: unknown option `{o}`\n\n{}", usage());
            ExitCode::from(1)
        }
        Err(ArgsError::BadValue(o)) => {
            eprintln!("error: option `{o}` requires a valid value");
            ExitCode::from(1)
        }
        Err(ArgsError::WrongCommand(opt, cmd)) => {
            eprintln!("error: option `{opt}` is not valid for `{cmd}`");
            ExitCode::from(1)
        }
        Err(ArgsError::MissingValue(o)) => {
            eprintln!("error: option `{o}` requires a value\n\n{}", usage());
            ExitCode::from(1)
        }
        Err(ArgsError::Io(msg)) => {
            eprintln!("error: {msg}");
            ExitCode::from(1)
        }
    }
}

fn dispatch(args: &[String]) -> Result<ExitCode, ArgsError> {
    let Some(cmd) = args.first() else {
        return Err(ArgsError::MissingCommand);
    };
    match cmd.as_str() {
        "inspect" => cmd_inspect(&args[1..]),
        "run" => cmd_run(&args[1..]),
        "trace" => cmd_trace(&args[1..]),
        "map" => cmd_map(&args[1..]),
        "--help" | "-h" | "help" => {
            println!("{}", usage());
            Ok(ExitCode::from(0))
        }
        "--version" | "-V" => {
            println!("truman-emulator {VERSION}");
            Ok(ExitCode::from(0))
        }
        other => Err(ArgsError::UnknownCommand(other.to_string())),
    }
}

fn load_and_identify(path: &Path) -> Result<(firmware::Image, firmware::Identification), ExitCode> {
    let img = match firmware::load(path) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error: {e}");
            return Err(ExitCode::from(1));
        }
    };
    match firmware::identify(&img) {
        Ok(id) => Ok((img, id)),
        Err(e) => {
            eprintln!("error: {e}");
            Err(ExitCode::from(1))
        }
    }
}

fn cmd_inspect(args: &[String]) -> Result<ExitCode, ArgsError> {
    let path = parse_path_arg(args, "inspect")?;
    let (img, id) = match load_and_identify(&path) {
        Ok(v) => v,
        Err(c) => return Ok(c),
    };
    println!("file     : {}", img.path.display());
    println!("size     : {} bytes", img.bytes.len());
    println!("sha256   : {}", img.sha256);
    println!("kind     : {}", id.kind);
    println!("soc      : {}", id.soc);
    for n in &id.notes {
        println!("note     : {n}");
    }
    Ok(ExitCode::from(0))
}

/// A flag implemented for exactly one subcommand is rejected — not
/// silently ignored — when it appears on another one. Before Milestone BG
/// the shared run/trace parser accepted `run --limit`, `trace
/// --trace-tail` and `trace --dump` and dropped them on the floor; every
/// accepted flag must have an observable effect (§60).
fn reject_cross_flag(cmd: &str, owner: &str, flag: &str) -> Result<(), ArgsError> {
    if cmd == owner {
        Ok(())
    } else {
        Err(ArgsError::WrongCommand(flag.to_string(), cmd.to_string()))
    }
}

struct RunOptions {
    max_steps: u64,
    trace_tail: usize,
    trace_limit: usize,
    verbose: bool,
    /// `phys,len,path` triples dumped after the run (diagnostics).
    dumps: Vec<(u32, u32, PathBuf)>,
}

fn parse_run_options(
    args: &[String],
    default_steps: u64,
    cmd: &str,
) -> Result<(RunOptions, Vec<String>), ArgsError> {
    let mut opts = RunOptions {
        max_steps: default_steps,
        trace_tail: 24,
        trace_limit: 100,
        verbose: false,
        dumps: Vec::new(),
    };
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--max-steps" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| ArgsError::MissingValue("--max-steps".into()))?;
                opts.max_steps = v
                    .parse()
                    .map_err(|_| ArgsError::BadValue("--max-steps".into()))?;
                i += 2;
            }
            "--trace-tail" => {
                reject_cross_flag(cmd, "run", "--trace-tail")?;
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| ArgsError::MissingValue("--trace-tail".into()))?;
                opts.trace_tail = v
                    .parse()
                    .map_err(|_| ArgsError::BadValue("--trace-tail".into()))?;
                i += 2;
            }
            "--limit" => {
                reject_cross_flag(cmd, "trace", "--limit")?;
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| ArgsError::MissingValue("--limit".into()))?;
                opts.trace_limit = v
                    .parse()
                    .map_err(|_| ArgsError::BadValue("--limit".into()))?;
                i += 2;
            }
            "-v" | "--verbose" => {
                opts.verbose = true;
                i += 1;
            }
            "--dump" => {
                reject_cross_flag(cmd, "run", "--dump")?;
                // Format: phys,len,path (hex numbers, decimal path bytes).
                let spec = args
                    .get(i + 1)
                    .ok_or_else(|| ArgsError::MissingValue("--dump".into()))?;
                let parts: Vec<&str> = spec.splitn(3, ',').collect();
                if parts.len() != 3 {
                    return Err(ArgsError::BadValue(format!("--dump {spec}")));
                }
                let phys = u32::from_str_radix(parts[0].trim_start_matches("0x"), 16)
                    .map_err(|_| ArgsError::BadValue(format!("--dump phys {spec}")))?;
                let len = u32::from_str_radix(parts[1].trim_start_matches("0x"), 16)
                    .map_err(|_| ArgsError::BadValue(format!("--dump len {spec}")))?;
                opts.dumps.push((phys, len, PathBuf::from(parts[2])));
                i += 2;
            }
            other if other.starts_with('-') => return Err(ArgsError::BadOption(other.to_string())),
            other => {
                rest.push(other.to_string());
                i += 1;
            }
        }
    }
    Ok((opts, rest))
}

fn print_state(machine: &Machine) {
    let s = machine.snapshot();
    println!("-- registers --");
    for row in 0..4 {
        let line: Vec<String> = (0..8)
            .map(|c| {
                let i = row * 8 + c;
                format!("{:>4}=0x{:08x}", cpu::mips32::REG_NAMES[i], s.regs[i])
            })
            .collect();
        println!("  {}", line.join("  "));
    }
    println!("  {:>4}=0x{:08x}  {:>4}=0x{:08x}", "hi", s.hi, "lo", s.lo);
    println!("  pc=0x{:08x} next_pc=0x{:08x}", s.pc, s.next_pc);
}

fn print_touchmap(result: &truman_emulator::machine::RunResult) {
    println!("-- instrumented window access map --");
    for (name, offsets) in &result.touchmap {
        if offsets.is_empty() {
            println!("  {name}: (no accesses)");
            continue;
        }
        let shown: Vec<String> = offsets
            .iter()
            .take(24)
            .map(|o| format!("0x{o:04x}"))
            .collect();
        let more = if offsets.len() > 24 {
            format!(" (+{} more)", offsets.len() - 24)
        } else {
            String::new()
        };
        println!(
            "  {}: {} distinct offsets: {}{}",
            name,
            offsets.len(),
            shown.join(" "),
            more
        );
    }
}

fn cmd_run(args: &[String]) -> Result<ExitCode, ArgsError> {
    let (opts, rest) = parse_run_options(args, 1_000_000, "run")?;
    let path = parse_path_arg(&rest, "run")?;
    let (img, id) = match load_and_identify(&path) {
        Ok(v) => v,
        Err(c) => return Ok(c),
    };
    if opts.verbose {
        for n in &id.notes {
            println!("firmware: {n}");
        }
    }
    let mut machine = Machine::new(MachineConfig::default(), &img);
    for line in boot::reset_with_diagnostics(&mut machine, opts.verbose) {
        println!("{line}");
    }

    println!(
        "run: executing up to {} steps (backend: {})…",
        opts.max_steps,
        machine.cpu.name()
    );
    let result = machine.run(opts.max_steps);
    println!("run: stopped after {} steps: {}", result.steps, result.stop);
    println!("run: final pc=0x{:08x}", result.pc);
    for (phys, len, path) in &opts.dumps {
        let mut buf = Vec::with_capacity(*len as usize);
        let mut a = *phys;
        while a < phys.saturating_add(*len) {
            let v = machine
                .bus
                .peek_phys(a, truman_emulator::memory::AccessSize::Byte)
                .unwrap_or(0);
            buf.push(v as u8);
            a += 1;
        }
        match std::fs::write(path, &buf) {
            Ok(()) => println!("dump: 0x{phys:08x}+0x{len:x} -> {}", path.display()),
            Err(e) => println!(
                "dump: 0x{phys:08x}+0x{len:x} -> {} FAILED: {e}",
                path.display()
            ),
        }
    }
    let console = machine.uart_console();
    if !console.is_empty() {
        println!("-- serial console (UART @ 0xBFFE0900) --");
        for line in console.lines() {
            println!("  | {line}");
        }
        if !console.ends_with('\n') {
            println!();
        }
    } else {
        println!("run: serial console: (no UART output)");
    }
    print_touchmap(&result);

    if opts.trace_tail > 0 {
        // Re-run a short bounded window to capture the final instruction
        // sequence for diagnosis. Deterministic: same start state, so the
        // tail matches the run exactly.
        let tail_steps = opts.trace_tail.min(result.steps.max(1) as usize) as u64;
        let mut machine2 = Machine::new(MachineConfig::default(), &img);
        machine2.reset();
        let budget = result.steps.saturating_sub(tail_steps);
        let _ = machine2.run(budget);
        let mut tracer = Tracer::new(tail_steps as usize + 8);
        let _ = machine2.run_traced(tail_steps, &mut tracer);
        println!("-- last {} instructions --", tracer.total());
        for r in tracer.drain() {
            print_step(&r);
        }
    }
    print_state(&machine);

    match result.stop {
        truman_emulator::machine::StopReason::StepLimit => Ok(ExitCode::from(0)),
        truman_emulator::machine::StopReason::Cpu(_) => Ok(ExitCode::from(2)),
        truman_emulator::machine::StopReason::EntryReached { .. } => Ok(ExitCode::from(0)),
    }
}

fn print_step(r: &cpu::StepReport) {
    let mems = if r.mem.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = r
            .mem
            .iter()
            .map(|m| {
                format!(
                    "{} 0x{:08x}/{:x}=0x{:08x}",
                    if m.write { "W" } else { "R" },
                    m.vaddr,
                    m.size.bytes(),
                    m.value
                )
            })
            .collect();
        format!("  | {}", parts.join(" "))
    };
    println!(
        "  pc=0x{:08x}  {:08x}  {:<28}{}",
        r.pc, r.word, r.desc, mems
    );
}

fn cmd_trace(args: &[String]) -> Result<ExitCode, ArgsError> {
    let (opts, rest) = parse_run_options(args, 1_000, "trace")?;
    let path = parse_path_arg(&rest, "trace")?;
    let (img, _id) = match load_and_identify(&path) {
        Ok(v) => v,
        Err(c) => return Ok(c),
    };
    let mut machine = Machine::new(MachineConfig::default(), &img);
    for line in boot::reset_with_diagnostics(&mut machine, opts.verbose) {
        println!("{line}");
    }
    let mut tracer = Tracer::new(opts.trace_limit);
    let result = machine.run_traced(opts.max_steps, &mut tracer);
    println!(
        "trace: executed {} instructions (limit {}), stop: {}",
        result.steps, opts.max_steps, result.stop
    );
    println!("trace: final pc=0x{:08x}", result.pc);
    let console = machine.uart_console();
    if !console.is_empty() {
        println!("-- serial console (UART @ 0xBFFE0900) --");
        for line in console.lines() {
            println!("  | {line}");
        }
        if !console.ends_with('\n') {
            println!();
        }
    }
    let head = tracer.drain();
    println!("-- first {} instructions --", head.len());
    for r in &head {
        print_step(r);
    }
    print_touchmap(&result);
    print_state(&machine);
    match result.stop {
        truman_emulator::machine::StopReason::StepLimit => Ok(ExitCode::from(0)),
        truman_emulator::machine::StopReason::Cpu(_) => Ok(ExitCode::from(2)),
        truman_emulator::machine::StopReason::EntryReached { .. } => Ok(ExitCode::from(0)),
    }
}

struct MapOptions {
    pre_steps: u64,
    post_steps: u64,
    head: usize,
    verbose: bool,
    /// The `--value-tables` file, when given (§35): loads a non-default
    /// engine value ladder into [`MachineConfig::value_tables`].
    value_tables: Option<PathBuf>,
}

fn parse_map_options(args: &[String]) -> Result<(MapOptions, Vec<String>), ArgsError> {
    let mut opts = MapOptions {
        pre_steps: 2_000_000,
        post_steps: 1_000_000,
        head: 64,
        verbose: false,
        value_tables: None,
    };
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let take = |name: &str, slot: &mut u64| -> Result<(), ArgsError> {
            let v = args
                .get(i + 1)
                .ok_or_else(|| ArgsError::MissingValue(name.to_string()))?;
            *slot = v
                .parse()
                .map_err(|_| ArgsError::BadValue(name.to_string()))?;
            Ok(())
        };
        match args[i].as_str() {
            "--pre-steps" => {
                take("--pre-steps", &mut opts.pre_steps)?;
                i += 2;
            }
            "--post-steps" => {
                take("--post-steps", &mut opts.post_steps)?;
                i += 2;
            }
            "--head" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| ArgsError::MissingValue("--head".into()))?;
                opts.head = v
                    .parse()
                    .map_err(|_| ArgsError::BadValue("--head".into()))?;
                i += 2;
            }
            "--value-tables" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| ArgsError::MissingValue("--value-tables".into()))?;
                opts.value_tables = Some(PathBuf::from(v));
                i += 2;
            }
            "-v" | "--verbose" => {
                opts.verbose = true;
                i += 1;
            }
            other if other.starts_with('-') => return Err(ArgsError::BadOption(other.to_string())),
            other => {
                rest.push(other.to_string());
                i += 1;
            }
        }
    }
    Ok((opts, rest))
}

/// `map`: boot to the loader's `jr v0` handoff and map what executes after
/// it (Milestone AD, §32). Evidence-only output; exit 2 when the post
/// window ends on a CPU condition or the handoff is not reached, 0 when it
/// ends on the step budget.
fn cmd_map(args: &[String]) -> Result<ExitCode, ArgsError> {
    let (opts, rest) = parse_map_options(args)?;
    let path = parse_path_arg(&rest, "map")?;
    let (img, _id) = match load_and_identify(&path) {
        Ok(v) => v,
        Err(c) => return Ok(c),
    };
    let mut config = MachineConfig::default();
    if let Some(vt_path) = &opts.value_tables {
        let raw = std::fs::read(vt_path).map_err(|e| {
            ArgsError::Io(format!(
                "cannot read value tables {}: {e}",
                vt_path.display()
            ))
        })?;
        let tables = truman_emulator::inflate::ValueTables::from_bytes(&raw)
            .map_err(|e| ArgsError::Io(format!("value tables {}: {e}", vt_path.display())))?;
        println!(
            "map: value ladder loaded from {} (non-default engine semantics, §35)",
            vt_path.display()
        );
        config.value_tables = tables;
    }
    let mut machine = Machine::new(config, &img);
    for line in boot::reset_with_diagnostics(&mut machine, opts.verbose) {
        println!("{line}");
    }
    println!(
        "map: boot to handoff (budget {}), then observe up to {} steps…",
        opts.pre_steps, opts.post_steps
    );
    let map = machine.map_handoff(opts.pre_steps, opts.post_steps, opts.head);

    if !map.handoff_reached {
        println!(
            "map: handoff NOT reached within {} steps (stop: {:?}, pc=0x{:08x})",
            opts.pre_steps, map.post_stop, map.final_pc
        );
        return Ok(ExitCode::from(2));
    }
    let regs = map
        .handoff_regs
        .as_ref()
        .expect("handoff reached implies a snapshot");
    println!(
        "map: handoff at step {} -> pc=0x{:08x}  v0=0x{:08x} v1=0x{:08x} a0=0x{:08x} sp=0x{:08x}",
        map.pre_steps, map.handoff_pc, regs.regs[2], regs.regs[3], regs.regs[4], regs.regs[29]
    );
    match &map.post_stop {
        Some(stop) => println!(
            "map: post-handoff {} steps, stop: {stop}, final pc=0x{:08x}",
            map.post_steps, map.final_pc
        ),
        None => println!("map: post-handoff budget exhausted before execution"),
    }

    if map.exec_pages.is_empty() {
        println!("map: executed pages: (none)");
    } else {
        println!("-- executed pages (4 KiB, top) --");
        for (base, count) in map.exec_pages.iter().take(8) {
            println!("  0x{base:08x}  x{count}");
        }
        if map.exec_pages.len() > 8 {
            println!("  (+{} more)", map.exec_pages.len() - 8);
        }
    }

    if map.mmio.is_empty() {
        println!("map: post-handoff MMIO: (no accesses over modeled device windows)");
    } else {
        println!("-- post-handoff MMIO ({} accesses) --", map.mmio.len());
        if map.mmio_overflow {
            println!("  (watch cap hit — record truncated)");
        }
        for (step, pc, phys, kind, size, value) in map.mmio.iter().take(64) {
            println!(
                "  step {step:>7}  pc=0x{pc:08x}  {} 0x{phys:08x}/{:x} = 0x{value:08x}",
                match kind {
                    truman_emulator::memory::WatchKind::Read => "R",
                    truman_emulator::memory::WatchKind::Write => "W",
                    truman_emulator::memory::WatchKind::Fetch => "F",
                },
                size.bytes()
            );
        }
        if map.mmio.len() > 64 {
            println!("  (+{} more)", map.mmio.len() - 64);
        }
    }

    if map.writes_total == 0 {
        println!("map: post-handoff data writes: (no DRAM stores)");
    } else {
        println!(
            "-- post-handoff data writes ({} stores, {} distinct 4 KiB pages) --",
            map.writes_total,
            map.write_pages.len()
        );
        if map.mmio_overflow {
            println!("  (watch cap hit — record truncated)");
        }
        for (base, count) in map.write_pages.iter().take(8) {
            println!("  0x{base:08x}  x{count}");
        }
        if map.write_pages.len() > 8 {
            println!("  (+{} more)", map.write_pages.len() - 8);
        }
        println!(
            "-- first {} post-handoff data stores --",
            map.write_sample.len()
        );
        for w in &map.write_sample {
            println!(
                "  step {:>7}  pc=0x{:08x}  W 0x{:08x} (vaddr 0x{:08x})/{:x} = 0x{:08x}",
                w.step,
                w.pc,
                w.phys,
                w.vaddr,
                w.size.bytes(),
                w.value
            );
        }
    }

    if map.console_after.is_empty() {
        println!("map: console after handoff: (no UART output)");
    } else {
        println!("-- serial console after handoff --");
        for line in map.console_after.lines() {
            println!("  | {line}");
        }
        if !map.console_after.ends_with('\n') {
            println!();
        }
    }

    if !map.head.is_empty() {
        println!("-- first {} post-handoff instructions --", map.head.len());
        for r in &map.head {
            print_step(r);
        }
    }

    match map.post_stop {
        Some(truman_emulator::machine::StopReason::StepLimit) | None => Ok(ExitCode::from(0)),
        Some(truman_emulator::machine::StopReason::Cpu(_)) => Ok(ExitCode::from(2)),
        Some(truman_emulator::machine::StopReason::EntryReached { .. }) => Ok(ExitCode::from(0)),
    }
}
