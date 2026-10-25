//! CLI-surface drift guard (Milestone AZ, §53).
//!
//! The hardware-session handoff (docs/hardware-session.md) documents exact
//! command lines for the real-device ingestion (`dram_compare`,
//! `solve_divergence`, `ladder_solve`, `ladder_eval`, and the main CLI's
//! `map`). If a flag is renamed or an example's parser drops one, the doc
//! silently stops matching the tooling — and the future real-device session
//! hits friction exactly when it can least afford it. This test pins the
//! surface mechanically:
//!
//! 1. `--help` output must list every subcommand and every flag the
//!    dispatcher actually implements (both directions: documented flags
//!    exist, no undocumented flags in the usage text).
//! 2. `--version` must print the package version.
//! 3. Every `--flag` the handoff doc attaches to an example invocation must
//!    be a declared string literal in that example's source.
//! 4. Every `--flag` the README attaches to a `truman-emulator` invocation
//!    must appear in the usage text.

use std::collections::BTreeSet;
use std::process::Command;

fn run_cli(arg: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_truman-emulator"))
        .arg(arg)
        .output()
        .expect("spawn truman-emulator");
    assert!(
        out.status.success(),
        "{arg} exited non-zero: {}",
        out.status
    );
    String::from_utf8(out.stdout).expect("utf-8 output")
}

fn usage() -> String {
    run_cli("--help")
}

/// Every flag the dispatcher implements, from the usage text it prints.
fn usage_flags(usage: &str) -> BTreeSet<String> {
    usage
        .split_whitespace()
        .filter_map(|tok| tok.strip_prefix("--"))
        .map(|t| {
            t.trim_end_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                .to_string()
        })
        .filter(|t| !t.is_empty())
        .collect()
}

/// Every `-x` short flag in the usage text.
fn usage_shorts(usage: &str) -> BTreeSet<String> {
    usage
        .split_whitespace()
        .filter(|t| {
            t.len() == 2
                && t.starts_with('-')
                && !t.starts_with("--")
                && t.chars().nth(1).is_some_and(|c| c.is_ascii_alphabetic())
        })
        .map(|t| t[1..].to_string())
        .collect()
}

const SUBCOMMANDS: [&str; 4] = ["inspect", "run", "trace", "map"];
const USAGE_FLAGS_PIN: [&str; 7] = [
    "max-steps",
    "trace-tail",
    "limit",
    "pre-steps",
    "post-steps",
    "head",
    "value-tables",
];

#[test]
fn usage_lists_all_subcommands_and_flags() {
    let u = usage();
    for sc in SUBCOMMANDS {
        assert!(
            u.contains(&format!("truman-emulator {sc} ")),
            "usage text lost the `{sc}` subcommand line:\n{u}"
        );
    }
    let have = usage_flags(&u);
    for f in USAGE_FLAGS_PIN {
        assert!(have.contains(f), "usage text lost the --{f} flag:\n{u}");
    }
    assert!(
        usage_shorts(&u).contains("v"),
        "usage text lost the -v short flag:\n{u}"
    );
}

#[test]
fn version_matches_the_package() {
    let v = run_cli("--version");
    assert!(
        v.contains(env!("CARGO_PKG_VERSION")),
        "--version output {} does not carry the package version {}",
        v.trim(),
        env!("CARGO_PKG_VERSION")
    );
}

/// Flags declared inside an example's argument parser (string literals that
/// start with `--` in its source text).
fn example_flags(example_src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for tok in example_src.split(['"', '\n']) {
        if let Some(f) = tok.strip_prefix("--") {
            if !f.is_empty() && f.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
                out.insert(f.to_string());
            }
        }
    }
    out
}

/// Every `--flag` token the doc attaches to the given example. Fenced
/// `cargo run ... -- example ...` invocations are joined across `\`
/// continuations first and only the part after the standalone `--`
/// separator is scanned, so cargo's own flags (`--release`, `--example`)
/// are not misattributed to the example. Prose lines mentioning the
/// example are scanned whole.
fn documented_flags_for(doc: &str, needle: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();

    // 1. Join fenced invocations into logical lines.
    let mut logical: Vec<String> = Vec::new();
    {
        let mut open: Option<String> = None;
        for line in doc.lines() {
            let l = line.trim_end();
            match open.take() {
                Some(mut acc) => {
                    let ended = !l.ends_with('\\');
                    let chunk = l.trim_end_matches('\\').trim_end();
                    acc.push(' ');
                    acc.push_str(chunk);
                    if ended {
                        logical.push(acc);
                    } else {
                        open = Some(acc);
                    }
                }
                None => {
                    if l.contains("cargo run") {
                        let ended = !l.ends_with('\\');
                        let chunk = l.trim_end_matches('\\').trim_end().to_string();
                        if ended {
                            logical.push(chunk);
                        } else {
                            open = Some(chunk);
                        }
                    } else {
                        logical.push(l.to_string());
                    }
                }
            }
        }
        if let Some(acc) = open {
            logical.push(acc);
        }
    }

    // 2. Scan.
    for line in &logical {
        if !line.contains(needle) {
            continue;
        }
        let scan = if line.contains("cargo run") {
            match line.find(" -- ") {
                Some(i) => &line[i + 4..],
                None => continue,
            }
        } else {
            line.as_str()
        };
        for tok in scan.split_whitespace() {
            // trailing punctuation (`,`, `;`, backticks, `--flag)`) tolerated
            let t = tok
                .trim_start_matches('`')
                .trim_end_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
            if let Some(f) = t.strip_prefix("--") {
                if !f.is_empty() {
                    out.insert(f.to_string());
                }
            }
        }
    }
    out
}

#[test]
fn hardware_session_flags_match_the_example_parsers() {
    let doc = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/docs/hardware-session.md"
    ))
    .expect("read docs/hardware-session.md");
    for (example, needle) in [
        ("dram_compare", "dram_compare"),
        ("solve_divergence", "solve_divergence"),
        ("ladder_solve", "ladder_solve"),
        ("ladder_eval", "ladder_eval"),
    ] {
        let src = std::fs::read_to_string(format!(
            "{}/examples/{example}.rs",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap_or_else(|e| panic!("read examples/{example}.rs: {e}"));
        let declared = example_flags(&src);
        assert!(
            !declared.is_empty(),
            "{example}.rs exposes no --flags (parser moved?)"
        );
        let docd = documented_flags_for(&doc, needle);
        let missing: Vec<_> = docd.difference(&declared).collect();
        assert!(
            missing.is_empty(),
            "docs/hardware-session.md uses flags missing from examples/{example}.rs: {missing:?}\ndeclared: {declared:?}"
        );
    }
}

#[test]
fn readme_invocation_flags_exist_in_usage() {
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
        .expect("read README.md");
    let u = usage();
    let have = usage_flags(&u);
    for line in readme.lines().filter(|l| l.contains("truman-emulator")) {
        for tok in line.split_whitespace() {
            if let Some(f) = tok.strip_prefix("--") {
                let f = f.trim_end_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
                assert!(
                    have.contains(f),
                    "README invocation uses --{f} which the usage text does not list:\n{line}\nusage flags: {have:?}"
                );
            }
        }
    }
}
