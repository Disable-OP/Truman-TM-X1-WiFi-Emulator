//! CLI success-path output contract (Milestone BC, §56).
//!
//! Milestone AZ pinned the usage/dispatch surface, Milestone BB pinned the
//! error paths and the two stop-reason exit codes; this file pins what the
//! commands print **when they succeed**, so the report formats cannot drift
//! silently either. All pins run against the real firmware image and are
//! deterministic (fixed start state, no time dependence):
//!
//! - `inspect`: the eight report lines, the SHA-256 anchor, the three notes.
//! - `trace` (5 steps): the boot diagnostics, the stop report, the first
//!   five decoded instructions verbatim, the all-empty window map, the
//!   register footer.
//! - `trace` head honesty (the §56 defect): `--limit` must cap the printed
//!   head to the **first** N instructions while the executed count still
//!   reports every step. Before the fix a full ring buffer printed the
//!   *last* N steps under a "first" heading — this test is its regression
//!   pin, and it also pins prefix stability (the head of a 300-step trace
//!   is the 5-step trace).
//! - `run` (1000 steps): the boot diagnostics, the stop report, the final
//!   pc pin, the UART banner content, the uart window's access line, the
//!   default 24-instruction tail block and the register footer.
//! - `run` flag policies (Milestone BG, §60): duplicate numeric flags are
//!   last-wins, and `--verbose` is a byte-identical long form of `-v`.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_truman-emulator");
const FW: &str = "firmware";

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Out {
    let out = Command::new(BIN).args(args).output().expect("spawn binary");
    Out {
        code: out.status.code().expect("exit code"),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn firmware_path() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push(FW);
    let mut entries = std::fs::read_dir(&p).expect("firmware dir");
    let bin = entries
        .find(|e| {
            e.as_ref()
                .ok()
                .and_then(|e| e.path().extension().map(|x| x == "bin"))
                .unwrap_or(false)
        })
        .expect("firmware image")
        .unwrap()
        .path();
    bin
}

/// The two boot diagnostics every booting command prints first.
const BOOT_RESET_LINE: &str =
    "boot: reset pc=0xbfc00000 (hypothesis A: flash XIP at MIPS boot window)";
const BOOT_STACK_LINE: &str = "boot: firmware-provided stack top will be 0xbffc0e00";

/// The first five executed instructions from reset, verbatim (trailing
/// format padding trimmed). Pinned by the 5-step trace and re-checked as
/// the head of the 300-step trace (prefix stability).
const FIRST_FIVE: [&str; 5] = [
    "pc=0xbfc00000  24006fb5  addiu zero, zero, +28597",
    "pc=0xbfc00004  3c1a0819  lui k0, 0x0819",
    "pc=0xbfc00008  375a2011  ori k0, k0, 0x2011",
    "pc=0xbfc0000c  3c1a5553  lui k0, 0x5553",
    "pc=0xbfc00010  375a504e  ori k0, k0, 0x504e",
];

/// The modeled window names, in the order `print_touchmap` lists them.
const WINDOW_NAMES: [&str; 19] = [
    "iram-overlay",
    "intc",
    "timer",
    "sysreg-params",
    "sysreg-params-mid0",
    "sysreg-params-mid",
    "uart",
    "sysreg-params-hi",
    "ddrphy",
    "ddrctrl",
    "keyport",
    "decomp-engine",
    "sysreg-params-hi2",
    "sysreg-params-hi2b",
    "spi",
    "sysreg-params-top",
    "win-0x18000000",
    "win-0x10000000",
    "win-0x30000000",
];

fn lines(stdout: &str) -> Vec<&str> {
    stdout.lines().map(|l| l.trim_end()).collect()
}

/// Lines strictly between the two section headers (first occurrence each).
fn section<'a>(ls: &'a [&'a str], start: &str, end: &str) -> Vec<&'a str> {
    let s = ls
        .iter()
        .position(|l| *l == start)
        .unwrap_or_else(|| panic!("missing section {start:?}: {ls:?}"));
    let e = ls[s + 1..]
        .iter()
        .position(|l| *l == end)
        .map(|p| s + 1 + p)
        .unwrap_or(ls.len());
    ls[s + 1..e].to_vec()
}

#[test]
fn inspect_pins_the_eight_report_lines() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    let o = run(&["inspect", fw]);
    assert_eq!(o.code, 0, "stderr: {}", o.stderr);
    let ls = lines(&o.stdout);
    assert_eq!(ls.len(), 8, "inspect prints exactly eight lines: {ls:?}");
    assert!(
        ls[0].starts_with("file     : ") && ls[0].ends_with(".bin"),
        "file line carries the path: {}",
        ls[0]
    );
    assert_eq!(ls[1], "size     : 4194304 bytes");
    assert_eq!(
        ls[2],
        "sha256   : 5ca012b28e2a70e322a48d513d2a3011a0778a7afa7ecd53e4aad102c2cd28f0"
    );
    assert_eq!(
        ls[3],
        "kind     : raw SPI NOR flash dump (4 MiB, full image)"
    );
    assert_eq!(ls[4], "soc      : Sunplus SPHE15xx (probable SPHE1506HV)");
    assert_eq!(
        ls[5],
        "note     : stage-1 loader: zeroBoot v1.0 for 1506F (flash 0x0000)"
    );
    assert_eq!(
        ls[6],
        "note     : stage-2 loader: Boot form SPI v1.13 (flash 0x8000)"
    );
    assert_eq!(ls[7], "note     : SoC descriptor: SPHE15 (flash 0x38b000)");
}

#[test]
fn trace_pins_the_first_instructions_from_reset() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    let o = run(&["trace", fw, "--max-steps", "5"]);
    assert_eq!(o.code, 0, "stderr: {}", o.stderr);
    let ls = lines(&o.stdout);
    assert_eq!(ls[0], BOOT_RESET_LINE, "boot diagnostics lead the report");
    assert_eq!(ls[1], BOOT_STACK_LINE);
    assert!(ls.contains(&"trace: executed 5 instructions (limit 5), stop: step limit reached"));
    assert!(ls.contains(&"trace: final pc=0xbfc00014"));
    let head = section(
        &ls,
        "-- first 5 instructions --",
        "-- instrumented window access map --",
    );
    assert_eq!(head.len(), 5, "exactly five step lines: {head:?}");
    for (got, want) in head.iter().zip(FIRST_FIVE.iter()) {
        assert_eq!(
            got.trim_start(),
            *want,
            "the decode lines are pinned verbatim"
        );
    }
    // Five steps in, no modeled window has been touched: the map is the
    // all-empty block, names in the fixed order.
    let map = section(
        &ls,
        "-- instrumented window access map --",
        "-- registers --",
    );
    assert_eq!(map.len(), WINDOW_NAMES.len(), "map: {map:?}");
    for (got, name) in map.iter().zip(WINDOW_NAMES.iter()) {
        assert_eq!(*got, format!("  {name}: (no accesses)"), "map: {map:?}");
    }
    // Register footer pins: the five steps landed k0 = 0x5553504e
    // (lui 0x5553 / ori 0x504e — the magic from the reset vector block).
    assert!(
        ls.iter().any(|l| l.contains("k0=0x5553504e")),
        "k0 carries the lui/ori constant: {ls:?}"
    );
    assert!(ls.contains(&"  pc=0xbfc00014 next_pc=0xbfc00018"));
}

#[test]
fn trace_limit_caps_the_printed_head_and_keeps_the_count() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    // §56 defect pin: default --limit is 100, so a 300-step trace must
    // print the FIRST 100 steps — not the last 100 mislabelled as "first"
    // (what a full ring buffer produced before the fix).
    let o = run(&["trace", fw, "--max-steps", "300"]);
    assert_eq!(o.code, 0, "stderr: {}", o.stderr);
    let ls = lines(&o.stdout);
    assert!(
        ls.contains(&"trace: executed 300 instructions (limit 300), stop: step limit reached"),
        "the executed count still reports every step: {ls:?}"
    );
    let head = section(
        &ls,
        "-- first 100 instructions --",
        "-- instrumented window access map --",
    );
    assert_eq!(
        head.len(),
        100,
        "the head is capped at the limit: {}",
        head.len()
    );
    // Prefix stability: the head of a longer trace is the short trace.
    for (got, want) in head.iter().take(5).zip(FIRST_FIVE.iter()) {
        assert_eq!(
            got.trim_start(),
            *want,
            "the head is the first instructions, prefix-stable"
        );
    }
    // And an explicit smaller limit caps likewise.
    let o = run(&["trace", fw, "--max-steps", "40", "--limit", "10"]);
    assert_eq!(o.code, 0, "stderr: {}", o.stderr);
    let ls = lines(&o.stdout);
    assert!(ls.contains(&"trace: executed 40 instructions (limit 40), stop: step limit reached"));
    let head = section(
        &ls,
        "-- first 10 instructions --",
        "-- instrumented window access map --",
    );
    assert_eq!(head.len(), 10, "limit 10 prints ten steps: {}", head.len());
}

#[test]
fn run_pins_boot_stop_console_tail_and_registers() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    let o = run(&["run", fw, "--max-steps", "1000"]);
    assert_eq!(o.code, 0, "stderr: {}", o.stderr);
    let ls = lines(&o.stdout);
    assert_eq!(ls[0], BOOT_RESET_LINE);
    assert_eq!(ls[1], BOOT_STACK_LINE);
    assert!(ls.contains(&"run: executing up to 1000 steps (backend: mips32r2-le-interpreter)…"));
    assert!(ls.contains(&"run: stopped after 1000 steps: step limit reached"));
    assert!(ls.contains(&"run: final pc=0xb8000100"));
    // The stage-1 banner reached the UART console within the budget.
    assert!(ls.contains(&"-- serial console (UART @ 0xBFFE0900) --"));
    assert!(
        ls.iter().any(|l| l.contains("zeroBoot v1.0 for 1506F")),
        "banner line present: {ls:?}"
    );
    assert!(ls.iter().any(|l| l.contains("SPI SCS flow")));
    // The uart window saw exactly its four register offsets in this window.
    assert!(
        ls.contains(&"  uart: 4 distinct offsets: 0x0000 0x0004 0x0014 0x0018"),
        "uart access line: {ls:?}"
    );
    // Default tail: 24 traced instructions; the last tail step and the
    // register footer agree with the final pc pin.
    let tail = section(&ls, "-- last 24 instructions --", "-- registers --");
    assert_eq!(tail.len(), 24, "tail: {}", tail.len());
    assert_eq!(
        tail.last().expect("tail lines").trim_start(),
        "pc=0xb80000fc  21efffff  addiu t7, t7, -1"
    );
    assert!(ls.contains(&"  pc=0xb8000100 next_pc=0xb8000104"));
}

#[test]
fn run_trace_tail_honors_an_explicit_count() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    // BC pinned the default 24-line tail; the explicit count must be
    // honored likewise (§58 companion pin).
    let o = run(&["run", fw, "--max-steps", "1000", "--trace-tail", "5"]);
    assert_eq!(o.code, 0, "stderr: {}", o.stderr);
    let ls = lines(&o.stdout);
    assert!(
        ls.contains(&"-- last 5 instructions --"),
        "tail header: {ls:?}"
    );
    let tail = section(&ls, "-- last 5 instructions --", "-- registers --");
    assert_eq!(tail.len(), 5, "five tail lines: {tail:?}");
    // The 5-line tail is the suffix of the default 24-line window: its
    // last line is the same pinned final step.
    assert_eq!(
        tail.last().expect("tail lines").trim_start(),
        "pc=0xb80000fc  21efffff  addiu t7, t7, -1"
    );
    // And tail 0 disables the block entirely.
    let o = run(&["run", fw, "--max-steps", "1000", "--trace-tail", "0"]);
    assert_eq!(o.code, 0);
    assert!(
        !o.stdout.contains("-- last"),
        "no tail block when 0: {}",
        o.stdout
    );
}

#[test]
fn duplicate_flags_are_last_wins() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    // Milestone BG (§60): the parser's duplicate-flag policy is last-wins,
    // pinned so it cannot flip silently. Both budgets are below the
    // handoff, so the run stops on the budget and exits 0.
    let o = run(&["run", fw, "--max-steps", "40", "--max-steps", "60"]);
    assert_eq!(o.code, 0, "stdout: {}\nstderr: {}", o.stdout, o.stderr);
    assert!(
        o.stdout.contains("run: executing up to 60 steps"),
        "the effective budget must be the last value: {}",
        o.stdout
    );
    assert!(
        !o.stdout.contains("up to 40"),
        "the earlier duplicate must not win: {}",
        o.stdout
    );
    assert!(
        o.stdout.contains("stopped after 60 steps"),
        "the executed count must match the last value: {}",
        o.stdout
    );
}

#[test]
fn verbose_long_form_is_identical_to_the_short_form() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    // Milestone BG (§60): `--verbose` is the documented long form of `-v`
    // (now listed in the usage OPTIONS block). Both must produce
    // byte-identical reports on the deterministic 5-step window, including
    // the three `firmware:` identification notes.
    let short = run(&["run", fw, "--max-steps", "5", "-v"]);
    let long = run(&["run", fw, "--max-steps", "5", "--verbose"]);
    assert_eq!(short.code, 0, "stdout: {}", short.stdout);
    assert_eq!(long.code, 0, "stdout: {}", long.stdout);
    assert_eq!(
        short.stdout, long.stdout,
        "-v and --verbose must be indistinguishable"
    );
    let notes = short
        .stdout
        .lines()
        .filter(|l| l.starts_with("firmware: "))
        .count();
    assert_eq!(
        notes, 3,
        "the three identification notes must lead: {}",
        short.stdout
    );
}
