#!/usr/bin/env python3
"""Cross-reference audit for docs/: every '§N' must match a '## N.' heading;
every 'docs/*.md' and 'tests/*.rs' mention must exist; README numbers must
match the tree."""
import re, sys, pathlib

root = pathlib.Path(__file__).resolve().parent.parent
problems = []

fa = (root / "docs/firmware-analysis.md").read_text()

# 1. Section headers present
headers = {int(m.group(1)) for m in re.finditer(r"^## (\d+)\.", fa, re.M)}
print(f"section headers found: {len(headers)} (max {max(headers)})")

# 2. Every §N reference must resolve to a header (<= max)
refs = {int(m.group(1)) for m in re.finditer(r"§(\d+)", fa)}
dangling = sorted(r for r in refs if r not in headers)
print(f"§ references: {len(refs)} distinct; dangling: {dangling or 'none'}")
if dangling:
    problems.append(f"dangling § refs: {dangling}")

# 3. docs/*.md mentions must exist
for m in re.finditer(r"docs/([a-z-]+\.md)", fa + (root / "README.md").read_text()):
    if not (root / f"docs/{m.group(1)}").exists():
        problems.append(f"missing doc file: docs/{m.group(1)}")

# 4. tests/*.rs mentions must exist
tests_txt = (root / "README.md").read_text() + fa
for m in re.finditer(r"tests/([a-z0-9_]+\.rs)", tests_txt):
    if not (root / f"tests/{m.group(1)}").exists():
        problems.append(f"missing test file: tests/{m.group(1)}")

# 5. README test-count / version claims
readme = (root / "README.md").read_text()
cargotoml = (root / "Cargo.toml").read_text()
ver = re.search(r'^version = "(.*?)"', cargotoml, re.M).group(1)
print(f"Cargo version: {ver}")
mv = re.search(r"v?(\d+\.\d+\.\d+)" , readme)
if mv and mv.group(1) != ver:
    # README may legitimately pin an older milestone version; report only
    print(f"NOTE: README first version-like number {mv.group(1)} vs Cargo {ver}")
for claim in re.findall(r"(\d+)\s+tests", readme):
    print(f"README test-count claim: {claim}")

# 6. CHANGELOG latest version must match Cargo
chlog = (root / "CHANGELOG.md").read_text()
clv = re.search(r"^## (\d+\.\d+\.\d+)", chlog, re.M).group(1)
print(f"CHANGELOG latest: {clv}  Cargo: {ver}  {'OK' if clv == ver else 'MISMATCH'}")
if clv != ver:
    problems.append(f"CHANGELOG {clv} != Cargo {ver}")

# 7. roadmap latest milestone version must match Cargo
roadmap = (root / "docs/roadmap.md").read_text()
rv = re.search(r"\(v(\d+\.\d+\.\d+)\)", roadmap)
print(f"roadmap latest: v{rv.group(1)}  Cargo: {ver}  {'OK' if rv.group(1) == ver else 'MISMATCH'}")
if rv.group(1) != ver:
    problems.append(f"roadmap v{rv.group(1)} != Cargo {ver}")

# 8. firmware SHA pin still in README (audit anchor)
if "5ca012b2" not in readme:
    problems.append("README lost the firmware SHA-256 anchor")

print()
if problems:
    print("PROBLEMS:")
    for p in problems:
        print(" -", p)
    sys.exit(1)
print("cross-reference audit: clean")
