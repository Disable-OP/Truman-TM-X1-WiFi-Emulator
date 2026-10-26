//! The bench-band sync guard (Milestone BJ, §63).
//!
//! Milestone BI restated the README's bench band from "25.4–25.6 across
//! sandbox rebuilds" to "24–26" after fresh measurement, but left
//! `tools/claims_audit.py`'s `BAND` constant at (25.4, 25.6). The seam was
//! demonstrated live: the audit measured 25.65 M instr/s — inside the
//! README's own band — and reported "OUTSIDE the README band 25.4-25.6"
//! with a false advisory. A checker that misstates the claim it audits is
//! a misreport vector of the §54/§56/§59 family, and it happened precisely
//! because nothing mechanically read both statements together (the same
//! unguarded-file shape as §62.2's ci.yml).
//!
//! This test pins the two statements to each other: the README's stated
//! band and the audit tool's `BAND` constant must name the same interval.
//! A deliberate re-statement of either side now requires updating both —
//! the drift BI introduced can never recur silently.

use std::fs;

/// The band the README states inline, e.g.
/// "… (machine-dependent; 24–26 measured across sandbox rebuilds …)".
/// The dash has historically been an en dash; both dash forms are accepted.
fn readme_band(readme: &str) -> Option<(f64, f64)> {
    let line = readme
        .lines()
        .find(|l| l.contains("measured across sandbox rebuilds"))?;
    let before = line.split(" measured across sandbox rebuilds").next()?;
    let dash = before.rfind(['\u{2013}', '-'])?;
    let lo_start = before[..dash].rfind(|c: char| !c.is_ascii_digit())? + 1;
    let lo: f64 = before[lo_start..dash].parse().ok()?;
    // The dash may be a multi-byte en dash: advance past it in chars,
    // never in raw bytes.
    let dash_char = before[dash..].chars().next()?;
    let hi: String = before[dash + dash_char.len_utf8()..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let hi: f64 = hi.parse().ok()?;
    Some((lo, hi))
}

/// The band the audit tool states as its operative constant:
/// `BAND = (lo, hi)`.
fn tool_band(tool: &str) -> Option<(f64, f64)> {
    let line = tool
        .lines()
        .find(|l| l.trim_start().starts_with("BAND ="))?;
    let inner = line.split('(').nth(1)?.split(')').next()?;
    let mut it = inner.split(',');
    let lo: f64 = it.next()?.trim().parse().ok()?;
    let hi: f64 = it.next()?.trim().parse().ok()?;
    Some((lo, hi))
}

#[test]
fn bench_band_statements_agree() {
    let readme = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
        .expect("read README.md");
    let tool = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tools/claims_audit.py"
    ))
    .expect("read tools/claims_audit.py");

    let (rlo, rhi) = readme_band(&readme).expect(
        "README must state a numeric bench band before `measured across \
         sandbox rebuilds`",
    );
    let (tlo, thi) =
        tool_band(&tool).expect("claims_audit.py must keep a parsable `BAND = (lo, hi)`");

    assert_eq!(
        (rlo, rhi),
        (tlo, thi),
        "the README bench band ({rlo}-{rhi}) and tools/claims_audit.py \
         BAND ({tlo}-{thi}) drifted apart — restate BOTH or neither (§63)"
    );
}
