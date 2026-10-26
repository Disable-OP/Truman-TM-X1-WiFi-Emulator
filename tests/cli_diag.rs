//! `map` success-path contract and the diagnostics seam (Milestone BD, §57).
//!
//! Milestone BC pinned the success reports of `inspect`/`trace`/`run`;
//! the fourth subcommand, `map`, was still unpinned — as were the
//! diagnostics flags the hardware-session procedure leans on: `--dump`
//! (byte-window dump after a run) and `-v` (identification notes). All
//! pins run against the real firmware image and are deterministic:
//!
//! - `map` boots to the loader's `jr v0` handoff (step 1,301,763 — the
//!   acceptance boot chain continues 20 steps further to the pinned trap
//!   at 1,301,783) and maps the post window: the handoff registers, the
//!   Cpu stop at 0xa1600050, the executed pages, the empty MMIO/write/
//!   console sections and the head trace. Exit 2 (post window ends on a
//!   CPU condition, same mapping as run).
//! - `--dump phys,len,path` writes exactly the requested physical window
//!   (the boot window 0x1fc00000 reads back the firmware image head) and
//!   reports an unwritable target with a `FAILED:` line without failing
//!   the run (exit stays 0).
//! - `-v` prints the three identification notes as `firmware:` lines
//!   before the boot diagnostics; without it they are absent.

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

const BOOT_RESET_LINE: &str =
    "boot: reset pc=0xbfc00000 (hypothesis A: flash XIP at MIPS boot window)";
const BOOT_STACK_LINE: &str = "boot: firmware-provided stack top will be 0xbffc0e00";

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
fn map_pins_the_handoff_and_the_post_window() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    let o = run(&["map", fw, "--post-steps", "2000"]);
    assert_eq!(o.code, 2, "the post window ends on the trap: {}", o.stdout);
    let ls = lines(&o.stdout);
    assert_eq!(ls[0], BOOT_RESET_LINE);
    assert_eq!(ls[1], BOOT_STACK_LINE);
    assert!(ls.contains(&"map: boot to handoff (budget 2000000), then observe up to 2000 steps…"));
    // The jr v0 handoff: step pin and the register pin. The acceptance
    // boot chain (§54) continues 20 steps past it to the trap at 1301783.
    assert!(ls.contains(
        &"map: handoff at step 1301763 -> pc=0xa1600000  v0=0xa1600000 v1=0x00013800 a0=0x00000400 sp=0x82004b80"
    ), "handoff line: {ls:?}");
    assert!(ls.contains(
        &"map: post-handoff 20 steps, stop: unknown instruction at pc=0xa1600050: 0x0000ffff, final pc=0xa1600050"
    ), "post stop line: {ls:?}");
    // Executed pages: only the entry window, 20 fetches.
    let pages = section(
        &ls,
        "-- executed pages (4 KiB, top) --",
        "map: post-handoff MMIO: (no accesses over modeled device windows)",
    );
    assert_eq!(pages, vec!["  0xa1600000  x20"], "pages: {pages:?}");
    // Nothing else is reachable from the entry window: MMIO, DRAM stores
    // and console are all empty in the post horizon.
    assert!(ls.contains(&"map: post-handoff MMIO: (no accesses over modeled device windows)"));
    assert!(ls.contains(&"map: post-handoff data writes: (no DRAM stores)"));
    assert!(ls.contains(&"map: console after handoff: (no UART output)"));
    // The head trace shows all 20 post-handoff steps, verbatim starts.
    let head = section(
        &ls,
        "-- first 20 post-handoff instructions --",
        "-- executed pages (4 KiB, top) --",
    );
    assert_eq!(head.len(), 20, "head: {}", head.len());
    assert_eq!(
        head[0].trim_start(),
        "pc=0xa1600000  00008160  special(0x20)"
    );
    assert_eq!(head[1].trim_start(), "pc=0xa1600004  00000000  nop");
}

#[test]
fn dump_seam_writes_the_window_and_reports_failures() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    // The boot window (phys 0x1fc00000) is the firmware image XIP-mapped:
    // a dump must read back the image head byte-for-byte, including the
    // pinned first instruction word 0x24006fb5 (little-endian b56f0024).
    let dir = std::env::temp_dir();
    let path = dir.join(format!("truman-bd-dump-{}-good.bin", std::process::id()));
    let path = path.to_str().unwrap().to_string();
    let o = run(&[
        "run",
        fw,
        "--max-steps",
        "100",
        "--dump",
        &format!("0x1fc00000,0x40,{path}"),
    ]);
    assert_eq!(o.code, 0, "stdout: {}\nstderr: {}", o.stdout, o.stderr);
    assert!(
        o.stdout
            .contains(&format!("dump: 0x1fc00000+0x40 -> {path}")),
        "dump line: {}",
        o.stdout
    );
    let dumped = std::fs::read(&path).expect("dump file exists");
    let image = std::fs::read(fw).expect("firmware readable");
    assert_eq!(dumped.len(), 0x40);
    assert_eq!(dumped, image[..0x40], "the boot window is the image head");
    assert_eq!(
        &dumped[..4],
        &[0xb5, 0x6f, 0x00, 0x24],
        "first instruction word"
    );
    std::fs::remove_file(&path).ok();
    // An unwritable target is reported inline, the run still succeeds.
    let o = run(&[
        "run",
        fw,
        "--max-steps",
        "100",
        "--dump",
        "0x1fc00000,0x40,/nonexistent-dir/truman-bd.bin",
    ]);
    assert_eq!(o.code, 0, "a failed dump must not fail the run");
    assert!(
        o.stdout
            .contains("dump: 0x1fc00000+0x40 -> /nonexistent-dir/truman-bd.bin FAILED:"),
        "FAILED line: {}",
        o.stdout
    );
}

#[test]
fn verbose_flag_prints_the_identification_notes() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    let notes = [
        "firmware: stage-1 loader: zeroBoot v1.0 for 1506F (flash 0x0000)",
        "firmware: stage-2 loader: Boot form SPI v1.13 (flash 0x8000)",
        "firmware: SoC descriptor: SPHE15 (flash 0x38b000)",
    ];
    let o = run(&["run", fw, "--max-steps", "5", "-v"]);
    assert_eq!(o.code, 0, "stderr: {}", o.stderr);
    let ls = lines(&o.stdout);
    for (i, n) in notes.iter().enumerate() {
        assert_eq!(ls[i], *n, "notes lead the verbose report: {ls:?}");
    }
    assert_eq!(ls[3], BOOT_RESET_LINE, "boot diagnostics follow the notes");
    // And without -v the notes are absent everywhere.
    let o = run(&["run", fw, "--max-steps", "5"]);
    assert_eq!(o.code, 0);
    assert!(
        !o.stdout.contains("firmware: "),
        "quiet mode has no notes: {}",
        o.stdout
    );
}

#[test]
fn map_head_honors_an_explicit_count() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    // BD pinned the default head (64 → all 20 post steps); an explicit
    // smaller head caps the printed window likewise.
    let o = run(&["map", fw, "--post-steps", "2000", "--head", "10"]);
    assert_eq!(o.code, 2);
    let ls = lines(&o.stdout);
    assert!(
        ls.contains(&"-- first 10 post-handoff instructions --"),
        "{ls:?}"
    );
    let head = section(
        &ls,
        "-- first 10 post-handoff instructions --",
        "-- executed pages (4 KiB, top) --",
    );
    assert_eq!(head.len(), 10, "head: {}", head.len());
    assert_eq!(
        head[0].trim_start(),
        "pc=0xa1600000  00008160  special(0x20)"
    );
}

#[test]
fn value_tables_missing_file_is_a_clean_io_error() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    // §59 fix pin: a missing file is an I/O error — the value was
    // present, so the old contradictory double report ("cannot read…"
    // then "requires a value" + usage reprint) must stay gone.
    let o = run(&["map", fw, "--value-tables", "/nonexistent/tables.bin"]);
    assert_eq!(o.code, 1, "stdout: {}\nstderr: {}", o.stdout, o.stderr);
    assert!(
        o.stderr
            .contains("error: cannot read value tables /nonexistent/tables.bin:"),
        "stderr: {}",
        o.stderr
    );
    assert!(
        !o.stderr.contains("requires a value"),
        "no contradiction: {}",
        o.stderr
    );
    assert!(
        !o.stderr.contains("USAGE"),
        "no usage reprint: {}",
        o.stderr
    );
    assert!(!o.stderr.contains("panicked"));
}

#[test]
fn value_tables_junk_file_is_rejected_by_content() {
    let fw = firmware_path();
    let fw = fw.to_str().unwrap();
    // A present-but-unparseable file is a content rejection (exit 1,
    // the parser's message, no panic) — the same gate the library
    // tests pin, now end-to-end through the CLI.
    let dir = std::env::temp_dir();
    let path = dir.join(format!("truman-bf-junk-{}.bin", std::process::id()));
    std::fs::write(&path, b"not a value ladder").expect("write junk");
    let path = path.to_str().unwrap().to_string();
    let o = run(&["map", fw, "--value-tables", &path]);
    assert_eq!(o.code, 1, "stdout: {}\nstderr: {}", o.stdout, o.stderr);
    assert!(
        o.stderr.contains(&format!("error: value tables {path}:")),
        "stderr: {}",
        o.stderr
    );
    assert!(!o.stderr.contains("panicked"));
    std::fs::remove_file(&path).ok();
}
