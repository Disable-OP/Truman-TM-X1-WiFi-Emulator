//! CI-workflow drift guard (Milestone BI, §62.2).
//!
//! Nothing in the repo mechanically read `.github/workflows/ci.yml`: the
//! quality-gate workflow (fmt, clippy at 0 warnings, tests in debug AND
//! release, check) is contractual — the docs and the README claim it — but
//! a renamed gate or a drifted branch filter would have gone unnoticed.
//! A byte-level investigation of a suspected corrupted push filter
//! refuted the suspicion (the committed filter is the well-formed
//! `branches: [main]`; the apparent corruption was an observation-layer
//! artifact, closed at byte level: 0x5b 0x6d ... = `[main]`), and the
//! investigation showed exactly how unguarded the file was. This test
//! pins the workflow's contractual surface: the main-branch push filter
//! and the exact gate list.

use std::fs;

#[test]
fn ci_workflow_filter_and_gates_are_intact() {
    let src = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/.github/workflows/ci.yml"
    ))
    .expect("read .github/workflows/ci.yml");

    assert!(
        src.contains("branches: [main]\n"),
        "the push filter must remain the well-formed main-branch list:\n{src}"
    );
    for gate in [
        "cargo fmt --check",
        "cargo clippy --all-targets --all-features -- -D warnings",
        "cargo test\n",
        "cargo test --release\n",
        "cargo check --all-targets",
        // BL (§65): the docs cross-reference audit is a CI gate, not a
        // session habit — the §52.2 promise ("reproducible by any future
        // loop or CI run") is wired in, so a dangling § ref or a version
        // drift fails CI exactly like a failing test.
        "python3 tools/xref_audit.py",
    ] {
        assert!(
            src.contains(gate),
            "ci.yml lost the gate `{gate:?}`:\n{src}"
        );
    }
}
