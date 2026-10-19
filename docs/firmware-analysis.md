# Truman TM X1 WiFi — Firmware Analysis (Milestone A)

Target artifact: `firmware/1506HV_512M_4M_SVT2_TRUMAN_TM_X1_WIFI_Internal_flashfile_20220902.bin`

Status legend used throughout this document:

- **Verified** — confirmed by direct evidence (disassembly, reproducible commands, tests).
- **Probable** — supported by evidence but not yet confirmed.
- **Unknown** — insufficient evidence.
- **Not implemented** — functionality the emulator does not have yet.

All commands below were executed against the pristine file in `firmware/`; the
original image was never modified. Analysis scripts referenced here live in
`tools/` (committed) so every result is reproducible from a fresh clone.

---

## 1. Artifact identity

| Property | Value |
|---|---|
| Filename | `1506HV_512M_4M_SVT2_TRUMAN_TM_X1_WIFI_Internal_flashfile_20220902.bin` |
| Size | 4,194,304 bytes = exactly 4 MiB (`0x400000`) |
| SHA-256 | `5ca012b28e2a70e322a48d513d2a3011a0778a7afa7ecd53e4aad102c2cd28f0` |
| MD5 | `52b9c5358826b30e4b9d8e36951243a7` |
| `file(1)` | `data` (no known container magic) |
| Container | **Verified**: raw SPI NOR flash dump, full-image, no external packaging header |

Reproduce:

```sh
sha256sum firmware/*.bin
file firmware/*.bin
python3 tools/fw_recon.py firmware/1506HV_*.bin
```

The size exactly equals a 4 MB-class SPI NOR flash ("4M" in the filename).
The last bytes of the image are `0xFF` (erased flash), which is
characteristic of a flash *dump*, not a packaged update file. The filename
term "Internal flashfile" agrees.

## 2. CPU architecture — VERIFIED

**MIPS32, little-endian, Release 2** (MIPS32r2). This is established from the
instruction encodings at the image entry, not from the filename or strings.

Evidence (offsets are file offsets; disassemble with
`python3 tools/fw_disasm.py firmware/1506HV_*.bin`):

| Offset | Encoding | Instruction | Why it proves the claim |
|---|---|---|---|
| 0x34 | `00 60 1a 40` | `mfc0 $k0, $12` (Status) | COP0 access — privileged MIPS |
| 0x38 | `c0 04 5a 7f` | `ext $k0, $k0, 19, 1` | `ext` exists only in MIPS32r2+ |
| 0xC4 | `c0 14 4b 7d` | `ext $t3, $t2, 19, 3` | ditto |
| 0xE4 | `02 60 8d 71` | `mul $t4, $t4, $t5` | MIPS32 `mul` (SPEC2) |
| 0xF8 | `00 00 c8 bd` | `cache 8, 0($t6)` | MIPS cache-management op |
| 0x198 | `08 00 e0 03` | `jr $ra` | canonical MIPS return |
| 0x68 | `fc bf 1d 3c` | `lui $sp, 0xBFFC` | `lui` + absolute kseg address |

Additional structural evidence:

- Every branch at the entry is followed by a delay-slot instruction
  (`nop` or a real instruction) — MIPS branch-delay architecture.
- The code at 0xC0–0x160 **reads `Config1` (COP0 reg 16, sel 1)**, extracts
  `IL`/`IS`/`IA` fields (`ext` at bits 19/22/16), computes
  `icache_line = 2 << IL`, `icache_size = (64 << IS) * (IA + 1)`, and runs an
  `Index Store Tag` invalidate loop over the I-cache. No emulator could fake
  this without implementing MIPS COP0 semantics — it is real,
  architecture-dependent boot code.
- At 0x178: `mfc0 $k0, Config` / mask `~7` / set `3` — sets `Config.K0 = 3`
  (cacheable write-back), the classic MIPS cache-attribute setup.
- Word-aligned 4-byte instruction stream throughout the loader; no MIPS16e /
  microMIPS encodings observed in the boot region.

**Byte order: little-endian — Verified.** `lui $sp, 0xBFFC` is encoded as
`fc bf 1d 3c` (LSB-first word order in memory).

**Execution mode: boot in kernel mode, unmapped kseg addressing — Verified**
(COP0 Status/Config manipulation at entry, kseg1/kseg0 constants below).

## 3. SoC identification

- **Verified**: SoC family is **Sunplus SPHE15xx** — the descriptor block at
  file offset 0x38B000 begins with ASCII `SPHE15\0\0`; the first-stage boot
  banner says `zeroBoot v1.0 for 1506F` (0x446); the second-stage banner is
  `Boot form SPI v1.13` (0x9016); SDK strings `demo_1500` and
  `demo_1500@sunmedia` (0x38B130) plus `tftp://cdtftp01/user name/rom_net_upgrade.bin`
  (0x38B102) identify the Sunplus/Sunmedia SDK.
- **Probable**: exact die is SPHE1506HV ("1506HV" in the filename; the "1506F"
  in zeroBoot's banner shows the shared 1506 platform).
- **Probable**: build timestamp embedded at 0x38B010: `e6 07 09 02` =
  2022-09-02, matching the filename suffix `20220902`.
- **Unknown**: the exact MIPS core model inside the SPHE1506 (24Kc-class vs
  other MIPS32r2 cores). No `PRId` value is programmed at reset in the flash
  itself; the loader reads `Config1`, implying a normal MIPS32r2 core with
  I-cache. Does not block emulation: a generic MIPS32r2 LE core with
  correct COP0 behavior covers the observed instruction set.
- **Unknown**: public datasheets for SPHE1506 are not available; searches
  found no public memory-map documentation. The memory map below is therefore
  reconstructed from the firmware's own behavior.

## 4. Flash layout

Regions derived from the entropy map + strings + disassembly
(reproduce: `python3 tools/fw_recon.py`, `python3 tools/fw_regions2.py`):

| Flash offset | Size | Entropy | Content | Confidence |
|---|---|---|---|---|
| 0x000000–0x007FFF | 32 KiB | 4.5 | **Stage-1 loader** "zeroBoot v1.0 for 1506F": MIPS32r2 code, strings `SPI SCS flow`, `jump to second boot!!`, `Get RAM Param and configure RAM`, `copy fw to RAM`, `TEST KEY` | **Verified** |
| 0x008000–0x00FFFF | 32 KiB | 4.5–7.3 | **Stage-2 loader** "Boot form SPI v1.13": DDR controller init, DRAM eye/deskew training prints (`WL_IPRD0`, `REYE_*`, `WEYE_*`, `CTCAL_ERR flag`), `DRAM CPU access FAIL !!!` | **Verified** |
| 0x010000–0x01FFFF | 64 KiB | 7.3 | **Encrypted payload + header**: 0x30-byte header (magic `0xCED9D8CC`, version `1`, fields `0x40000000`, size `0x1B000`, dst `0x81600000`, `0x0000868C`) followed by high-entropy data | header structure **Verified**; payload meaning **Probable** |
| 0x020000–0x37FFFF | 3.5 MiB | 8.00 | **Main firmware, encrypted/compressed** — no readable strings, no known compression magic, first word `0xACB60400` | encrypted or compressed — **Probable** (mechanism **Unknown**) |
| 0x380000–0x38Axxx | ~43 KiB | 6.0 | High-entropy data (unknown) | **Unknown** |
| 0x38B000–0x38B1xx | — | — | **`SPHE15` descriptor block**: SoC string, build date 2022-09-02, size/offset table, checksum word at 0x38B0FC | structure **Verified**, semantics **Probable** |
| 0x38B1xx–0x3AExxx | — | — | Encrypted data + TFTP/net-upgrade defaults | **Unknown** |
| 0x3AE1D4–0x3BFFFF | — | — | **Factory satellite/TP database**: `KU_Amos 5`, `Eutelsat 28A/Astra`, `KU_Arabsat 5A`, `Paksat 1R`, `NigComSat`, `Belintersat`, `Express AM6`, `Yamal 402` | **Verified** |
| 0x3C0000–0x3CFFFF | 64 KiB | 7.94 | High-entropy blob (unknown — possibly another encrypted payload) | **Unknown** |
| 0x3D0000–0x3DFFFF | 64 KiB | 2.2 | Config/parameters (mixed entropy; starts with random-looking data) | **Probable** |
| 0x3E0000–0x3EFFFF | 64 KiB | 1.0 | **Parameter page — ERASED (all 0xFF) in this image.** The stage-1 loader reads offsets 0x210/0x284/0x2AC/0x298/0x0100 of this page at boot (see §5) | **Verified** |
| 0x3F0000–0x3FFFFF | 64 KiB | 0.5 | User/ID data: default MAC-like string `4:d4:c4:3c:3d:60` (0x3F03CC), marker strings `22222222`, `55555555`, `65432100`, long hex key block at 0x3FAF08; mostly 0xFF | **Verified** content, semantics **Probable** |

There is no MBR/squashfs/JFFS2/cramfs/LZMA/gzip header anywhere in the image
(magic scan in `tools/fw_regions2.py`); the only clear-text file-system-like
region is the satellite database. The main firmware body is opaque.

## 5. Boot flow (from the code itself)

**Verified** from disassembly of the first 2 KB:

1. Image offset 0x0 begins with a 0x2C-byte prologue that builds four magic
   constants in `$k0/$k1`: `0x5553504E`, `0x554C2053`, `0x6F462072`,
   `0x69447876` (ASCII-reversed: `NPSU`, `S LU`, `r Fo`, `vxDi` — an obfuscated
   signature, **Verified** as data construction; its consumer is **Unknown**,
   most likely the SoC boot ROM).
2. At 0x2C–0x58: reads COP0 `Status` bit 19 (NMI); branches to an NMI path if
   set. Clears COP0 `Count`, then `bal` over a branch table.
3. At 0x68: `lui $sp, 0xBFFC ; ori $sp, $sp, 0x0E00` → **stack at
   0xBFFC0E00, growing down**.
4. At 0x70–0xB8: **read-modify-write of window 0xBFFE0210, 0xBFFE0284,
   0xBFFE02AC** OR-ing `0x380` into each word. With the same-image mapping
   hypothesis these targets equal flash offsets 0x3E0210/0x3E0284/0x3E02AC —
   the erased parameter page — so on real hardware these must decode to
   something else (registers or RAM overlay). **Open question Q1.**
5. At 0xC0–0x160: `Config1`-driven I-cache probe and `Index Store Tag`
   invalidation loop over the whole I-cache; `Config.K0 = 3` setup;
   `mtc0 $zero, Count`.
6. Stage-1 banner `zeroBoot v1.0 for 1506F`, then (per strings)
   `Get RAM Param and configure RAM` → DDR init/parameter programming →
   `copy fw to RAM` → `jump to second boot!!`.
7. Stage-2 (`Boot form SPI v1.13`, image offset 0x8000) performs full DDR
   eye/deskew training with extensive UART prints, verifies signatures
   (`Failed to verify signature in sboot,cal value`), then decompresses/copies
   the main firmware into DRAM and jumps. The encrypted payload header at
   0x10000 names destination **0x81600000** (kseg0 DRAM) with size 0x1B000 —
   a **Probable** load address for one boot component.

**COP0 registers the boot code actually touches** (scan of first 64 KiB,
`tools/fw_cop0.py`) — the minimum COP0 model required to boot:

- reads: `Config1(16,1)`, `Status(12,0)`, `Config(16,0)`, `ErrorEPC(30,0)`,
  `15,1`
- writes: `TagLo/TagHi(28/29, sel 0 and sel 2)`, `Config(16,0)`,
  `ErrorEPC(30,0)`, `Count(9,0)`, `Cause(13,0)`, `Status(12,0)`

## 6. Address-space constants used by the loader

Histogram of `lui`+`ori` pairs and bare `lui` halves in the first 64 KiB
(`tools/fw_luimap.py`):

| Address | Hits | Interpretation | Confidence |
|---|---|---|---|
| 0xBFFE0000 (+0x210/0x284/0x2AC/0x1580/0x15CC/0x1524/0x1504/0x1618/0x3E08/0x3E14/0x8000…) | 73 | Primary register/param window touched throughout the loader | **Verified** as used; decode **Unknown** (Q1) |
| 0xBFFC0000 / 0xBFFC0E00 / 0xBFFC1EEC | 15+4 | Stack region (0xBFFC0E00) and loader-side constants | **Verified** as used; decode **Unknown** (Q1) |
| 0x98010000 / 0x98000000 | 74+10 | kseg0 → physical 0x18010000 / 0x18000000 — heavily used window (peripheral or IRAM) | **Verified** as used; decode **Unknown** (Q2) |
| 0x82000000 | 55 | kseg0 → physical 0x02000000 (DRAM data area) | **Probable** (DRAM) |
| 0x80000000 | 16 | kseg0 DRAM base | **Probable** (DRAM) |
| 0xA0000000 | 6 | kseg1 alias of DRAM base (uncached) | **Probable** |
| 0xB0000000 / 0xF0000000 / 0x90000000 | 6/6/5 | Vendor peripheral spaces (kseg1 phys 0x10000000 / 0x30000000) | **Verified** as used; decode **Unknown** (Q2) |
| masks 0x0FFFFFFF / 0xC0FFFFFF / 0xFC7FFFFF / 0xFFC0FFFF | many | address-mask arithmetic (cache/decode helpers) | — |

## 7. CPU backend decision (evaluated against evidence)

| Candidate | Verdict | Reason |
|---|---|---|
| Unicorn Engine 2 (QEMU-derived, MIPS32 LE) | **Rejected for this target** | Empirically probed (tools/unicorn_probe.py, unicorn 2.x): **any instruction fetch from kseg addresses ≥ 0x80000000 fails with `UC_ERR_FETCH_UNMAPPED`**, signed or unsigned. This firmware executes at 0xBFC00000 and dereferences kseg1/kseg0 constantly. Also: native lib per host platform (cmake build), GPLv2. A kseg→physical remap inside guest code is not possible without modifying the guest. |
| QEMU system emulation | Deferred | A real MIPS platform model for SPHE1506 does not exist; integrating QEMU as a library from Rust is a large project. Re-evaluate only if the custom core proves insufficient. |
| Existing Rust MIPS cores (community crates) | Surveyed | No maintained Rust MIPS32r2 interpreter with the COP0/cache surface we need was found at the time of writing. |
| **Custom MIPS32r2 LE interpreter in Rust** | **Selected** | The boot code's instruction repertoire is compact (integer ops + `ext/ins/mul` + COP0 subset + cache ops as no-ops). Pure Rust = no native deps, portable, license-clean, fully traceable. Kept behind a `CpuBackend` trait so a second backend can be added later without touching machine/device code. |

## 8. Open questions and next experiments

| # | Question | Next concrete experiment |
|---|---|---|
| Q1 | What decodes at 0xBFFC0000 and 0xBFFE0000 (SRAM overlay vs registers vs flash alias)? The loader *writes* there and uses 0xBFFC0E00 as stack. | Emulate with hypothesis A (flash XIP at 0xBFC00000; 0xBFFC0000 = RAM overlay, 0xBFFE0000 = register window) and trace every access; a wrong decode shows up as a write fault or an infinite polling loop, which pins the correct model. |
| Q2 | Which peripherals live at 0x98010000 / 0x98000000 / 0xB0000000 / 0xF0000000? | In the emulator, log the first 50 distinct offsets touched per window after DDR init begins; correlate with the loader's print sequence (`DRAM CPU access…`) to name blocks by function. |
| Q3 | Where is the UART (the loader prints via `putchar`-style writes)? | Trace byte-wide stores (`sb`) in tight loops between instruction-run bursts; the first such window is the UART data register. |
| Q4 | What is the main-firmware encryption/compression (0x020000–0x37FFFF)? | After boot progresses far enough, dump the DRAM range the stage-2 decompressor writes and compare structure; consider the sboot signature path. |
| Q5 | Exact reset PC: does the BootROM jump to flash offset 0 mapped at 0xBFC00000, or copy to IRAM first? | Boot-emulation experiment: start PC at 0xBFC00000 with flash mapped there; if the first stores corrupt the instruction stream (stack at 0xBFFC0E00 falls inside the flash window under hypothesis A1), switch to the IRAM-copy hypothesis and record which one survives. |
| Q6 | Core model / `PRId` the loader expects. | Run boot; if the loader reads PRId and branches on it, log the comparison value. |

## 9. Security-relevant observations

- The loader contains signature verification (`Failed to verify signature in
  sboot,cal value`, `TEST KEY`). **Verified** strings; mechanism **Unknown**.
- The image stores default IDs/keys at 0x3F0000+ (MAC-like string, hex key
  block). Published here only because they are factory defaults inside the
  already-public firmware blob.
