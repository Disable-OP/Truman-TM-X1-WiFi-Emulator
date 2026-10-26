//! The README test-count sync guard (Milestone BK, §64).
//!
//! The README's front door states the suite size ("cargo test --release
//! # 198 tests: …") — a hand-maintained claim that no mechanical reader
//! had ever checked, the same unguarded-file shape as §62.2's ci.yml and
//! §63's bench band: a milestone that adds tests but forgets the README
//! (or bumps the README without adding tests) would ship a silently
//! stale count. The counting rule is exact and self-exact: a test is
//! counted as a line whose trimmed content is exactly `#[test]` —
//! `cargo fmt` puts every test attribute alone on its own line, while
//! mentions of the attribute inside docs or string literals (including
//! this file's own) never form such a line, so the counter cannot count
//! itself as more tests than it declares. `examples/` is excluded on
//! purpose — plain `cargo test` does not run example unittests, so
//! counting them would overstate the suite.
//!
//! The test pins the two statements to each other: a suite change or a
//! README re-statement must update BOTH in one commit. The §63-style
//! negative control (perturb → fail with the drift message → revert →
//! pass) was run live at introduction.

use std::fs;
use std::path::Path;

fn count_tests_in_file(path: &Path) -> usize {
    fs::read_to_string(path)
        .map(|src| src.lines().filter(|l| l.trim() == "#[test]").count())
        .unwrap_or(0)
}

fn count_tests_recursive(dir: &Path) -> usize {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += count_tests_recursive(&path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                total += count_tests_in_file(&path);
            }
        }
    }
    total
}

fn readme_claimed_count(readme: &str) -> Option<usize> {
    // The claim lives on the bench/test cheat-sheet line:
    // "cargo test --release          # 198 tests: …"
    readme.split("# ").find_map(|chunk| {
        let head = chunk.split_whitespace().next()?;
        head.parse::<usize>().ok().and_then(|n| {
            let rest = chunk.strip_prefix(&head.to_string())?;
            rest.starts_with(" tests:").then_some(n)
        })
    })
}

#[test]
fn readme_test_count_matches_the_suite() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let src = count_tests_recursive(&Path::new(manifest).join("src"));
    let tests = count_tests_recursive(&Path::new(manifest).join("tests"));
    let suite = src + tests;
    assert!(
        suite > 100,
        "the counter lost the tree: src={src} tests={tests}"
    );

    let readme = fs::read_to_string(Path::new(manifest).join("README.md")).expect("read README.md");
    let claimed = readme_claimed_count(&readme)
        .expect("README must state a `# N tests:` count on the test line");

    assert_eq!(
        suite, claimed,
        "the README test count ({claimed}) and the actual suite size \
         ({suite} = {src} src + {tests} tests) drifted apart — restate \
         BOTH or neither (§64)"
    );
}
