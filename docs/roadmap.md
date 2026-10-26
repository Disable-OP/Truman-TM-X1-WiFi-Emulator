# Roadmap

## Milestone CB — pure verification round: zero findings, chain green ✅ (v0.37.20)

- **No new evidence class; round honestly empty (§81.1)**:
  zero-diff against origin/main after a sixth routine snapshot
  rollback recovery, claims fresh-verified (1,301,783/trap/
  exit 2, banner 19/19, bench 24.84 in 24–26 — nineteenth
  consecutive in-band), CI of 9fa0ddb completed/success, 198
  tests debug and release, all gates clean. Boot pins unchanged;
  blocker stays hardware-gated (§51.2); §32 holds its two named
  candidates for the owner.

## Milestone CA — pure verification round: zero findings, chain green ✅ (v0.37.19)

- **No new evidence class; round honestly empty (§80.1)**:
  zero-diff against origin/main after a fifth routine snapshot
  rollback recovery, claims fresh-verified (1,301,783/trap/
  exit 2, banner 19/19, bench 25.37 in 24–26 — eighteenth
  consecutive in-band), CI of 1b83689 completed/success, 198
  tests debug and release, all gates clean. Boot pins unchanged;
  blocker stays hardware-gated (§51.2); §32 holds its two named
  candidates for the owner.

## Milestone BZ — pure verification round: zero findings, chain green ✅ (v0.37.18)

- **No new evidence class; round honestly empty (§79.1)**:
  zero-diff against origin/main after a fourth routine snapshot
  rollback recovery, claims fresh-verified (1,301,783/trap/
  exit 2, banner 19/19, bench 25.60 in 24–26 — seventeenth
  consecutive in-band), CI of d1aeb12 completed/success, 198
  tests debug and release, all gates clean. Boot pins unchanged;
  blocker stays hardware-gated (§51.2); §32 holds its two named
  candidates for the owner.

## Milestone BY — pure verification round: zero findings, chain green ✅ (v0.37.17)

- **No new evidence class; round honestly empty (§78.1)**:
  zero-diff against origin/main after a third recurring sandbox
  snapshot rollback (recovery now a fixed-cost routine —
  §68.2/§76.1/§77.1), claims fresh-verified (1,301,783/trap/
  exit 2, banner 19/19, bench 25.74 in 24–26 — sixteenth
  consecutive in-band), CI of e6a6968 completed/success, 198
  tests debug and release, all gates clean. Boot pins unchanged;
  blocker stays hardware-gated (§51.2); §32 holds its two named
  candidates for the owner.

## Milestone BX — pure verification round: zero findings, chain green ✅ (v0.37.16)

- **No new evidence class; round honestly empty (§77.1)**:
  zero-diff against origin/main after a second, recurring sandbox
  snapshot rollback (recovered from the written §68.2 discipline
  alone), claims fresh-verified (1,301,783/trap/exit 2, banner
  19/19, bench 25.55 in 24–26 — fifteenth consecutive in-band),
  CI of fc9aa4b completed/success, 198 tests debug and release,
  all gates clean. Boot pins unchanged; blocker stays
  hardware-gated (§51.2); §32 holds its two named candidates for
  the owner.

## Milestone BW — pure verification round: zero findings, chain green ✅ (v0.37.15)

- **No new evidence class; round honestly empty (§76.1)**:
  zero-diff against origin/main after a sandbox snapshot-rollback
  recovery (fetch-before-merge discipline §68.2 resolved a stale
  23471b2 clone showing permission-bit-only drift, nothing
  discarded), claims fresh-verified (1,301,783/trap/exit 2,
  banner 19/19, bench 25.72 in 24–26 — fourteenth consecutive
  in-band), CI of 9e6e139 completed/success, 198 tests debug and
  release, all gates clean. Boot pins unchanged; blocker stays
  hardware-gated (§51.2); §32 holds its two named candidates for
  the owner.

## Milestone BV — pure verification round: zero findings, chain green ✅ (v0.37.14)

- **No new evidence class; round honestly empty (§75.1)**:
  zero-diff against origin/main, claims fresh-verified
  (1,301,783/trap/exit 2, banner 19/19, bench 25.28 in 24–26 —
  thirteenth consecutive in-band), CI of e7fb1d7
  completed/success, 198 tests debug and release, all gates
  clean. Boot pins unchanged; blocker stays hardware-gated
  (§51.2); §32 holds its two named candidates for the owner.

## Milestone BU — pure verification round: zero findings, chain green ✅ (v0.37.13)

- **No new evidence class; round honestly empty (§74.1)**:
  zero-diff against origin/main, claims fresh-verified
  (1,301,783/trap/exit 2, banner 19/19, bench 25.59 in 24–26 —
  twelfth consecutive in-band), CI of 6d3923a completed/success,
  198 tests debug and release, all gates clean. Boot pins
  unchanged; blocker stays hardware-gated (§51.2); §32 holds its
  two named candidates for the owner.

## Milestone BT — pure verification round: zero findings, chain green ✅ (v0.37.12)

- **No new evidence class; round honestly empty (§73.1)**:
  zero-diff against origin/main, claims fresh-verified
  (1,301,783/trap/exit 2, banner 19/19, bench 25.56 in 24–26 —
  eleventh consecutive in-band), CI of b474371 completed/success,
  198 tests debug and release, all gates clean. Boot pins
  unchanged; blocker stays hardware-gated (§51.2); §32 holds its
  two named candidates for the owner.

## Milestone BS — pure verification round: zero findings, chain green ✅ (v0.37.11)

- **No new evidence class appeared; round honestly empty (§72.1)**:
  zero-diff against origin/main, claims fresh-verified
  (1,301,783/trap/exit 2, banner 19/19, bench 25.86 in 24–26 —
  tenth consecutive in-band), CI of 22b143b completed/success, 198
  tests debug and release, all gates clean. Boot pins unchanged;
  blocker stays hardware-gated (§51.2); §32 holds its two named
  candidates for the owner.

## Milestone BR — the Wayback bound completed to the limit of reachable tooling ✅ (v0.37.10)

- **Bound completed (§71.1)**: web.archive.org is host-level
  connection-dead from this sandbox (HTTP 000 on both schemes,
  control included, two rounds) — transport, not corpus; the
  archive.org availability API is alive and shows a vendor-root
  snapshot (2026-08-15) but ZERO archived snapshots at six
  plausible SPHE1506-era paths; the snapshot itself is unreadable
  (dead host). Discipline note: future rounds should NOT re-probe
  web.archive.org. Zero new evidence; zero downloads.
- **Claims re-verified fresh (§71.2)**: 1,301,783/trap/exit 2 PASS,
  banner 19/19, bench 24.82 inside 24–26 (ninth consecutive
  in-band); CI of 29d7632 completed/success. Boot pins unchanged;
  blocker stays hardware-gated (§51.2).

## Milestone BQ — the Internet Archive corpus completed at variant level; §32 candidate list doubles ✅ (v0.37.9)

- **Variant sweep (§70.1)**: seven queries, metadata level only,
  zero payload bytes downloaded — six new items enumerated.
  `IPFOX_IPTV_1507G.bin` is byte-size-identical (7,397,376 B) to
  BP's 1506G candidate: the §32 owner-decision list now names two
  same-family candidates. Three 1506T channel packs (one "wifi
  7601" = MT7601 USB WiFi context); Tatung TLM-1506T monitor manual
  bounded as a model-number coincidence; Montag GX ROM (14.8 MB)
  recorded with its GX6605S uncertainty. `1506HV` and
  `sunplus ecos`: zero items.
- **Claims re-verified fresh (§70.2)**: 1,301,783/trap/exit 2 PASS,
  banner 19/19, bench 25.58 inside 24–26 (eighth consecutive
  in-band); CI of a0d315b completed/success. Boot pins unchanged;
  blocker stays hardware-gated (§51.2).

## Milestone BP — the Internet Archive corpus probed; §32 names its first concrete candidate ✅ (v0.37.8)

- **Never-probed corpus (§69.1)**: archive.org seed searches — zero
  for the exact chip names; two unenumerated items under
  `sunplus 1506`: `ipfox-iptv-1506-g` (IPFOX_IPTV_1506G.bin,
  7,397,376 B — same-family 1506G firmware image, ~8 MiB scale) and
  a 347 KB misrsat ZIP (accessory pack, not a container). **Metadata
  level only — zero payload bytes downloaded** per the §32 owner
  decision; the record upgrades from abstract to a named,
  precisely-sized first candidate. Wayback CDX bounded-unavailable
  (control query also empty — tool-level, not corpus-level).
- **Claims re-verified fresh (§69.2)**: 1,301,783/trap/exit 2 PASS,
  banner 19/19, bench 25.86 inside 24–26 (seventh consecutive
  in-band); CI of f53856f completed/success. Boot pins unchanged;
  blocker stays hardware-gated (§51.2).

## Milestone BO — verification round: the temporal delta closed; claims fresh-verified ✅ (v0.37.7)

- **First delta-form probe (§68.1)**: five authenticated temporal
  searches (created-after / pushed-after at the project's start) on
  the engine seeds return total:0 across the board — the
  two-repository universe (§51.2) is not merely snapshot-complete
  (§65.1) but *staying* complete. Zero closed corpora re-sampled
  (timestamp-keyed queries are a dimension no prior pass used).
  §51.2 closure survives its sixth named-lead probe.
- **Claims re-verified fresh (§68.2)**: 1,301,783/trap/exit 2 PASS,
  banner 19/19, bench 25.66 inside 24–26 (sixth consecutive
  in-band); CI of 3e562e4 completed/success. Boot pins unchanged;
  blocker stays hardware-gated (§51.2).

## Milestone BN — verification round: the source repository's community surface and ref-graph bounded; claims fresh-verified ✅ (v0.37.6)

- **Three bounded probes (§67.1)**: upstream issues/PRs = 0/0 ever
  (no community hardware captures; the P1 unlock cannot arrive via
  the social surface); transport-level ref-graph = exactly one ref,
  zero tags/branches (the clone is provably complete); archived-repo
  freshness stays as recorded (§61.1, §66.1 — no re-sampling per the
  §65.1 discipline note). Zero leads; §51.2 closure survives its
  fifth named-lead probe.
- **Claims re-verified fresh (§67.2)**: 1,301,783/trap/exit 2 PASS,
  banner 19/19, bench 25.46 inside 24–26 (fifth consecutive in-band
  fresh-sandbox measurement); CI of c68dc69 completed/success
  (verify-the-predecessor chain unbroken). Boot pins unchanged;
  blocker stays hardware-gated (§51.2).

## Milestone BM — verification round: the sibling-product community bounded; claims fresh-verified ✅ (v0.37.5)

- **Two new named-lead repos probed (§66.1)**: `xav-ax100` search
  surfaced superdavex (update-pack contents: rootfs/spapp/MCU;
  README confirms Cortex-A9 + Linux 3.4.5 + an 8 MiB **ecos
  partition** — independent corroboration of the platform family's
  eCos pairing; `CUST_PACK.BIN` magic `GEMINIp`) and wtodd1
  (extraction/decryption tooling for the ARM family's update format).
  Zero engine evidence; vicyap freshness probe: frozen at 5682442 =
  the provenance pin (zero drift). §51.2 closure survives its fourth
  named-lead probe.
- **Claims re-verified fresh (§66.2)**: 1,301,783/trap/exit 2 PASS,
  banner 19/19, bench 25.66 inside 24–26 (fourth consecutive in-band
  fresh-sandbox measurement); CI of 3a68ca3 completed/success (sixth
  gate's first run included). Boot pins unchanged; blocker stays
  hardware-gated (§51.2).


## Milestone BL — the code-search universe enumerated; the docs audit wired into CI ✅ (v0.37.4)

- **Authenticated code search (§65.1)**: BK's bounded-unavailable probe
  executed (PAT via temporary header file, never in process args):
  `sp_imgdec`=3, `sphe15xx`=57, `sphe1506`=10, `MOON_PNG_RESET`=3,
  `sp_png_dec`=24 — the public engine universe is EXACTLY the two known
  repositories (Team-Proton, vicyap/sony-xav-ax100), both archived
  in-repo and studied; kernel hal_imgdec already documented identical
  (§34). Zero new leads; §51.2 closure strengthened. First hit was a
  rediscovery caught by the provenance table — discipline note
  recorded.
- **CI verified (§65.2)**: a87edc6 (BK) and ce351fa (BJ) runs
  `completed, success`.
- **xref into CI (§65.3)**: ci.yml gains the sixth gate
  (`python3 tools/xref_audit.py`) — §52.2's "reproducible by any CI
  run" arc closed; ci_workflow.rs restated in the same commit. Suite
  unchanged (198). Transport lesson: this session's display layer
  strips `[m` — verify by byte length/presence, never quoted display.
  Boot pins unchanged; blocker stays hardware-gated (§51.2).


## Milestone BK — two named probes executed; the README test-count seam closed ✅ (v0.37.3)

- **Named probes (§64.1, §51.2 respected)**: the public eCos tree —
  `packages/ecos.db` of the canonical CVS→git mirror has ZERO
  sunplus/sphe (MIPS HAL vendors: tx39/jmr3904/atlas/malta/…; no
  Sunplus target ever landed) — and GitHub repo search
  (`sphe15xx`/`sphe1506`/`sunplus ecos` all total: 0). Code-level
  seed search bounded-unavailable this session (search backend down,
  REST quota exhausted mid-probe). Zero leads; surveys stay closed.
- **CI verified (§64.2)**: the runs API reports ce351fa (BJ) and
  50e7768 (BI) `completed, success` — first session-side CI
  verification.
- **The count seam (§64.3)**: the README's `# N tests:` front-door
  claim had no mechanical reader; the suite is exactly the count of
  lone `#[test]` lines (src 67 + tests 131 = 198; examples excluded
  by `cargo test` semantics). `tests/readme_count_sync.rs` pins the
  two statements ("restate BOTH or neither"); negative control live
  (197 perturb → fail → restore → pass). A naive occurrence counter
  over-counted by three (its own doc/string mentions) — the drift
  message located it; the line-shaped rule is self-exact. Suite
  197 → 198 (debug and release). Boot pins unchanged; blocker stays
  hardware-gated (§51.2).


## Milestone BJ — the bench-band seam closed: the audit tool states the claim it audits ✅ (v0.37.2)

- **The seam (§63.1)**: BI restated the README bench band to 24–26 but
  left `tools/claims_audit.py`'s `BAND` at (25.4, 25.6) — demonstrated
  live: the tool measured 25.65 M instr/s (inside the README's band)
  and issued a false "OUTSIDE" advisory against a compliant
  measurement. The drift came from the very commit that proved the old
  band dishonest; the misreport family (§54/§56/§59) on the tool
  whose job is auditing the README.
- **The fix + guard (§63.2/§63.3)**: `BAND = (24.0, 26.0)` + docstring;
  `tests/claims_band_sync.rs` pins the README band and the tool `BAND`
  to each other ("restate BOTH or neither"), negative control run live
  (perturb → fail with the drift message → revert → pass). En-dash
  UTF-8 boundary lesson recorded. Audit re-run: 25.72 inside 24.0-26.0,
  PASS. Suite 196 → 197 (debug and release). Boot pins unchanged;
  blocker stays hardware-gated (§51.2).


## Milestone BI — the README claims audit as one command; the CI workflow pinned as a test ✅ (v0.37.1)

- **`tools/claims_audit.py` (§62.1)**: the BA/BF-style claims
  re-verification as one committed command — boot-chain pin and
  banner-in-order as hard gates (bytes-captured, CR-artifact-safe,
  ordered-subsequence vs the README sample), bench advisory. Fresh
  measurement: 1,301,783 steps / trap 0xa1600050 / exit 2 (pin holds
  post-BG/BH), banner 19/19, bench 24.3–25.7 observed — README band
  restated to 24–26.
- **CI workflow pinned (§62.2)**: a suspected corrupted push filter was
  refuted at byte level (the committed filter is the well-formed
  `[main]`; 49 successful runs on main verified via the runs API), but
  the file had no mechanical reader — `tests/ci_workflow.rs` now pins
  the filter and the five gate lines. Suite 195 → 196 (debug and
  release). Boot pins unchanged; blocker stays hardware-gated (§51.2).


## Milestone BH — the stray-token seam closed: nothing received is silently dropped ✅ (v0.37.0)

- **Named-lead probes (§61.1)**: the artifact owner's other GitHub
  repositories (all Blockman-GO-domain, zero SPHE leads) and the
  Team-Proton/linux-sunplus-15xx source (frozen 2016-05-13) — zero
  leads both ways; surveys stay closed (§51.2).
- **The seam (§61.2)**: the positional sibling of BG's cross-flag
  seam — `inspect fw -v` and `run fw junk` were live-demonstrated
  exiting 0 while silently dropping the extra tokens (`inspect` never
  looked past its path; `run`/`trace`/`map` read only the first
  leftover positional).
- **The fix (§61.3)**: `ArgsError::UnexpectedArgument` — "error:
  unexpected argument `X`", exit 1, stdout empty (rejection precedes
  load/boot/execution). Four tests (stray flag + second path on
  `inspect`; second positional on each of `run`/`trace`/`map`).
  Suite 191 → 195 (debug and release). Boot pins unchanged; blocker
  stays hardware-gated (§51.2).


## Milestone BG — the cross-flag seam closed: every accepted flag has an observable effect ✅ (v0.36.0)

- **The seam (§60.1)**: `run`/`trace` share one option parser, so three
  recognized flags were silently dropped on the other subcommand
  (`run --limit`, `trace --trace-tail`, `trace --dump`) — parsed into
  fields the command never reads. A silently-accepted flag is a
  misreport vector, the same family as §56/§59; none of the three has
  a meaningful cross-subcommand semantics, so the honest fix is
  rejection.
- **The fix (§60.2)**: `ArgsError::WrongCommand` — "error: option
  `--limit` is not valid for `run`", exit 1, stdout empty (rejection
  precedes load/boot/execution). Usage inventory completed: `--dump
  PHYS,LEN,PATH` now listed for `run`, and the `--verbose` long form
  documented (`-v, --verbose`).
- **Five contracts (§60.3)**: the three rejections pinned (exact
  message, empty stdout), duplicate flags pinned last-wins
  (`--max-steps 40 --max-steps 60` executes 60), `-v` vs `--verbose`
  pinned byte-identical on the 5-step window; `cli_surface` now pins
  `dump`/`verbose` in the usage inventory. Suite 186 → 191 (debug and
  release). Boot pins unchanged; blocker stays hardware-gated (§51.2).


## Milestone BF — the README claims re-verified through the tracer refactor; the last map flag contracts pinned ✅ (v0.35.4)

- **Claims re-measured (§59.2)**: boot chain exactly 1,301,783 steps
  (pin holds through BC/BD/BE); bench 25.42 M instr/s — inside the
  stated 25.4–25.6 band; every README banner sample line matches in
  order (trailing-whitespace normalization now stated alongside §54's
  CR note). Named-lead probes (§59.1): repo ecosystem and 60-day
  SPHE1506 doc search — zero leads; surveys stay closed (§51.2).
- **One error-path fix (§59.3)**: a missing/unparseable
  `--value-tables` file printed a contradictory double report ("cannot
  read…" + "requires a value" + usage); the new `ArgsError::Io`
  variant reports it cleanly (exit 1, no usage reprint), pinned both
  ways in `tests/cli_diag.rs` along with the explicit `--head`
  contract. Suite 183 → 186 (debug and release).
- **Unchanged**: boot pins (chain to trap 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); fmt clean, clippy 0 warnings. Blocker stays
  hardware-gated (P1, P2/P2b/P2c, P3; surveys closed per §51.2).


## Milestone BE — the machine's determinism invariants pinned chain-wide ✅ (v0.35.3)

- **`tests/determinism.rs` (§58.2)**: full end-state equality (stop,
  steps, pc, console bytes, registers, touchmap) for the real boot
  chain — atomic vs 65,537-step chunked runs (both stop at exactly
  1,301,783), `run` vs `run_traced` (describe hook inert chain-wide),
  and fresh rebuilds (no per-instance state leakage). Plus the
  explicit `--trace-tail 5` / `0` contract in `tests/cli_success.rs`.
  Suite 179 → 183 (debug and release).
- **Unchanged**: boot pins (chain to trap 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); fmt clean, clippy 0 warnings. Blocker stays
  hardware-gated (P1, P2/P2b/P2c, P3; surveys closed per §51.2).


## Milestone BD — the `map` success-path and the diagnostics seam pinned ✅ (v0.35.2)

- **`tests/cli_diag.rs` (§57.2)**: three end-to-end tests — the `map`
  contract on the real firmware (jr v0 handoff at step 1,301,763 with
  pinned registers; +20 steps to the acceptance trap at 1,301,783, the
  two counters now explicitly related; Cpu stop 0xa1600050/0x0000ffff →
  exit 2; one executed page x20; empty MMIO/writes/console; 20-line
  head trace verbatim); the `--dump` seam (boot window 0x1fc00000
  reads the image head byte-for-byte, first word 0x24006fb5; FAILED
  line doesn't fail the run); and `-v` (three `firmware:` notes lead,
  absent without). Suite 176 → 179 (debug and release).
- **Unchanged**: boot pins (chain to trap 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); fmt clean, clippy 0 warnings. Blocker stays
  hardware-gated (P1, P2/P2b/P2c, P3; surveys closed per §51.2).


## Milestone BC — the success-path output contract pinned; the trace head misreport fixed ✅ (v0.35.1)

- **One reporting defect found and fixed (§56.1)**: `Tracer` was a ring
  buffer keeping the *last* `--limit` reports while `trace` labelled its
  output "first N instructions" and printed the seen-total in the
  heading — with defaults, 100 tail steps printed under "first 1000";
  `--limit` capped nothing observable. Tracer is now a head tracer
  (first-N capture) and `trace` prints the drained length; the run tail,
  map head and entry oracle were audited and are unaffected.
- **`tests/cli_success.rs` (§56.3)**: four end-to-end success-path
  contract tests — the eight `inspect` lines (SHA anchor + three notes),
  the 5-step trace verbatim (boot diagnostics, first five decodes,
  all-empty window map, register footer), the head-honesty regression
  pin (300→100 with prefix stability; 40/10), and the 1000-step `run`
  report (boot/stop/final-pc, UART banner, uart window offsets, the
  24-instruction tail, register footer). Suite 172 → 176 (debug and
  release).
- **Unchanged**: boot pins (handoff step 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); fmt clean, clippy 0 warnings. Blocker stays
  hardware-gated (P1, P2/P2b/P2c, P3; surveys closed per §51.2).


## Milestone BB — the CLI error-path contract pinned end-to-end ✅ (v0.35.0)

- **`tests/cli_errors.rs` (§55.2)**: seven end-to-end tests against the
  real binary — nonexistent file, junk-wrong-size, junk-exact-4MiB,
  unknown command/option, missing path, malformed value, and the two
  stop-reason contracts on the real firmware (step limit below the
  handoff → exit 0; CPU fault at the pinned trap → exit 2 with
  pc/step-count in the report). Suite 165 → 172 (debug and release).
- **One UX defect fixed (§55.2)**: malformed numeric values were
  misreported as "unknown option"; a dedicated `BadValue` variant now
  distinguishes them across all seven numeric/spec parse sites.
- **Unchanged**: boot pins (handoff step 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); fmt clean, clippy 0 warnings. Blocker stays
  hardware-gated (P1, P2/P2b/P2c, P3; surveys closed per §51.2).


## Milestone BA — the README's measurable claims re-verified on a fresh sandbox ✅ (v0.34.9)

- **Three claims re-measured (§54.1)**: boot chain = exactly 1,301,783
  steps (pin holds, trap 0xA1600050/0xFFFF re-confirmed); bench =
  25.63 M instr/s vs the pinned ~25.4 (machine variance — README now
  states "≈25 M, machine-dependent, 25.4–25.6 across rebuilds"); boot
  banner re-captured line-by-line identical (device CR line endings
  normalized in the sample, as any faithful sample would).
- **No code changed (§54.2)**: 165 tests debug and release; fmt
  clean, clippy 0 warnings; boot pins unchanged.
- **Blocker stays hardware-gated (§51.2)**: P1, P2/P2b/P2c, P3; the
  handoff doc's command lines stay guarded (Milestone AZ). Surveys:
  named leads only.


## Milestone AZ — the CLI surface pinned as a test ✅ (v0.34.8)

- **`tests/cli_surface.rs` (§53.2)**: four drift-guard tests — usage ↔
  dispatch (4 subcommands, 7 flags, `-v`), version pin
  (`CARGO_PKG_VERSION`), handoff-doc ↔ example parsers (every `--flag`
  the doc attaches to `dram_compare`/`solve_divergence`/`ladder_solve`/
  `ladder_eval` must be declared in that example's source; fenced
  invocations joined across continuations, cargo's own flags excluded —
  the scanner's false-positive mode was itself caught on the first run,
  demonstrating detection), README ↔ usage. Zero drift on the
  committed tree; the suite grows 161 → 165 (debug and release).
- **Why (§53.1)**: the real-device session cannot afford command
  drift — the handoff doc's command lines are now mechanically guarded
  in both directions.
- **Unchanged**: boot pins (handoff step 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); fmt clean, clippy 0 warnings. Blocker stays
  hardware-gated (P1, P2/P2b/P2c, P3; surveys closed per §51.2).


## Milestone AY — documentation cross-reference audit tooling + README SHA-256 anchor restored ✅ (v0.34.7)

- **Mechanical audit committed (`tools/xref_audit.py`, §52.1)**: all §
  references resolve (38 + 21 + 4 across the three docs, zero
  dangling), path mentions exist, version pins consistent. The one
  gap found and fixed: the README lost the firmware SHA-256 anchor —
  restored (`5ca012b2…28f0`, re-verified against the committed file).
- **No code changed (§52.2)**: 161 tests debug and release; fmt
  clean, clippy 0 warnings; boot pins unchanged.
- **Blocker stays hardware-gated (§52.3)**: P1, P2/P2b/P2c, P3 — the
  real-device session per docs/hardware-session.md. Surveys: named
  leads only (§51.2).


## Milestone AX — the fourteenth survey: die-marking, Russian platforms, DVB wikis — all registered angles closed ✅ (v0.34.6)

- **The last three registered angles executed (§51.1)**: die-marking
  databases carry zero SPHE15xx entries (the family circulates under
  full MPNs — but the distributor layer yields one new audit datum:
  **SPHE1506G = QFP-128**, sibling matrix 1502A=HTQFP256 / 1506C=TQFP /
  1500A=BGA, lifecycle Active production); the Russian-language corpus
  bounded at the repair/model layer (satproshivka tag page model-only;
  video titles consistent with AW's repair-channel corpus); DVB
  hardware wikis documented as the Enigma2/Linux class — the SPHE15xx
  platform does not appear in them.
- **One contradiction found and resolved (§51.1)**: an AI-generated
  commerce guide claims "SPHE1506 = ARM9 + DSP" — refuted by the
  artifact's own instruction-stream ISA proof (§3, §7); the same
  corpus's 1512 page says "32-bit MIPS-based" — the corpus contradicts
  itself, the bytes do not. A live demonstration of why secondary
  claims cannot outrun primary evidence.
- **Verdict (§51.2)**: the fourteenth consecutive zero for the §30.3
  value-ladder; **no registered survey angles remain** — future passes
  re-open only on a named lead. Blocker stays hardware-gated exactly
  as registered (P1 dual-gate solve, P2/P2b/P2c, P3 readout).
- **Unchanged**: boot pins (handoff step 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); 161 tests debug and release; fmt clean,
  clippy 0 warnings.


## Milestone AW — the thirteenth survey: loader-tool corpus, gitee direct, the academic line ✅ (v0.34.5)

- **Three registered angles executed (§50.1)**: the RS232 loader-tool
  corpus (`Sunplus Loader Tool V1.5.0.8`, family list including 1505,
  `All Chunk Done!!!` chunk protocol; RT809F footage naming the exact
  **1506Hv 4MB Built in Wi-Fi** variant — our die + memory config as a
  repairable common platform); gitee closed by direct access (zero);
  the Chinese academic PNG-decode genre confirmed generic (Verilog
  theses, no Sunplus content); plus a generation-boundary data point —
  the 2011 SPHE1500/1510 sibling spec lists `JPEG、GIF、TIF、BMP` with
  **no PNG**, so PNG entered the low-cost family's photo pipeline
  between that generation and the 1506's.
- **Verdict (§50.2)**: zero new evidence toward the §30.3 value-ladder —
  the thirteenth consecutive pass; corroboration plus closed corpora.
  Blocker stays hardware-gated exactly as registered.
- **Unchanged**: boot pins (handoff step 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); 161 tests debug and release; fmt clean,
  clippy 0 warnings.

## Milestone AV — the twelfth survey: patents full-text, boot strings, gitee, Generalplus ✅ (v0.34.4)

- **Four corpora closed or bounded (§49.1)**: Sunplus decode-engine
  patents full-text (US 7,738,718 B2 — genuine Sunplus table-driven
  decoder IP, but JPEG-shaped end to end; no DEFLATE value-ROM); the
  boot-string code search (exact `zeroBoot` across public code: zero
  repositories — the loader family has no source footprint anywhere);
  gitee (sampled, nothing — retriable only with direct site access);
  the Generalplus successor line (marketing-level only — no SDK,
  register map, or value-structure content; lineage question now
  checked, not assumed).
- **Verdict (§49.2)**: zero new evidence toward the §30.3
  value-ladder — the twelfth consecutive pass. Remaining angles:
  video teardowns, CNKI theses, direct gitee crawl. The blocker stays
  hardware-gated exactly as registered.
- **Unchanged**: boot pins (handoff step 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); 161 tests debug and release; fmt clean,
  clippy 0 warnings.

## Milestone AU — the eleventh survey: four new angle families ✅ (v0.34.3)

- **Four corpora none of the prior ten touched (§48.1)**: the eCos
  upstream/fork lineage (upstream carries exactly one Sunplus
  architecture — SPCE, the 16-bit μ'nSP audio SoC; no SPHE15xx HAL
  exists upstream; GitHub eCos forks are generic clones — the vendor's
  `eCos v1.16 1506f` is now *documented* as an internal fork, not
  assumed); the Chinese commerce/ODM corpus (an Alibaba listing names
  `SUNPLUS1506G` with `Photo: JPEG,BMP,PNG` — the first marketing-level
  independent witness for the sibling die's PNG photo stack); the
  datasheet-aggregator corpus (zero SPHE1505/1506 datasheets; the
  corpus *does* host other SPHE datasheets, so the absence is vendor
  release practice — consistent with AT's UART-ISP-manual finding);
  the certification corpus (no FCC filing for a MENA-market device;
  official sunplus.com index has no SPHE1506 — the DVB STB line is
  ODM-channel only).
- **Verdict (§48.2)**: zero new evidence toward the §30.3
  value-ladder — the eleventh consecutive pass; corroboration plus one
  closed lineage question. The blocker stays hardware-gated exactly as
  registered (P1 dual-gate solve, P2/P2b/P2c, P3 readout).
- **Unchanged**: boot pins (handoff step 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); 161 tests debug and release; fmt clean,
  clippy 0 warnings.

## Milestone AT — the tenth survey: two independent corroborations ✅ (v0.34.2)

- **The device's home corpus, entered for the first time (§47.1)**: the
  MENA receiver-firmware ecosystem (Arabic-adjacent sites) confirms the
  audit's "Probable SPHE1506HV" die variant independently of our
  artifact — non-affiliated sites label the exact device "TRUMAN TM-X1
  1506HV 4M" with hardware version `DVBS-1506HV-V1.0-OTP-S0` and host
  the SVT1/SVC4 build trains (ours is SVT2; 512M/4M configuration
  matches). A May 2026 public security-research writeup on a sibling
  1506F receiver prints, on real UART hardware, the identical boot
  chain our audit reconstructed from bytes (`zeroBoot v1.0 for 1506F`,
  `SPI SCS flow`, `jump to second boot!!`, `LD eCos v1.16 1506f
  20111226-V1.1, RAM_START:=0x81600000`) plus the same flags-2 LZMA
  software path Milestone L identified — the first live-device
  independent witness for the boot-chain reconstruction. Adjacent:
  the zeroBoot loader family's `v1.12T2` variant in the wild, an
  `IC( SPHE1506 )` tool default, and Sunplus's own UART ISP manual
  publicly readable.
- **Corroboration, not constants (§47.2)**: the tenth consecutive
  survey returns zero new evidence toward the §30.3 value-ladder; the
  blocker stays hardware-gated (P1, P2/P2b/P2c, P3 as registered).
  docs/hardware-session.md gains the writeup's low-cost UART
  methodology (multimeter TX profiling, logic-analyzer baud
  determination) as a corroborated provenance route.
- **Unchanged**: boot pins (handoff step 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); 161 tests debug and release; fmt clean,
  clippy 0 warnings.

## Milestone AS — the ninth fresh-evidence survey; the session state ✅ (v0.34.1)

- **Six new angle families, none previously used (§46.1)**: the grep.app
  public code corpus (`SPHE1505`/`sphe15`/`PNG_IMG_WIDTH`/`png_dec` all
  zero — the vendor's chip name and driver symbols have no public code
  footprint); the SPHE15xx Linux BSP sweep (none exists; mainline
  Sunplus remains SP7021-only); the Sunplus image-coding patent sweep
  (US7974484B2 JBIG, US8229236B2 progressive JPEG — genuine Sunplus
  image-codec patents, neither DEFLATE-shaped, no value structures);
  the consumer photo-frame hack corpus (a January 2026 Qiita
  SPMF2800/ThreadX hack — different SoC family, failed for the same
  vendor-practice reason the eighth survey inferred); the
  receiver-community boards (SPHE1505/1506 confirmed as DVB mainboards
  with firmware dumps circulating, but no display-engine driver
  content); the vendor's own press record (the 2008 "Faster Photo
  Decoding" photo-frame SoC release corroborates only the engine's
  existence, no constants).
- **Verdict (§46.2)**: the ninth consecutive survey returns zero new
  evidence toward the §30.3 value-ladder constants; every offline step
  that could be closed into code has been. The remaining work stays
  hardware-gated exactly as registered — P1 (ladder constants via the
  dual-gate solve), P2/P2b/P2c (residue, window preset, staging-tail
  overhang), and the P3 readout whose expected values `dram_compare`
  prints.
- **Unchanged**: boot pins (handoff step 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); 161 tests debug and release; fmt clean,
  clippy 0 warnings.

## Milestone AR — the residue re-boot as one command; the tail overhang ✅ (v0.34.0)

- **The P2 decision row's last manual step, removed (§45.1)**: "load
  the residue via `AddressSpace::copy_in` and re-run the decode
  compare" was library-only prose; `dram_compare --pre <capture>
  --reboot-residue <flashfile.bin> [--base <phys>]` is now its
  one-command form — classify, boot clean, boot with the capture
  copy_in'd at the P2 window base before the boot chain runs, compare
  the two decode-output windows byte-for-byte. RESIDUE INERT /
  RESIDUE ACTIVE verdicts are the measured forms of the model's
  §18.3 statements; short injection is a named exit-2 error; an
  all-zero capture skips the boots as vacuous; exit codes unchanged
  (the experiment is attached data).
- **The tail overhang, found by writing the pins (§45.3)**: the
  driver's chunk-size formula programs the engine to read 0x8692
  bytes from the staging copy — 6 bytes PAST the loader's 0x868C-byte
  copy, content nothing writes (the zero-fill covers band+scratch
  only). The channel is ACTIVE: 0xA5 in the overhang moves exactly
  the decode's final 10 bytes (first divergence exactly at 75,398,
  §17.2's block-1 boundary; adler 0xfc670378; deterministic). The
  pinned 75,408 / 0xcd8d03bf decode is a function of the model's zero
  DRAM init there — a P1 compare whose ONLY divergence is
  [75,398, 75,408) is the pre-registered signature of staging-tail
  residue, not LZ error. **P2c registered** (hardware-session.md):
  one more `dump_image` line (0x02090680, 64 bytes) decides it.
- **Pinned** (3 new tests; 161 total debug and release): below-base
  residue inert with the handoff pin holding (1,301,783 steps under
  injection); whole-output-window residue fully overwritten by §17's
  row semantics (and the clean window's tail is the zero fill past
  the decode length); the overhang channel ACTIVE with the driver's
  own program pinned from the registers (src 0x02088000, ilen 0x8692,
  overhang 6) and the divergence localized to the 10-byte tail.
- **Docs defect fixed**: roadmap prose said handoff step 1,301,763 in
  six places (digit transposition); the authoritative 1,301,783
  (§28.2, §24, the test pins) is now consistent everywhere.
- **Unchanged**: boot pins (handoff step 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); fmt clean, clippy 0 warnings. Eighth
  fresh-evidence survey (2026-10-25, §45.4): the eCos archives hold no
  Sunplus BSP; Sunplus's own SP7021 wiki documents 30+ peripherals
  but no display-codec engine registers (a positive data point on the
  vendor's practice); the distinctive constants return zero hits. The
  §30.4 blocker stays hardware-gated.

## Milestone AQ — the band-window preset as a data seam ✅ (v0.33.0)

- **The last hardcoded decode input became a value (§44)**: the engine's
  LZ window — the vendor band buffer at 0x02040000, 32 KiB, zero-filled
  by the driver (§13), preset with zeros in the model (§18.3) — was a
  constant; `inflate_engine_with_preset` /
  `inflate_engine_with_traced_preset` take the preset CONTENT,
  `MachineConfig::preset_window` carries it, and
  `dram_compare --post --preset-window <file>` loads it. All-zero
  content is byte-identical to the zero-fill API (pinned): every boot
  pin holds by construction. The §35 pattern applied to the window
  dimension.
- **Pinned** (2 new tests; 158 total debug and release): the library
  test replays the §19.1 head's far-back geometry (len 5 / d=223) —
  zero content identical to the `preset_len` API, 0xA5 content changes
  exactly the far-back bytes (values only; the symbol sequence, output
  length and literal bytes are preset-independent). The machine test
  boots the real firmware: all-zero preset file transparent (same stop,
  steps, decode length, 0xcd8d03bf fingerprint); 0xA5 preset moves the
  head's preset bytes to `A5 A5` deterministically;
  `EngineDecodeInputs` carries the preset so the §36 replay is
  byte-exact under the seam (`solve_divergence` consumes it), and the
  zero-preset replay provably differs.
- **The capture plan's gap closed (P2b)**: the LZ window's pre-decode
  content lives in the band buffer — a window P2's pixel capture does
  NOT cover; hardware-session.md now registers the P2b dump (0xA2040000,
  32 KiB, at the P2 halt) plus a free post-decode P1b cross-check, and
  the §21.6 item-2 ingestion is one flag. The solver's family is
  unchanged (a preset-conditional family would multiply the search
  without a new anchor); the §30.4 blocker stays hardware-gated.
- **Unchanged**: boot pins (handoff step 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf); fmt clean, clippy 0 warnings. Seventh
  fresh-evidence survey (2026-10-25, §44.4): nothing new upstream.


## Milestone AP — the P3 readout closed in code; the sixth survey (session record) ✅ (v0.32.1)

- **The cheapest oracle's last manual step, removed (§43.1)**: the P3
  probe readout's half order (probe address 0 = HIGH 16-bit half,
  address 1 = LOW half) was prose-only; `machine::adler32_probe_halves`
  is now the split in code, pinned by test (RFC reference fingerprint
  `0xcd8d03bf` → `(0xcd8d, 0x03bf)`, the standard zlib vectors, and a
  79,872-byte round trip — the P1 window's own size), and
  `dram_compare --post/--ref` print the expected two `mdw` values under
  the reference adler line. `ladder_eval --fingerprint` remains the
  elimination loop; no emulator-semantics change (the probe window
  stays deliberately unmodeled, §34.2's registered caveat).
- **The sixth fresh-evidence survey (2026-10-25, §43.4)**: new angles
  (vendor symbol names, literal MMIO constants, Chinese-language
  hardware-PNG sources) — a generic Chinese PNG-hardware patent
  (CN116760996A, not Sunplus), ArtInChip's decoder docs (different
  vendor), no `sp_gzip`, no value-ladder constants. The §30.4 blocker
  stays hardware-gated.
- **CI observed green through AO (§43.2)**: every workflow run of the
  v0.29–v0.32 line concluded success, read from the public run list;
  gates re-run locally on the restored clone: fmt clean, clippy 0
  warnings, 156 tests debug and release (+1: the probe-halves pin).
- **Environment-recovery hygiene re-recorded (§43.3)**: stale snapshot
  restored again (this time Milestone K); remote remained authoritative
  (fetch → ancestry → `--ff-only`); `scripts/push_safe.sh` recreated
  from its documented design after the snapshot restore dropped it.
  Boot pins unchanged (handoff step 1,301,783, trap 0xA1600050,
  fingerprint 0xcd8d03bf).


## Milestone AO — the §40.3 class prior as the solver's ordering ✅ (v0.32.0)

- **The declared AM follow-up, implemented (§42)**: `length_class` ports
  the reference's class partition; `SolveLimits::class_prior` re-orders
  the length-axis spiral — short codes below-the-baseline first, longer
  classes above-first (the unconditional opposite-sign structure).
  `ladder_solve` runs prior-ordered. The prior is an ORDERING, never a
  constraint — the §40.3 derivation kills the per-class-uniform family
  and fixes the direction, but does not prove every non-uniform
  assignment has short ≤ RFC, so pruning would over-claim; the family,
  predicate, caps and verified verdict are identical either way.
- **Invariance pinned** (3 new tests; 155 total debug and release): on
  a stream whose §40.3-structured truth is content-UNIQUE (short 5→4,
  long 131→139; wrong lengths die on the trailing literal check), both
  orders recover byte-identical verified tables equal to the truth;
  the prior is deterministic; on the recovery suite's ambiguous
  periodic fixture the prior-ordered solve still verifies and
  re-decodes to the capture.
- **The measured economics, honestly (§42.3)**: identical candidate
  sets on unambiguous content (87,034 both orders) — the prior is
  directional alignment, not a search-size reduction. Its real effect
  is on ambiguous periodic content (the real capture is 78 KB of
  periodic pixel rows): it decides which verified answer is reported
  first, biasing it toward the evidence-derived structure.
- **Unchanged**: the §30.3 discipline (coverage bounded by caps and
  budget, never the heuristic), the §30.4 hardware-gated blocker, the
  boot pins; fmt clean, clippy 0 warnings, 155 tests debug and
  release. Fifth fresh-evidence survey (2026-10-25): nothing new
  upstream (software PNG decoders, unrelated Sunplus lines, out-of-
  scope firmware sites).

## Milestone AN — the vector gate as code: the solve pipeline enforces the vendor's second validation ✅ (v0.31.0)

- **§40.4 became executable** (§41): `src/vendor_vector.rs` extracts the
  vendor vector from the archived SDK source, validates it against its
  own accounting (SP magic, 9 chunks, stream_len 0xe98, 3,744 bytes —
  corruption is named and rejected before any verdict), and re-decodes
  it under candidate tables with the engine's own semantics at the
  three §40 start bits. `ladder_solve` now gates every verified solve
  automatically before `--out`; gate-FAIL is reported per start bit and
  NOT written (exit 1); there is no skip flag. AM's sentence — a ladder
  that fits P1 but fails the vector gate is wrong — is now mechanical.
- **The §40 numbers are dual-pinned** (`tests/vendor_vector.rs`, 6
  tests; crate total 152 debug and release): the Rust gate reproduces
  the Python reference exactly — RFC over-runs +570/+377/+358 at
  320/1024/1088, literal census 1,577/1,538/1,535, matches
  1,439/1,403/1,400, clean alphabet and ≥990‰ hex backbone at every
  start bit, and the RFC ladder fails the accounting condition (the
  gate's discriminating edge is real).
- **The frame/value split, measured (§41.3)**: a value-only ladder
  (bases +1, widths fixed) leaves the literal census byte-identical
  while the accounting moves; a width-family ladder (all widths zero)
  re-aligns the stream — 2,350 literals vs the RFC 1,538 at start 1024.
  §19.3's ladder-independence is a fixed-frame statement; the widths
  are stream-position data. The gate fails wrong ladders through both
  routes.
- **What this does not change**: the §30.3 underdetermination extends
  to the vector (the RFC ladder produces clean text — now pinned in
  Rust too); the §40.3 joint anchor sweep remains
  `tools/fw_vector_probe.py`'s job (independent cross-check); the
  §30.4 blocker stays hardware-gated; boot pins unchanged (handoff step
  1,301,783, trap 0xA1600050, fingerprint 0xcd8d03bf); fmt clean,
  clippy 0 warnings, 152 tests debug and release. Fourth fresh-evidence
  survey (2026-10-25): nothing new upstream.

## Milestone AM — the vendor vector's structural leak: the pattern backbone, the joint anchor system, the post-capture vector gate ✅ (v0.30.0)

- **The SDK test container is a richer oracle than any session had used
  (§40)**: the vendor's own `sp_gzip` output (`ZlibBuf` in the archived
  `cmd_sp_png_dec.c`, scramble key 0xa6fc, 9 chunks → 9,216 bytes) was
  re-probed end-to-end (`tools/fw_vector_probe.py`, deterministic,
  offline). Findings, byte-level: (1) the dynamic header's alphabet is
  exactly the observed output alphabet, and **no start bit in
  `[304, 4096)` reaches EOB** under the canonical tables — the vector's
  decode ends by row-count accounting (§40.1); (2) the engine-semantics
  decode (32 KiB zero preset) completes with zero invalid distances and
  over-runs the 9,216 anchor by +570/+377/+358 at the three candidate
  start bits; (3) the literals are ≥99% hex characters and form fluent
  text — **the pattern's backbone is in the stream**, with
  match-produced gaps between fragments (§40.2).
- **The joint two-stream length-anchor system (§40.3)**: Truman's pinned
  `Σ n_t·d = +4,464` vs the vector's `−570/−377/−358` — **opposite
  signs on the same dominant short-length class**. The entire
  per-extra-bit-class uniform family (bounded integer deltas, base
  validity enforced) has **zero joint survivors** at all three start
  bits. Conditional on the vector's no-waste anchor (registered
  caveat); the opposite-sign structure is unconditional — the first
  hardware-independent structural fact about the true ladder beyond
  RFC: short codes at-or-below RFC, long codes above.
- **The post-capture vector gate (§40.4)**: `ladder_solve`'s recovered
  ladder must also re-decode the vector to clean sparse hex text with
  the pinned backbone and the 9,216 accounting — a second,
  vendor-authored validation, now a mandatory step in
  `docs/hardware-session.md`. A ladder that fits P1 but fails the
  vector gate is wrong.
- **No emulator-semantics change; the blocker stays hardware-gated**:
  the §30.3 underdetermination extends to the vector (the text oracle
  admits many ladders — the RFC ladder already produces clean text at
  every start bit), the distance side gains no size equation, and the
  survey (third pass, 2026-10-25) finds nothing new upstream. Boot pins
  unchanged (handoff step 1,301,783, trap 0xA1600050, fingerprint
  0xcd8d03bf); all 146 tests debug and release, fmt/clippy clean.

## Milestone AL — the CI record observed green; the second survey (session record) (v0.29.2)

- **The gate battery is now remotely observed, not just locally claimed**:
  the GitHub Actions record was read via the API on 2026-10-25 — **all 26
  push-triggered workflow runs since CI's creation (first run
  2026-10-23T22:13Z) concluded `success`, zero failures**, including the
  v0.29.1 commit `112199e` (run 40159344860). §38.1's byte-level
  conclusion ("push CI has been firing since Milestone M") is thereby
  confirmed by direct observation, and every gate claim this document
  makes for the sampled milestones (fmt, clippy 0-warnings, check,
  debug AND release suites) has a remote green data point behind it.
- **The second fresh-evidence survey (2026-10-25, later in the day)**:
  no new SPHE15xx PNG_DEC sources, no `sp_gzip` publication, no
  value-ladder constants; the only Truman-related hits are third-party
  firmware distribution sites (other model variants — X1 PLUS is not
  X1 WiFi) whose downloads are out of scope for this repo and
  disallowed by this project's own constraints anyway. The §30.4
  blocker stays hardware-gated; `docs/hardware-session.md` remains the
  single unlock.
- **Environment-recovery hygiene, recorded**: this session's sandbox
  restored a stale local snapshot (Milestone M) with the toolchain
  gone; the clone was re-fast-forwarded from origin (0 local commits,
  25 fetched) with zero loss — the remote remained authoritative
  throughout, which is exactly the failure mode the push-then-verify
  discipline is designed for. All gates re-run locally on the restored
  state: fmt clean, clippy 0 warnings, `check --all-targets` clean, 146
  tests debug and release — matching the remote CI verdicts run for
  byte. No emulator-semantics change; boot pins unchanged.

## Milestone AK — the phantom trigger defect, disproven byte-level; the fresh-evidence survey ✅ (v0.29.1)

- **A false defect, diagnosed and refuted the repo's own way (§38.1)**:
  this session's terminal toolchain intermittently ate the two-character
  sequence that opens a bracketed lowercase-`m` token (the display
  rendered `branches: [main]` as `branches: ain]` across cat, `git
  show`, grep and python-repr views of `.github/workflows/ci.yml`),
  which produced a coherent-looking but wrong diagnosis — "the push
  trigger is the scalar `ain]`, CI has never fired on push". Byte-level
  verification refutes it completely: the trigger line's bytes are
  `5b 6d 61 69 6e 5d` (`[main]`) in the Milestone-M creation blob
  (`7cb66eb7`) AND in the Milestone-AF blob (`220819c8`) AND at
  origin/main — **the trigger was correct from day one and push CI has
  been firing since Milestone M**. The only AF change was the header
  comment and the debug-suite gate (1191 → 1420 bytes). No repo defect,
  no fix needed; the investigation is recorded because the working
  discipline that matters here is that a diagnosis, however plausible,
  is not evidence — only the bytes are (the §30.3 rule applied to the
  toolchain itself).
- **Method note**: every textual shell view is suspect when the content
  can contain the eaten token pair; the reliable reads were the ones
  whose output channel cannot carry it — `git rev-parse` blob SHAs,
  per-byte decimal dumps (`' '.join(str(b) …)`), and the Read tool.
  The initial false positives all came from contiguous-text views. The
  repo's committed bytes were never touched by the corruption (verified
  by hash-object equality between the worktree file and the origin
  blob).
- **The fresh-evidence survey (§38.2), re-run with a stronger method**:
  both upstream sources of the archived vendor references were
  re-fetched on 2026-10-25 (raw-blob fetch for `regfile.h` at
  Team-Proton/linux-sunplus-15xx `master`; a blobless shallow clone +
  `git cat-file` for the three U-Boot files at vicyap/sony-xav-ax100
  `master`, whose tree nests one level deeper than the README path
  suggested) and **all four files are byte-identical to the archives**
  (SHA-256 recorded in §38.2). No new SPHE15xx PNG_DEC source, no
  `sp_gzip` publication, no value-ladder constants anywhere reachable —
  the §30.4 blocker stays hardware-gated and `docs/hardware-session.md`
  remains the single unlock. No emulator-semantics change; all 146
  tests debug and release, green; fmt/clippy clean; boot pins
  unchanged.

## Milestone AJ — the capture→ladder solver: the P1 solve, mechanical ✅ (v0.29.0)

- **The hardware session's last manual step is a command (§37)**:
  `solve::solve_ladder` inverts one true decode output into the engine's
  value ladder, and `examples/ladder_solve.rs` routes it through the
  pre-registered P1 tree (identical → capture-point row; non-zero band →
  the §26.4 row), verifies the recovered ladder by the engine's own
  re-decode, and writes the `SPHEVTBL` container (verified solves only)
  for the §35 `--value-tables` seam. `pixel_dump` gains
  `--value-tables` (the candidate-ladder capture generator).
- **The search frame is §30.2's own evidence**: the RFC frame parses
  canonically, so the extra-bit widths are stream data and the unknowns
  are exactly the 59 base values. The bit-level parse is therefore
  base-independent (a wrong base can only fail a content check, never
  move the parse), and the decode's own output length is a LINEAR
  equation in the bases — `SumGuard` walks the fixed parse once and
  gives every pin a global feasibility interval check. On the pinned
  reference decode the solve commits every match on its first candidate:
  **9,267 attempts for 9,267 matches, 57/57 exercised codes pinned
  agreeing with RFC byte-for-byte, ~16 ms release** (§37.2).
- **Per match, the target's own periodicity is the only oracle**: the
  copy must reproduce `target[j..j+L]` from `src(j−D)` — the engine's
  live-window semantics. Baseline-first candidate ordering (the null
  hypothesis), then a spiral by Manhattan distance; every candidate
  passes the same predicate; caps and the attempt budget are typed
  errors (`SolveLimits`/`SolveError`), never silent partial answers.
  The envelope is stated honestly: baseline-at-truth solves are linear;
  larger deviations spend the budget and fail typed — the P3
  fingerprint is the session's cheap first read, and a budget failure
  with a differing fingerprint is itself recorded information (§37.2).
- **No semantics change**: 146 tests (+9: the synthetic bases-only
  solve, the typed failures, the real-firmware gates incl. the census
  and the exact-RFC agreement), debug and release, green; fmt/clippy
  clean; boot pins unchanged (handoff step 1,301,783, trap at
  0xA1600050, fingerprint `0xcd8d03bf`). Fresh evidence survey: nothing
  new beyond §34.5/§35.2/§36.4 (sp_gzip remains unpublished; no source
  uploads a value ladder).

## Milestone AI — the divergence→symbol map: the P1 solve, fed ✅ (v0.28.0)

- **The P1 DIVERGENT row is executable end-to-end (§36)**: the
  pre-registered "first divergent byte identifies the first match whose
  value semantics differ" is now a lookup — `solve_divergence` boots,
  compares, replays the boot's own decode with a per-symbol event trace,
  **verifies the replay byte-identical before attributing** (integrity
  gate), and names the covering event with its ladder contribution.
- **Passive-observer refactor**: `inflate_engine_with` and the new
  `inflate_engine_with_traced` share one decode core (byte-identical by
  construction); the machine captures the exact decode inputs at service
  time (`engine_decode_inputs()`), so post-boot replay is byte-exact
  regardless of later DRAM writes. `attribute_divergence` is the pure
  mechanical half (library-level, unit-tested).
- **The reference decode's trace is pinned structural data**: 25,031
  events (15,762 literals / 9,267 matches / 2 end-of-block markers);
  decode length 75,408 vs the 79,872 image — the 4,464-byte zero-filled
  band region measured (§17); and the §19.1 head decomposes exactly:
  `60 81` are stream LITERALS (ladder-unreachable, §19.3), bytes 2..7
  are one preset-reaching match (lc=259, len=5, d=223 → zeros).
- **No semantics change**: 142 tests (+7), debug and release, green; the
  boot pins and the `0xcd8d03bf` fingerprint re-verify; fmt/clippy
  clean. All five verdict paths smoke-tested (identical,
  prefix-identical, literal, match, band). Fresh evidence survey:
  nothing new beyond §34.5/§35.2.
- Blocker unchanged: the 59 value-ROM constants (§30.4) — data-gated.
  This milestone shortens the capture→verdict distance: the capture's
  decisive constraint is now a command output.


## Milestone AH — the value-table file seam: the post-capture swap as data ✅ (v0.27.0)

- **The ladder is now a data file (§35)**: `ValueTables::to_bytes` /
  `from_bytes` — a deterministic 191-byte `SPHEVTBL` container admitting
  every offline-bounded family (RFC, per-code deltas, uniform widths,
  m4 direct values); validation rejects exactly what no analyzed family
  occupies (typed `ValueTablesError`).
- **The decode path is config-carried**: `MachineConfig::value_tables`
  (default field-identical to the pinned `ENGINE_VALUE_TABLES` static —
  boot pins unchanged by construction). The post-capture edit (§32.3)
  is now `--value-tables <file>` on `dram_compare --post` and the CLI
  `map`, not a recompile.
- **The P3 elimination row is a loop**: new `examples/ladder_eval.rs`
  boots a candidate file, prints the §19.1 head + the window's
  Adler-32, and with `--fingerprint` exits ELIMINATED/SURVIVES — the
  pre-registered "re-run the swept families under the fingerprint
  oracle" (docs/hardware-session.md) is executable as a shell loop.
- **No semantics change**: 135 tests, debug and release, green (7 new:
  round-trips, pinned container encoding, six typed rejections,
  config-default invariant, file-seam reference reproduction
  `0xcd8d03bf`, candidate differential); fmt/clippy clean; boot pins
  unchanged. Fresh evidence survey: nothing new beyond §34.5.

## Milestone AG — the vendor completion protocol; the probe-window
### fingerprint ✅ (v0.26.0)

- **Registers vendor-named from the already-archived driver sources
  (§34)**: 0xBFFE384C = PNG_DEC_INTERRUPT — the §10.3 dispatcher's bits
  are the engine's interrupt lines (0 bitstream-empty, 2 end-of-chunk,
  4 end-of-image, 5 pixel-buffer-ready = the §13.3 event line, 6
  decode-error) and its "keycode" writes are the vendor ack encoding
  (`bit << 8`); the bit-6 path reading PNG_DEC_CTRL is the vendor
  error-type handler. §23.3's "no keycode consumer" verdict is
  structural: there are no keycodes. 0xBFFE3878/0x387C =
  PNG_PROBE_ADDR/VALUE — §12's "strobe + two samples" is the vendor
  end-of-image **Adler-32 readout** (addr 0 = high half, 1 = low).
- **The cheap oracle, registered before data (P3,
  docs/hardware-session.md)**: four debugger operations at the P1
  window yield the device decode's 32-bit fingerprint;
  `machine::adler32` (zlib-vector-pinned) + a `dram_compare` report
  line + the pinned emulator fingerprint `0xcd8d03bf` make it
  comparable offline. The raw-mode accumulation caveat is registered
  as a first-class outcome, and the emulator models no accumulator on
  purpose (§30.3's discipline).
- **No semantics change**: 128 tests, debug and release, green; boot
  pins unchanged; the 59 value-ROM constants stay hardware-gated
  (§30.4).

## Milestone AF — CI parity: the debug blind spot closed ✅ (v0.25.1)

- **The lesson, applied to the gate itself (§31's own history)**: the
  Milestone AB defect hid from CI because release builds compile out
  `debug_assert!`; the fix verification ran both suites *locally*
  (§31.3), but `.github/workflows/ci.yml` still gated only
  `cargo test --release`. The same blind spot was one push away from
  recurring. CI now runs the full suite in **debug** (debug_assert!
  paths live) before the release run — the gate now matches what the
  repo claims ("126/127 tests, debug and release").
- **+1 instrument pin (127 total)**: the shared watch-hit cap flows
  through the AE classifier correctly — cap=1 records and classifies
  the first hit (device write → MMIO), drops the second (the DRAM
  store) *by the cap* with `mmio_overflow` set, the write map degrades
  to empty rather than to a wrong partial count, and the console byte
  is unaffected (watches observe, not gate). The classifier never
  loses a recorded hit; the cap can, and the flag is the honesty about
  it.
- 127 tests, debug and release, green; fmt/clippy/check clean; boot
  pins unchanged. No emulator-semantics change.

## Milestone AE — the written-memory map: the framebuffer question ✅ (v0.25.0)

- **The measured gap (§33.1)**: §32.3's post-capture map answers code
  (executed pages), device init (MMIO record) and console — but not
  *where the stage writes data*. The handoff entry (0xA1600000) is
  itself DRAM, the menu surface will be DRAM, and the AD instrument's
  own test pins the DRAM marker store as "not a device access": a real
  menu render would light up `exec_pages` and `mmio` and leave the
  framebuffer invisible.
- **The instrument (§33.2)**: `map_handoff` now installs a fifth watch
  over the whole DRAM span, write-kind only (fetches are the page
  histogram's business; reads are not evidence of rendering), and routes
  every post-handoff hit by kind+physical address: DRAM data stores feed
  a written-page histogram (`write_pages`, top 256), a total
  (`writes_total`) and a bounded first-store sample (`write_sample`,
  `WRITE_SAMPLE_CAP` = 64, addresses/values/widths for stride
  inference); every other hit stays in the MMIO record — no recorded
  evidence is dropped. Shared watch cap semantics preserved
  (`mmio_overflow` documents truncation); zero CPU hot-path changes.
- **The horizon, re-measured (§33.3)**: identical blast radius — 20
  instructions, no MMIO, **zero DRAM stores**, no console. §32.2's
  "touches nothing" is now measured for data writes too; pinned in
  `tests/map.rs` (6 tests: marker-store mechanics, render-loop counting
  + sample capping, real-firmware horizon, determinism). 126 tests,
  debug and release.
- **Post-capture procedure updated (§33.4)**: same one command after
  swapping `ENGINE_VALUE_TABLES`; the map now also yields the
  written-page map — consecutive stores 0x400 apart are 1024-byte rows
  (the PNG width), so framebuffer placement and stride fall out of the
  sample directly. No further instrumentation is anticipated.

## Milestone AD — the post-handoff horizon, mapped ✅ (v0.24.0)

- **The instrument (§32.1)**: `Machine::map_handoff(pre, post, head)`
  plus CLI `map` — boots to the loader's `jr v0` entry
  (`MachineConfig::handoff_entry`, default 0xA1600000) and records the
  post window: a per-4 KiB-page executed-instruction histogram, MMIO
  watchpoints over every modeled device window with `(step, PC)`
  attribution, the console delta, and an optional head trace. Bounded by
  construction; deterministic.
- **The current horizon, measured (§32.2)**: handoff at step 1,301,783
  with `v1`=0x13800 (79,872 pixels) and `a0`=0x400 (PNG width) live;
  20 instructions, all in the entry page; death on the 0xFFFF word at
  0xA1600050; zero MMIO; zero console — the §19.1 head paradox, now a
  dynamic measurement.
- **Post-capture procedure closed (§32.3)**: swap `ENGINE_VALUE_TABLES`
  → `cargo test` → `map` — the menu path (code map, device init order,
  console) is mapped by the same command. Pinned in `tests/map.rs`;
  125 tests, debug and release.

## Milestone AC — the value-table seam ✅ (v0.23.2)

- **Prep for the hardware session (§31.4)**: the engine's length/
  distance ladder is now an injectable value (`inflate::ValueTables`);
  the machine decode path goes through the named
  `ENGINE_VALUE_TABLES` constant. Replacing it with the recovered ROM
  values is the whole post-capture edit.
- **Differential test**: a hand-encoded minimal dynamic block (bits
  carry no value information) decodes to different pinned outputs under
  the RFC and custom ladders — tables proven to flow through the engine
  path end-to-end. 120 tests; all pins unchanged.

## Milestone AB — memory-subsystem correctness repair ✅ (v0.23.1)

- **Defect (§31.1)**: the `DdrPhy` window was registered inside the
  `DdrCtrl` span; first-match routing predates the Milestone V
  hot-region memo, whose non-overlap invariant the nesting violates —
  a cached `ddrctrl` slot can misroute an access landing in the PHY
  window (both share one 4 KiB page). Debug builds tripped the memo's
  own `debug_assert!`; release builds (CI) compiled it out.
- **Fix (§31.2)**: `DdrCtrl` split into two non-overlapping windows
  sharing one register file (the decomp engine's pattern); access log
  byte-identical; register semantics unchanged.
- **Verification (§31.3)**: 119/119 in debug *and* release — the first
  clean debug run since Milestone V; boot pins unchanged, confirming
  the misroute was benign for the pinned horizon. No change to the §30
  hardware-session conclusion.

## Milestone AA — the vendor driver lineage; the offline program closed ✅

- **The engine program is named by the vendor's own driver code**
  (`tools/reference/`): MODE 0x90 = 128 B DMA + `zlib_mode=1` (raw
  DEFLATE — no PNG filters, which is *why* every filter family was
  dead), `IMG_TYPE` 0x68 = palette/8bpp passthrough, CHUNK_SIZE = len +
  8 + (len % 2) + 6 verbatim, "SP" = the SDK's "ZLIB/Scrambler"
  container, `PNG_REM_CHUNK_SIZE` = the scramble-key slot (§30.1).
- **The SDK's own test container** (U-Boot test-pattern path, scramble
  key 0xa6fc) parses canonically and violates RFC distances: vendor-side
  proof that the value tables are hardware ROM (§30.2).
- **Refuted/closed offline**: per-code delta value tables (45.4 %
  plateau, `tools/fw_value_fit.py`); the m4 accounting is structurally
  underdetermined without the true pattern (`tools/fw_m4_fit.py`); no
  plaintext eCos copy in flash; Q8's algorithm not in any sweepable
  family (`tools/fw_checksum_hunt.py`); `sp_gzip` not public (§30.3).
- **Boot blocker unchanged in kind, narrowed to one ROM**: the hardware
  session's P1 capture decides it in one command (§30.4).


Milestones follow the original project plan (A → G). Status reflects what is
actually demonstrated by code + tests in this repository.

## Milestone Z — the per-step loop measured and closed ✅

- **Measured first (§28)**: `examples/loop_probe.rs` attributes the
  19.5-vs-63.8 M instr/s gap to the tick fan-out (15.5 ns — two
  `BTreeMap` lookups in the intc tick, one trait dispatch for one timer
  increment), the engine poll (2.2 ns `borrow_mut`), and ~1.5 ns of
  already-fast checks.
- **Three semantics-preserving fixes**: intc change-detection tick,
  timer counter shared cell (run loops increment directly), engine
  request hint outside the `RefCell` — each pinned by
  `tests/looptune.rs`; one inversion defect caught by its own test
  pre-measurement.
- **Results**: boot chain 19.50 → 25.4 M instr/s (+30 %), same
  1,301,783 steps; all 116 prior tests unchanged, 119 total.
- **Verdicts**: tick decimation refuted for this repo (observable timer
  counts; QEMU pays dedicated MMIO machinery for what we keep per-step
  at ~3.6 ns); decode cache stays optional raw-core-only work (§24.5).

## Milestone Y — the hardware-session handoff ✅

- **§22.5/§26.4's remainder made one-command**: pure comparison analysis
  (`Machine::compare_dumps` / `Machine::classify_residue`,
  `src/machine.rs`, integer-only and deterministic, 10 tests incl. the
  real-firmware reference extraction) plus `examples/dram_compare.rs`
  (`--post` P1 / `--pre` P2 / `--ref` offline).
- **Capture protocol registered** (`docs/hardware-session.md`): windows,
  sizes, halt points, the (unvalidated) JTAG/EJTAG plan, capture
  integrity checklist, and the pre-registered decision tree for both
  experiments — verdicts are interpreted before data exists.
- Nothing about the evidence changes: §26's refutations stand; the SP
  stream stays hardware-blocked. The hardware session's cost drops to
  one command per experiment.

## Milestone X — the combined length+distance class: refuted ✅

- **The last in-silico class swept jointly** (§26): every prior length
  search ran with RFC distances (§15.6/§18.2) and every distance probe
  with RFC lengths (§15.6/§17.3); the joint "combined length+distance
  value semantics with position shift" class §25.5 left standing had
  never been tested as such.
- **2,520 exact-parse combinations** (72 length families × 35 distance
  families) through three pre-registered oracles (size ± 1 row, static
  head filter, strict capstone validity ≥ 90 %): baselines reproduce
  byte-for-byte (75,398 control; §15.6's 64 exact-size ladders), and
  **zero survivors** — best strict validity 55.0 % vs the 91–95 % code
  band, zero eCos markers.
- The three §16.3 exact-count predicates (+2 × 2,237 = the exact Δ
  4,474) reach only 54.2 % even with the best distance offset — the
  size match buys no content.
- **Consequence**: every hypothesis class reachable without hardware is
  now refuted with data; the SP-stream program is complete pending a
  real device's post-decode DRAM dump (§22.5 instruments ready).

## Milestone V — performance: device-log ring defect fixed, 13.4× boot
### throughput ✅

- **Measured first (§24.1)**: a bench harness (`examples/bench.rs`) plus an
  isolation probe showed the raw interpreter core at 71.6 M steps/s against
  a 1.45 M steps/s full-machine rate — the bottleneck was never decode.
- **The defect (§24.2)**: every device access log used
  `Vec::remove(0)` at capacity — an O(n) 128–256 KiB shift per logged
  access, 12 sites; the boot's hot windows log on every access. Replaced by
  `VecDeque` rings with identical capped/oldest-dropped semantics.
- **Secondary fixes (§24.3)**: per-step trace formatting made lazy
  (`CpuBackend::set_describe`; +16% alone), interrupt-line re-programming
  gated on change, watch-arming fast path, hot-region memo (hashed page
  key; non-overlap asserted at build).
- **Results (§24.4)**: deterministic boot chain 1.45 → 19.4–19.6 M instr/s
  (13.4×); 100 M steps ≈ 5 s; test suite ~29 s → ~3.2 s. All 104 prior
  tests pass unchanged (semantic equivalence), plus 2 new regression tests.
- **Decode-cache verdict (§24.5)**: refuted as the needed mechanism by
  measurement; remains optional future work for the raw core.

## Milestone U — device coverage: Cause fixes, COP0 timer, keycode verdict ✅

- **Cause-register corrections (§23.2)**: BD = bit 31, IV = bit 23, TI =
  bit 30 (read-only) — the old IV (bit 30) sat on TI and the old BD (bit
  22) on WP; pinned against Linux `mipsregs.h` by a regression test. The
  real boot is unaffected (IE = 0 forever; boot tests untouched).
- **COP0 Count/Compare timer completed to spec (§23.2)**: match-edge
  latch, ack-by-Compare-write, IP7 input + Cause.TI readout, DC gating,
  IM7/IE delivery. 7 unit tests + an end-to-end synthetic delivery test
  (`tests/interrupt.rs`); `Cause.TI` and the write mask match the
  architecture.
- **Keycode consumer closed negative (§23.3)**: static constant-propagation
  scan over the whole payload (`tools/fw_keycode_consumer.py`, 99.0%
  coverage) + dynamic watchpoint attribution on the real boot
  (`examples/keycode_consumer.rs`, pinned by a test) — all six port
  accesses belong to the dispatcher; no code reads the keycode back in
  the horizon. Button labels stay evidence-bounded (consumer is in the
  unreachable eCos stage or hardware). Runtime proof that the engine
  event line (bit 5) shares the key-port read.
- **SoC timer register program pinned (§23.4)**: writes +0x00=0 (once),
  +0x0C=0x8095, +0x1C=0; 226 counter reads (one delay(10) poll, max 901);
  no IRQ semantics claimed. Unit tests + real-firmware pin
  (`tests/timer.rs`, `examples/timer_probe.rs`).

## Milestone T — the Q8 runtime verdict: no checksum code path exists ✅

- **Physical-range watchpoints shipped** (§22.1): `AddressSpace::add_watch`
  records every CPU load/store/fetch inside a physical range with the
  issuing (step, PC), address, width, value and region; the machine run
  loops stamp the per-step context. 9 mechanics tests
  (`tests/watch.rs`); the consumer is `examples/q8_watch.rs`.
- **One anchor corrected** (§22.2): desc word 0x1224 is `0x5DB06FB5`
  (bytes `b5 6f b0 5d`); §15.2's `0xB56FB05D` was a byte-order error.
  The sweep always covered both orders, so its negative result stands.
- **The stage-1 source-field semantics resolved** (§22.3): the loader
  reads exactly three descriptor words — source offset 0x121C, dest
  0x1214, length 0x1220 — and computes the source as hardcoded base
  0x1C10 + offset (0x1C10 + 0x135C = file 0x2F6C). Word 0x1218 and the
  checksum word 0x1224 are never read.
- **The copy and the stream are pure data movement** (§22.3, pinned by
  `tests/q8.rs`): `lwl/lwr`+`sw` one-pass copy (30,136 byte reads,
  3,767 word stores, nothing else); the 19 post-copy destination reads
  are the driver's 18-entry config table; the driver's container loop
  (`lbu; sb; bnez`) accumulates nothing and returns zero
  unconditionally; neither anchor constant exists anywhere in the image
  outside its descriptor word; after the entry no data-read pass over
  the image occurs.
- **Verdict (§22.4)**: the loader has no checksum code path — the
  anchors are packaging metadata, the algorithm is unrecoverable from
  the firmware, and the Q8 oracle does not exist on-device. The head
  paradox is bounded by hardware-only residue (§22.5).

## Milestone S — rank classes and output transforms: both refuted ✅

- **The class-rule input space is closed** (§21.1): the upload inventory
  (§13.6) shows the engine's table RAM receives only syms/first/cum for
  litlen + dist — no class metadata exists. The class of a token is
  f(rank, T[r], first[], cum[], ladder-ROM); external check confirms no
  public PNG_DEC driver/documentation exists beyond the register header.
- **The rank-boundary rule is refuted at word 0, ladder-independently**
  (§21.2, `tools/fw_rank_transform.py`): the rank head `60 81 03 43 …` is
  formed before any match, so all ten ladder × accounting variants share
  one head; the register-aware oracle kills it at step 0 on
  `0x43038160` = COP0 rs=0x18 (a reserved exception on the real 24KEc
  too). All rank walks also under-fill the image (45.6–52.9 kB).
- **Start-bit variants under the rank rule refuted dynamically** (§21.3):
  1088 → `J` to unmapped 0xA8080AC8 (2 steps); 1152 → `JAL`, unknown
  delay-slot word (1 step); 1008/1072 statically fatal.
- **The T-byte boundary refuted statically** (§21.4): fills the image but
  the head dies before any branch (+0x74/+0x14).
- **The output-transform family is dead** (§21.5): 96 candidates total —
  all 23 period-4 lane permutations, 7 period-8 rotations, swap16/
  bitrev8/nibswap, row mirror, and both image-shaped transposes (78×1024,
  1024×78 — the one output-order reading the hardware evidence had NOT
  excluded) over the m1/m2 bases. 24 static survivors, all dynamically
  killed ≤ 14 steps; zero budget-survivors.
- **Consequence (§21.6)**: the head paradox is evidence-bounded — every
  natural single-boundary class rule and small-period output transform is
  dynamically refuted. Remaining: Q8 checksum escalation, hardware-side
  residue (pre-decode DRAM), or two-tier rules (no upload evidence).

## Milestone R — the entry harness: the register-aware dynamic oracle ✅ (commit `R`)

- **Instrument built** (§20.1): `Machine::run_until_pc` (watch-PC stop),
  `CpuBackend::restore` + `Machine::restore_cpu_state` (full register
  restore), `AddressSpace::copy_out`/`copy_in` (bulk DRAM snapshot/restore),
  and `examples/entry_oracle.rs` — boot the real firmware to 0xA1600000,
  capture the loader's post-jump register state (v1 = 0x13800, §19.1),
  then execute each candidate decode from the pixel buffer with a bounded
  budget. Covered by 5 integration tests (`tests/entry.rs`).
- **Sled discrimination**: budget-survivors are automatically checked for
  NOP-sled artifacts (distinct-PC count, zero-word fetch share, UART
  emission) — the zero-filled DRAM below the pixel buffer would otherwise
  fake "survival".
- **Litlen table structure pinned** (§20.2, `tools/fw_syms_probe.py`):
  all 286 litlen symbols coded; canonical rank(s) != s (length symbols
  276/285 at ranks 256/257 as bytes 20/29, literals continue at 258+); a
  byte-valued table cannot carry the literal/length split — a rank-boundary
  class rule is the natural hardware reading and stays open.
- **The class-preserving literal-value family is REFUTED** (§20.3,
  `tools/fw_syms_family.py`, 522 candidates through the dynamic oracle):
  xor/add sweeps (510), bitrev8/nibswap, lut-by-value, rank-shifts,
  per-length reversal, m2/m4 re-walks — 519 fault in the head; the 3
  budget-survivors are NOP-sled artifacts. §19.2's static m1/m2 verdicts
  confirmed dynamically; **m4 corrected** (the `blez $v1` at +0x18 falls
  through with v1 = 0x13800 > 0; death moves +0x6c/+0x74 → +0x38).
- **Next (Milestone S)**: (1) the rank-boundary class rule as an engine
  variant (rank < 256 = literal, = 256 = EOB, > 256 = length-ladder
  selector) — the token stream changes, so static refutations do not
  transfer; (2) the output-transform family; (3) Q8 checksum.

## Milestone A — Repository & firmware audit ✅ (commit `b30337b`)

- Reproducible analysis of `/firmware` (SHA-256 pinned, layout mapped).
- CPU architecture **verified** from opcode evidence: little-endian MIPS32r2
  (ext/ins/mul/cache/COP0 + branch-delay-slot behavior).
- SoC identified: Sunplus SPHE150x family ("SPHE15", zeroBoot 1506F strings).
- Unicorn 2 probed and rejected with a reproducible kseg-fetch failure.

## Milestone B — Rust skeleton ✅

- Cargo crate + CLI (`inspect` / `run` / `trace`), no dependencies.
- Firmware identification/validation (size, prologue words, banners, SHA-256).
- Memory subsystem: MIPS segment translation, RAM/ROM/device regions,
  alignment checks, clean faults.
- Machine composition from a documented `MachineConfig`.

## Milestone C — CPU execution proof ✅

- Known-answer tests for the interpreter (arithmetic, loads/stores,
  unaligned lwl/lwr/swl/swr, ext/ins, COP0, branch delay slots, `mul/div`).
- Real stage-1 loader executes past its prologue (integration test).

## Milestone D — Machine model & basic devices ✅

- Device models derived from boot-code evidence:
  UART (+0x900), timer (+0x600), SPI controller (+0x3E00), DDR PHY (+0x1580),
  DDR controller (+0x1500..0x16A0), param RAM, flash mirror window.
- Each model documents its evidence and confidence; stubs are visible in the
  access map, never silent.

## Milestone E — Boot progress ✅

- Full deterministic boot chain: zeroBoot → "Boot form SPI v1.13" → DDR
  training ("Trim PASS", "DRAM-1G") → payload decompress/copy → "Go!" →
  main firmware ("1ST BOOT", "Zero BSS", "IntCtrl", "LD eCos v1.16 1506f")
  → idle front-panel poll. ~1.3 M instructions, < 1 s wall clock.
- Covered by `tests/boot.rs::real_firmware_boots_through_ddr_training_into_main_firmware`.

## Milestone F — First credible receiver milestone ✅ (definition met)

The plan defined F as "bootloader executes / memory initialization succeeds"
— both are demonstrated. The next credibility rungs are application-level.

## Milestone H/I — Decompression engine: handshake ✅, eCos stage entered ✅

- **Blocker resolved**: the post-"LD eCos" park was the decompression
  engine's wait loop at 0xBFFE3800..0xBFFE3878, not a UI idle loop.
- Engine handshake modeled (devices::DecompEngineLo/Hi, machine service
  loop): table uploads via 0xBFFE3844/0xBFFE3848, start 0xBFFE3824 bit 0.
- **Completion semantics corrected (§13.3)**: the driver's success path is
  the poll's **bit-5 event branch** (ack 0x2000 → event counter reaches the
  header-mirrored limit 78 → accounting block at 0xA2002630 → return
  0x13800); the bit-6 branch is the failure epilogue. Modeling completion
  on bit 5 (the first event satisfies the counter because the second
  counter is initialized to u16@2 − 1 = 77) is what unlocks the loader.
- Compression format: "SP" container = raw DEFLATE; uploaded tables match
  the stream header bit-for-bit. Engine decode modeled with the programmed
  start bit (R3818 = 1024 stream bits; the 16-bit gap is zero padding) and
  decode-to-input-end semantics (end-of-block = marker).
- **Real firmware now prints "Go eCos" + "Boot from SPI" and jumps to the
  decompressed eCos entry at 0xA1600000** (~1.31 M instructions,
  deterministic). First entry into the eCos stage.

## Milestone N — the engine identified: it is a PNG decoder ✅

- **0xBFFE3800..0x387C = PNG_DEC_***, named by the SPHE15xx platform
  register header (`regfile.h`, Team-Proton/linux-sunplus-15xx; CPU =
  MIPS 24KEc per the platform DTS). Every programmed value fits: IMG_WIDTH
  0x400 (driver constant), IMG_HEIGHT 78 (container u16@2), IMG_TYPE 0x68
  (driver constant), BLK_INFO = (btype<<1)|bfinal, PXL_LINE countdown.
- The boot renders a **1024x78 8-bpp image into 0x01600000 and jumps into
  the pixel buffer**: the vendor tool used the boot ROM's PNG engine as a
  decompressor; the eCos binary is the pixel data.
- **The +4,474 size-delta premise is dissolved**: 79,872 = 78 rows x
  1024 px is the image allocation; the engine completes every row (events
  = per-row completion; the "LZ window" = the PNG band buffer, 32 KiB =
  32 rows; the "scratch" = the scaling buffer) and pads short streams
  from the zero-filled band. The §16.1 tail-structure question is moot.
- Engine model updated (`Machine::service_decomp_engine`): row padding to
  the programmed image size and the PNG_BS_USED_CNT completion accounting
  (624 units x 128 px = 79,872 — matches the real device's DRAM-pinned
  value). `Machine::decomp_reg`/`decomp_last_result` accessors +
  `tests/png.rs` pin the whole PNG register program on the real firmware.
- Refutation battery (new tools, §17.3): the §16.3 shift predicates
  **refuted by decoding** (not just counting); all four match-token field
  orders; Kraft-validated second-header search (none); raw-8-bit filter
  geometry (row 0 refutes); 682 exact-count (predicate, adjustment) pairs
  and 135 group-constant ladders for the 79,950 filtered target (zero
  pass the PNG filter-byte oracle); global distance offsets +-16 (flat).
- **Open**: the engine's LZ *value* semantics for the (now image-framed)
  stream; the container is NOT a filtered PNG zlib stream (row 0's first
  byte is the binary's own `60 81`), so IMG_TYPE 0x68 appears to select
  raw filter-less rows.

## Milestone M — Q2 resolved; SP stream structure probed; CI ✅

- **Q2 closed with runtime + static evidence** (§16.4): 0x98000000/
  0x98010000 = the flash mirror's code/data paths (the 0x10000-package
  descriptor reads land there during "LD eCos"); 0xB0000000/0x90000000/
  0xF0000000 are address-classification masks in the loader's dispatcher,
  never dereferenced — the phys 0x10000000/0x30000000 stubs stay visible
  and unused. Tool: `examples/window_correlation.rs` (UART-milestone
  phase segmentation of per-window access logs).
- **SP stream structure probed** (§16.1, `tools/fw_stream_structure.py`):
  block 1 is a well-formed FINAL dynamic block (ends at bit 106,259,
  28,260 bytes); 169,229 bits follow with **no standard continuation**
  (dynamic/fixed/stored refuted by exhaustive per-bit search); both
  Huffman tables are complete (Kraft = 1), so the single-block walk was
  never actually verified past EOB#1.
- **Checksum sweep** (§16.2, `tools/fw_ck_sweep.py`): ~250 standard
  families against both anchors — no hit; Q8 stays open.
- **Position-shift reframing** (§16.3, `tools/fw_shift_predicate.py`):
  only output bytes 0..1 are positionally certain; a length-accounting
  shift (+4,474 via exact-count predicates `dist<96` / `dist≥3485` /
  `len≥5&dist<3073`, all +2) is the remaining structural hypothesis
  class alongside the tail structure.
- **CI**: GitHub Actions workflow (`.github/workflows/ci.yml`) running
  `cargo fmt --check`, `clippy -D warnings`, `cargo test --release` and
  `cargo check` on push/PR.
- New API: `Machine::access_log_names()` (tested).

## Milestone L — container formats resolved; LZ narrowed by five families ✅

- **Q4 resolved for the driver**: the payload is stored **plaintext** at
  flash 0x2F6C (100.000% verbatim vs a post-boot DRAM dump); "copy fw to
  RAM" is a plain copy — pinned by
  `tests/boot.rs::driver_payload_is_stored_plaintext_in_flash`.
- The 0x10000 `CED9D8CC` package decoded: 28-byte descriptors at
  0x100010+idx*28; flags dispatch = 0 plain copy / 1 hardware engine /
  **2 software LZMA decoder** (props div-45/div-9, u64 size, decoder at
  0xA200304C) — a second codec found in the image (§15.3–§15.4).
- desc[6] checksum is never read by the loader; two checksum anchors
  recorded (Q8). Engine window-usage scan: no modes beyond the decoded
  driver/loader/key-dispatcher traffic (§15.5).
- Engine LZ value semantics narrowed by five new refutation families —
  ring-size sweep (incl. a W=32768≡RFC cross-check), word-aligned copy
  sources, snapshot copy, position-periodic write transforms, and the
  size-constrained length-base-table search (64 exact-size solutions,
  all ~50%) — plus the corruption-structure map: validity **decays**
  67→45% and is **not chunk-periodic** (§15.6).

## Milestone K — engine register program complete; LZ semantics narrowed ✅

- The driver's engine program is fully decoded from the runtime access
  log (`Machine::access_log` + `examples/engine_probe.rs`): input base /
  **input end** (`round128(base+len)`), 32 KiB zeroed window, output,
  32-bit input length = container+6, and `0xBFFE383C = (BTYPE<<1)|BFINAL`
  (docs/firmware-analysis.md §13.2).
- The LZ value-semantics search was widened (presets, transforms, ring
  indexing, remap families) against **calibrated** validity oracles and
  then narrowed: the wrongness is structural, not a wrong base table
  (§14.2–§14.4). Next: stage-2 decompressor disassembly (Q4) and
  chunk-periodicity mapping of the corruption.

## Milestone J — Engine interface decoded; decode defect fixed ✅

- **The driver's engine interface is fully reverse-engineered** (§13.6):
  start-bit computation, table-upload protocol (command = engine
  table-RAM address), the canonical counting-sort table builder, the
  completion accounting formula, and the poll countdown preloading.
- **The emulator's engine decode had a frozen read pointer** (every
  match emitted a constant fill of its first source byte). Proven by
  re-implementation (100.000% byte match against the previous output)
  and fixed with a regression KAT. All §13.4 measurements taken with
  the buggy decode are superseded.
- The bit-stream parse is independently verified exact (25,027 symbols,
  zero bad codes); the uploaded tables are verified canonical from the
  DRAM dump; the output size program is verified 78×1024 = 79,872.
- **Open (§14)**: the engine's LZ value semantics — RFC 1951 values give
  75,657 bytes of not-yet-valid code vs the programmed 79,872; the
  length mapping (hardware base tables) is the primary unknown, the
  desc[6] checksum algorithm the decisive missing oracle.

## Milestone G — Extension & stabilization (in progress)

Track by evidence-first value, in rough order:

1. ✅ **Interrupt controller + COP0 cause/compare (first pass)** — the
   "IntCtrl" init program was reverse-engineered and replays bit-identically
   (`devices::IntController`, hypothesis H11); CPU-side delivery is
   architecture-complete (Status/Cause/EPC/EXL/eret/vector selection, delay
   slots) and covered by 14 dedicated tests. **Open**: pending/ack
   semantics, per-source vectors, priorities — the firmware never enables
   `Status.IE` in the observed boot (docs/firmware-analysis.md §11).
2. ✅ **Front-panel/key register (0xBFFE384C) — decode + injection path** —
   the key-scan dispatcher is decoded (read = key lines, write = keycode
   0x100/0x400/0x1000/0x2000; docs/firmware-analysis.md §12) and virtual
   button presses are proven end-to-end on the real firmware
   (`devices::KeyPort`, `Machine::press_key`). **Open**: button labels
   (keycode→UI mapping) and a front-end that surfaces presses.
3. ✅ **Payload codec identification** — resolved in Milestone L: the
   driver payload is plaintext in flash (no codec); the eCos "SP"
   container is a DEFLATE-shaped stream consumed only by the hardware
   engine; the SDK also ships a software LZMA decoder (flags 2 path).
   Remaining: the engine's LZ value semantics (§15.6/§14).
4. ~~**0x98010000 / 0xB0000000 / 0xF0000000 windows**~~ ✅ resolved in
   Milestone M (§16.4): flash-mirror data path + address-classification
   masks; no peripherals found behind them.
5. ~~**Performance**~~ ✅ resolved in Milestone V (§24): the real defect
   was the O(n) device-log ring, not decode — 1.45 → 19.4 M instr/s
   (13.4×); 100 M-step runs ≈ 5 s.
6. ✅ **CI**: GitHub Actions running fmt/clippy/test on push (Milestone M).
7. **SP tail structure + LZ value semantics** (§16.1/§16.3/§16.5): the
   primary boot blocker — decode the 169,229 bits after SP block 1.
   Framing is closed (§25 refutes the chunk-resync dialect family, all
   three §16.5 probes; §16.1 already excluded new block headers) and the
   combined length+distance value-semantics class is refuted jointly
   (§26: 2,520 combinations, zero survivors). What remains is
   hardware-side residue (§22.5) or the §18.4 m2/m4 accounting solve
   that §19.3's head-DFS structurally de-prioritized.

## Milestone P — size anchor + ladder verdict + pre-decode DRAM bound ✅ (commit pending)

- **Size anchor (§18.1)**: under the m2 accounting the symbol path is
  value-independent, so the full-parse decode length is LINEAR in the
  ladder; with RFC values it is 103,014 bytes vs the 79,872 image —
  Milestone O's "exact fill" was the walk cap, not an input-end
  property. The true ladder must satisfy S(L) = image size.
- **Length-ladder search (§18.2)**: 477 in-band seeds cluster at the
  baseline; band-preserving descent tops out at 58.51% (my oracle) /
  49.5% (capstone) with NO structure; monotone projection ≈ baseline.
  Capstone cross-checked (`tools/fw_ladder_verify.py`).
- **Verdict**: the m2 length-ladder family (RFC distance ladder) is
  refuted; unconstrained high scores are size-infeasible by
  construction.
- **Pre-decode DRAM bound (§18.3)**: canary + fresh boot replays prove
  the boot NEVER writes below the pixel base (0 bytes in 1 MiB);
  device determinism then forces the engine's dist>output source to the
  zero-filled band buffer (the emulator's model). New diagnostics API
  `AddressSpace::poke_phys` (ROM write-safe, store still faults).
  Impact bound: 458 matches / 3,001 bytes = 3.98% of the block-1
  content would differ under a below-base window semantics.
- **Next (§18.4)**: the remaining hypothesis space is the BIT
  ACCOUNTING itself — m4-style all-fixed accounting with BOTH ladders
  (59 unknowns) solved jointly against the same linear anchor; Q8
  checksum; DDR residue needs hardware.

## Milestone Q — the executable-head oracle ✅ (session record)

- **Head paradox (§19.1)**: the boot provably executes LZ output byte 0
  (the loader's `jr v0` → 0xA1600000, `v1` = the driver's return value
  live in the register dump) — yet under m1..m4 every decoded head hits a
  reserved/trap word within +0x74, on LITERAL bytes. The device boots, so
  the true decode's head differs from every tested model.
- **Refutations (§19.2)**: uniform extra-width family (100 combos),
  start-bit variants (1008..1216 × m1/m2/m4), table-RAM convention
  variants (first[] shift, cum[] inclusive) — all fail the head oracle.
  Vendor entry signature absent from all decodes; pixel-format
  expansions contradict the completion accounting.
- **Structural conclusion (§19.3)**: the heads die on literals —
  length/distance ladder values cannot fix the head; the m2/m4 joint
  ladder solve (§18.4) is NOT the next step. The remaining space is the
  symbol→value mapping (engine syms[] interpretation) or an unmodeled
  output transform, both parse-preserving.
- **Tools**: `tools/fw_first_corruption.py` (per-byte attribution + word
  classifier + survey), `tools/fw_head_sweep.py` (uniform widths),
  `examples/pixel_dump.rs` (fresh engine output dump).
- **Head-branch DFS run (§19.4)**: escapes exist only via degenerate d=1
  self-copy floods (m1/m4) and m2 admits no escape within 400k nodes —
  the transform is NOT ladder-shaped; next: the symbol-mapping family or
  an emulator entry harness (register-aware oracle).
