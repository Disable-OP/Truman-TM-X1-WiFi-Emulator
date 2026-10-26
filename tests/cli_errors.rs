//! CLI error-path contract, pinned end-to-end (Milestone BB, §55).
//!
//! The task contract requires testing invalid firmware, bounds and step
//! limits; the validation layer has unit tests (`src/firmware.rs`), but the
//! CLI's observable contract — exit codes and error messages for bad input —
//! was only checked by hand. docs/hardware-session.md *documents* exit-code
//! contracts (usage = 2 for the solve examples; the main CLI's stop-reason
//! mapping: StepLimit = 0, CPU fault = 2), so the surface is contractual
//! and must not drift silently. This file pins the main binary's behavior:
//!
//! - nonexistent file → exit 1, `error:` on stderr, no panic
//! - junk file, wrong size → exit 1 (size rejection)
//! - junk file, exact 4 MiB size → exit 1 (prologue/banner rejection)
//! - unknown subcommand → exit 1, "unknown command"
//! - missing path → exit 1
//! - unrecognized option → exit 1, "unknown option"
//! - malformed option value → exit 1, "requires a valid value" (the
//!   `BadValue` distinction, Milestone BB)
//! - step limit on the real firmware (--max-steps below the handoff) →
//!   exit 0: a budget stop is a *successful bounded run*
//! - CPU-fault stop on the real firmware (--max-steps past the handoff
//!   trap) → exit 2: the trap is a *failure*, deterministically
//! - cross-flag use (Milestone BG, §60): a flag implemented for exactly
//!   one subcommand is rejected on the other (`run --limit`,
//!   `trace --trace-tail`, `trace --dump`) — exit 1, no execution, the
//!   shared run/trace parser no longer silently drops such flags
//!
//! The two real-firmware runs reuse the boot chain the acceptance tests
//! already execute; both are bounded and deterministic (same pins as
//! `tests/hwsession.rs`).

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

#[test]
fn nonexistent_file_is_a_clean_error_not_a_panic() {
    let o = run(&["inspect", "/nonexistent/truman/missing.bin"]);
    assert_eq!(o.code, 1, "stdout: {}\nstderr: {}", o.stdout, o.stderr);
    assert!(o.stderr.starts_with("error:"), "stderr: {}", o.stderr);
    assert!(!o.stderr.contains("panicked"), "must not panic");
}

#[test]
fn junk_file_wrong_size_is_rejected() {
    let path = std::env::temp_dir().join("truman-cli-err-tiny.bin");
    std::fs::write(&path, vec![0u8; 4096]).expect("write junk");
    let o = run(&["inspect", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(o.code, 1, "stderr: {}", o.stderr);
    assert!(o.stderr.starts_with("error:"), "stderr: {}", o.stderr);
}

#[test]
fn junk_file_exact_size_is_rejected_by_content() {
    // Same size as the real image (4 MiB), garbage content: the size gate
    // passes, the prologue/banner gate must reject.
    let path = std::env::temp_dir().join("truman-cli-err-4m.bin");
    let mut junk = vec![0x5Au8; 4 * 1024 * 1024];
    junk[0] = 0x00; // not the real image's first byte
    std::fs::write(&path, &junk).expect("write junk");
    let o = run(&["inspect", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(o.code, 1, "stderr: {}", o.stderr);
    assert!(o.stderr.starts_with("error:"), "stderr: {}", o.stderr);
}

#[test]
fn unknown_command_and_options_report_cleanly() {
    let o = run(&["frobnicate"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("unknown command `frobnicate`"));
    assert!(o.stderr.contains("USAGE"), "must reprint usage");

    let o = run(&["run", "--no-such-flag"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("unknown option `--no-such-flag`"));
}

#[test]
fn missing_path_and_malformed_value_report_cleanly() {
    let o = run(&["inspect"]);
    assert_eq!(o.code, 1);

    // BadValue: the option is recognized, the value is malformed — its own
    // message, no usage reprint (Milestone BB).
    let o = run(&["run", FW, "--max-steps", "abc"]);
    assert_eq!(o.code, 1, "stderr: {}", o.stderr);
    assert!(
        o.stderr.contains("requires a valid value"),
        "stderr: {}",
        o.stderr
    );
    assert!(
        !o.stderr.contains("unknown option"),
        "bad values must not be misreported as unknown options: {}",
        o.stderr
    );
}

#[test]
fn step_limit_below_the_handoff_is_a_successful_bounded_run() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    // Well below the 1,301,783-step handoff: the run must stop on the
    // budget (StepLimit) and exit 0 — a bounded run is a success.
    let o = run(&["run", fw, "--max-steps", "100000"]);
    assert_eq!(o.code, 0, "stdout: {}\nstderr: {}", o.stdout, o.stderr);
    assert!(
        o.stdout.contains("100000"),
        "the stop report must carry the budget: {}",
        o.stdout
    );
}

#[test]
fn cpu_fault_at_the_trap_is_exit_2() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    // Past the handoff (1,301,783) into the pinned trap at 0xA1600050:
    // StopReason::Cpu must map to exit 2 — deterministically, same pin as
    // the acceptance tests.
    let o = run(&["run", fw, "--max-steps", "1310000"]);
    assert_eq!(o.code, 2, "stdout: {}\nstderr: {}", o.stdout, o.stderr);
    assert!(
        o.stdout.contains("0xa1600050"),
        "the stop report must carry the trap pc: {}",
        o.stdout
    );
    assert!(
        o.stdout.contains("1301783"),
        "the stop report must carry the step count: {}",
        o.stdout
    );
}

#[test]
fn run_rejects_the_trace_only_limit_flag() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    // Milestone BG (§60): the shared run/trace parser used to accept
    // `run --limit` and silently drop it. Every accepted flag must have
    // an observable effect; cross-flag use is rejected before anything
    // executes (no boot diagnostics on stdout, exit 1).
    let o = run(&["run", fw, "--limit", "10"]);
    assert_eq!(o.code, 1, "stdout: {}\nstderr: {}", o.stdout, o.stderr);
    assert!(
        o.stderr
            .contains("error: option `--limit` is not valid for `run`"),
        "stderr: {}",
        o.stderr
    );
    assert!(
        o.stdout.is_empty(),
        "rejection must precede execution, stdout: {:?}",
        o.stdout
    );
}

#[test]
fn trace_rejects_the_run_only_trace_tail_flag() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    let o = run(&["trace", fw, "--trace-tail", "5"]);
    assert_eq!(o.code, 1, "stdout: {}\nstderr: {}", o.stdout, o.stderr);
    assert!(
        o.stderr
            .contains("error: option `--trace-tail` is not valid for `trace`"),
        "stderr: {}",
        o.stderr
    );
    assert!(
        o.stdout.is_empty(),
        "rejection must precede execution, stdout: {:?}",
        o.stdout
    );
}

#[test]
fn trace_rejects_the_run_only_dump_flag() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    let o = run(&["trace", fw, "--dump", "0x1fc00000,0x4,window.bin"]);
    assert_eq!(o.code, 1, "stdout: {}\nstderr: {}", o.stdout, o.stderr);
    assert!(
        o.stderr
            .contains("error: option `--dump` is not valid for `trace`"),
        "stderr: {}",
        o.stderr
    );
    assert!(
        o.stdout.is_empty(),
        "rejection must precede execution, stdout: {:?}",
        o.stdout
    );
}
