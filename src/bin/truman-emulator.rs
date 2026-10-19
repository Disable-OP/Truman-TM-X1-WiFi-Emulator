//! `truman-emulator` command-line frontend.
//!
//! Subcommands:
//! - `inspect <firmware>` — identify and validate the image, print findings
//! - `run <firmware> [--max-steps N] [--trace-tail N] [-v]` — boot and execute
//! - `trace <firmware> [--max-steps N] [--limit N] [-v]` — boot and print a
//!   per-instruction trace of the first N steps
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
         \x20 truman-emulator run <firmware-path> [--max-steps N] [--trace-tail N] [-v]\n\
         \x20 truman-emulator trace <firmware-path> [--max-steps N] [--limit N] [-v]\n\
         \n\
         OPTIONS:\n\
         \x20 --max-steps N   instruction budget (default: run = 1_000_000, trace = 1_000)\n\
         \x20 --trace-tail N  print last N traced instructions on stop (default 24)\n\
         \x20 --limit N       print first N traced instructions (default 100)\n\
         \x20 -v              verbose diagnostics"
    )
}

#[derive(Debug)]
enum ArgsError {
    MissingCommand,
    UnknownCommand(String),
    MissingPath,
    BadOption(String),
    MissingValue(String),
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
        Err(ArgsError::MissingValue(o)) => {
            eprintln!("error: option `{o}` requires a value\n\n{}", usage());
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
                    .map_err(|_| ArgsError::BadOption("--max-steps".into()))?;
                i += 2;
            }
            "--trace-tail" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| ArgsError::MissingValue("--trace-tail".into()))?;
                opts.trace_tail = v
                    .parse()
                    .map_err(|_| ArgsError::BadOption("--trace-tail".into()))?;
                i += 2;
            }
            "--limit" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| ArgsError::MissingValue("--limit".into()))?;
                opts.trace_limit = v
                    .parse()
                    .map_err(|_| ArgsError::BadOption("--limit".into()))?;
                i += 2;
            }
            "-v" | "--verbose" => {
                opts.verbose = true;
                i += 1;
            }
            "--dump" => {
                // Format: phys,len,path (hex numbers, decimal path bytes).
                let spec = args
                    .get(i + 1)
                    .ok_or_else(|| ArgsError::MissingValue("--dump".into()))?;
                let parts: Vec<&str> = spec.splitn(3, ',').collect();
                if parts.len() != 3 {
                    return Err(ArgsError::BadOption(format!("--dump {spec}")));
                }
                let phys = u32::from_str_radix(parts[0].trim_start_matches("0x"), 16)
                    .map_err(|_| ArgsError::BadOption(format!("--dump phys {spec}")))?;
                let len = u32::from_str_radix(parts[1].trim_start_matches("0x"), 16)
                    .map_err(|_| ArgsError::BadOption(format!("--dump len {spec}")))?;
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
    let (opts, rest) = parse_run_options(args, 1_000_000)?;
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
    let (opts, rest) = parse_run_options(args, 1_000)?;
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
    println!("-- first {} instructions --", tracer.total());
    for r in tracer.drain() {
        print_step(&r);
    }
    print_touchmap(&result);
    print_state(&machine);
    match result.stop {
        truman_emulator::machine::StopReason::StepLimit => Ok(ExitCode::from(0)),
        truman_emulator::machine::StopReason::Cpu(_) => Ok(ExitCode::from(2)),
    }
}
