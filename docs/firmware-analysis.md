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
| 0x000000–0x007FFF | 32 KiB | 4.5 | **Stage-1 loader** "zeroBoot v1.0 for 1506F": MIPS32r2 code, strings `SPI SCS flow`, `jump to second boot!!`, `Get RAM Param and configure RAM`, `copy fw to RAM`, `TEST KEY`; a data area at 0x1210+ (descriptor table, §15.2) and — **Verified (Milestone L)** — the **driver payload stored PLAINTEXT at 0x2F6C..0x6948** (0x3ADC bytes; see §15.1) | **Verified** |
| 0x008000–0x00FFFF | 32 KiB | 4.5–7.3 | **Stage-2 loader** "Boot form SPI v1.13": DDR controller init, DRAM eye/deskew training prints (`WL_IPRD0`, `REYE_*`, `WEYE_*`, `CTCAL_ERR flag`), `DRAM CPU access FAIL !!!` | **Verified** |
| 0x010000–0x01FFFF | 64 KiB | 7.3 | **eCos module package (Milestone L, §15.3)**: `CED9D8CC` header + 28-byte descriptor table at 0x10010 + the "SP" DEFLATE container of the eCos image (0x1002C..0x186B8) + trailing package data | structure **Verified**; container decode semantics open (§14) |
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

## 10. Runtime findings (Milestones B–E, emulator-driven)

The following register models were derived by *executing* the real firmware in
the emulator and analyzing where it polls (static cross-checks via
capstone in `tools/`). All commands are reproducible; the emulator's
per-window access log (see `run --max-steps …`, "instrumented window access
map") provides the runtime evidence.

### 10.1 System page 0xBFFE0000 (phys 0x1FFE0000) — block map

| Offset | Block | Evidence | Confidence |
|---|---|---|---|
| +0x000..0x600 | param RAM (loader RMWs: 0x210/0x284/0x298/0x2AC/0x038C…) | access log | **Verified** writable RAM-like |
| +0x600..0x620 | **Timer block**: +0x600 = free-running counter (software resettable); +0x60C = 0x8095, +0x61C = 0 written at init | `delay(N)` at mirror 0xB8008880: writes 0 to +0x600, polls until count == 90×N | Counter semantics **Verified**; real tick rate **Unknown** (modeled 1/instruction) |
| +0x620..0x900 | param RAM | access log | Verified |
| +0x900..0x920 | **UART0**: +0x900 = TX data (write = emit byte), +0x904 = status (bit 0 = TX ready — stage-1/2 putchar; **bit 6 = TX idle** — main firmware polls 0x40) | putchar loops at mirror 0xB8000958 (stage-1), 0xA200044C (main fw) | **Verified** TX path; bit-6 semantics = hypothesis H7 |
| +0x920..0x1500 | param RAM | access log | Verified |
| +0x1500..0x16A0 | **DDR controller/PHY cluster** (see 10.2) | DDR-V6.2 training flow | Protocol **Verified** by polls; completion modeled instantly (H6) |
| +0x1580..0x15E0 | DDR PHY training engine: +0x1584 = command (0x0F02/0x0F06), +0x1588 = status (bit 0 = done, bits 9..11 = result), +0x15A4 = result word | training loop at mirror 0xB800C24C | Protocol **Verified** |
| +0x3E00..0x3E40 | **SPI controller**: +0x3E0C = TX word, +0x3E04 = start (0x20 → done bit 0x400; 0x40 → done bit 0x800), +0x3E08 = status, +0x3E14 = RX byte, +0x3E24 = CS config | SCS flow at mirror 0xB800CD60/0xB800CCD8/0xB800CE2C | Protocol **Verified**; RX = TX[15:8] echo = **hypothesis H3** |
| +0x3E40..0x10000 | param RAM (incl. 0xBFFE3E08 polled w/ timeout, key/status 0xBFFE384C) | access log | Writable RAM-like **Verified**; 0x384C decode **Unknown** |

### 10.2 DDR init flow (stage-2, "DDR-V6.2.01.09-v")

1. Config RMWs across 0xBFFE0040/0x0048/0x1500..0x1520 (clock/reset gating).
2. PHY training command sequence (0x1584 ← 0x0F02/0x0F06; poll 0x1588.0) —
   prints WL/REYE/WEYE/RSL/PHA/PSD metric groups.
3. Controller command issue: +0x1628 strobes (bits 0x20/0x01/0x02/0x100/0x200),
   argument register +0x166C, command register +0x1660; polls 0x1628 bits
   17 (cmd ack) and 18 (DRAM init done); also polls 0x1504 bit 2 (with a
   10M-iteration timeout — the ONLY observed bounded wait) and 0x1600 bit 0.
4. "Trim PASS" / "DRAM-1G" (the SoC's own DRAM size report — 1 Gbit = 128 MiB;
   the emulator's 64 MiB DRAM model is still sufficient because the payload
   loads at phys 0x02000000).

### 10.3 Payload copy and main-firmware entry

- Descriptor table (stage-1 data area, file offsets): [0x1214] = **0xA2000000**
  load address, [0x1218] = 0x00010000, [0x121C] = 0x0000135C,
  [0x1220] = 0x00003ADC size, [0x1224] = 0xB56FB05D (checksum word,
  §15.2/Q8), [0x1228] = 0x7F. Milestone L resolved the mechanism (§15.1):
  the payload is **stored plaintext at flash 0x2F6C** and "copy fw to RAM"
  is a byte-for-byte copy; the interpretation of the [0x1218]/[0x121C]
  source fields against the 0x2F6C placement is **open (Q8)**.
- "copy fw to RAM" + "Go!": the loader copies the payload to kseg1
  0xA2000000 (phys 0x02000000) — **Verified plaintext copy** (Milestone L,
  pinned by the test `driver_payload_is_stored_plaintext_in_flash`); no
  crypto or decompression is involved in this step.
- "1ST BOOT" banner: printed via **unrolled immediate stores** (PCs
  0xA2000340..0xA2000448) — the banner bytes never exist in memory, which is
  why file/DRAM scans cannot find them.
- Steady state (current milestone): the eCos application polls the
  key/status register 0xBFFE384C (bits 0/2/4/5/6) with gp-relative state at
  0x82003B1C — an idle wait for front-panel input. Modeled register reads 0,
  matching a receiver with no key pressed. See §12 for the full decode and
  the injection path.

### 10.4 Instruction-set additions discovered in the main firmware

| Instruction | Encoding | Where |
|---|---|---|
| `lwx` (DSP ASE indexed word load) | SPECIAL3 funct 0x0A, sa 0x00 | eCos memset/copy routines |
| `lhx` (DSP ASE indexed halfword load) | SPECIAL3 funct 0x0A, sa 0x04 | same |

### 10.5 Honest status of the emulated boot (as of this milestone)

**Actually demonstrated** (deterministic, covered by tests):
reset → zeroBoot (stage 1) → "jump to second boot!!" → "Boot form SPI v1.13"
DDR training to "Trim PASS" / "DRAM-1G" → payload decompress/copy →
"Go!" → main firmware "1ST BOOT" → "Zero BSS" → "IntCtrl" →
"LD eCos v1.16 1506f" → idle status poll (~1.3M instructions, ~0.4 s).

**Not demonstrated / unknown**: video/audio pipeline, tuner/demod lock,
Wi-Fi, remote control, channel database, and any register semantics not
listed above. The main-firmware payload format (file 0x10000+) is treated
as an opaque blob transformed by the firmware itself; its codec is **not
yet identified** (Q4).

## 11. Interrupt controller (Milestone G, emulator + DRAM-dump evidence)

The decompressed main firmware was dumped from emulated DRAM (phys
0x02000000, kseg0 alias 0x82000000) after a full boot and disassembled
with capstone (`tools/` scripts from this milestone live in the session
analysis tree; every claim below was cross-checked against the final
sysreg register state of the same run). Banner print sites in the image:
`1ST BOOT\r\nZero BSS` @0x33C, **`IntCtrl` @0x59C**,
`LD eCos v1.16 1506f` @0x6CC, plus `Go eCos`/`Boot from SPI` @0x9D4 and
`LD eCos fail` @0xC48 (alternate boot paths of this SDK loader that the
observed boot does not take). All banners are printed via unrolled
immediate stores to the UART — the strings exist only inside the
instruction stream.

### 11.1 What the "IntCtrl" step actually programs — VERIFIED by execution

The code after the `Zero BSS` banner calls a computed function at DRAM
0x82000E80 immediately before printing `IntCtrl`. With `$s6 = 0xBFFE8000`
its stores are (all values **confirmed identical** in the post-boot sysreg
dump):

| sysreg address | value | note |
|---|---|---|
| 0xBFFE0480 | 0xFFFFFFFF | enable/config word A |
| 0xBFFE0484 | 0xFFFFFFFF | enable/config word B |
| 0xBFFE0488 | 0x00001F2F | config |
| 0xBFFE048C | 0x00000007 | config |
| 0xBFFE0490 | 0xFFFFFFFF | config word C |
| 0xBFFE0494 | 0xFFFFFFFF | config word D |
| 0xBFFE0498 | 0x0000017C | config |
| 0xBFFE049C..0xBFFE04B0 | 0 | zeroed block |
| 0xBFFE0014 | 0 (cleared bits 1:0) | see below |
| 0xBFFE0040 | 0x14 (later 0x0C) | clock/gating block (rewritten elsewhere) |
| 0xBFFE0B00 / 0xBFFE0B04 | 0x114 / 0x105 | written twice each |
| 0xBFFE0B24 | 0x40 (bit 6 set by RMW) | see below |

Two helper primitives surround the block init:

- **0xBFFE0014, bits 1:0 — CPU interrupt-line control** (function at
  0x8200117C): arg 1 → `reg |= 3`; arg 2 → `reg &= ~3`; read-back →
  `reg & 3 ? 2 : 1`. IntCtrl init calls it with arg 2 (**disable**) before
  configuring. Final value 0. Purpose: hypothesis **H8** (an
  enable/disable/query API on a fixed register is the classic CPU-IRQ-line
  switch; not proven).
- **0xBFFE0B24 bit 6** — RMW'd (`|= 0x40`) in the entry code
  (0x82000268..0x82000270) immediately *before* the COP0 Cause/Status
  programming. Purpose: hypothesis **H10** (a global IRQ gate; not proven).

### 11.2 COP0 interrupt state — VERIFIED (and the decisive finding)

At 0x8200025C..0x82000288 the main firmware executes:

```
addiu $k0, $zero, 3
mtc0  $k0, Config          ; Config.K0 = 3
ehb
lw    $k0, -0x74DC($s6)    ; 0xBFFE0B24
ori   $k0, $k0, 0x40
sw    $k0, -0x74DC($s6)    ; H10 gate
mtc0  $zero, Cause         ; Cause = 0
lui   $v0, 0x1100
ori   $v0, $v0, 0xFF00
mtc0  $v0, Status          ; Status = 0x1100FF00
```

`Status = 0x1100FF00` decodes as: **IM[7:0] = 0xFF (all eight interrupt
masks unmasked)**, MX=1 (DSP ASE), CU0=1, **BEV=0** (vectors at EBase, not
the boot ROM), **IE = 0 — global interrupt enable stays OFF**, ERL=0,
EXL=0, kernel mode.

Exhaustive scans of the whole 16 MiB decompressed image:

- exactly **one** `mtc0 Status` and **one** `mtc0 Cause` (the pair above);
  no further writes;
- **zero** `di`/`ei`/`eret`/`wait` instructions (strict encoding scan,
  bits 15:6 == 0);
- one `mfc0 EBase (15,1)` used only for a `& 0xF` single-core check at
  0x820000A8; **EBase is never written** (reset value 0x80000000 stands);
- Count/Compare never programmed (no `mtc0 $11`).

**Conclusion (Verified):** the observed boot *configures* interrupt state
(masks unmasked, vectors selected, SoC-side enables programmed) but
**never enables interrupts** (`Status.IE = 0` forever). Consistently, the
BEV=0 vector area (kseg0 0x80000000..0x80000200) contains only zeroes and
the DRAM-test fill marker 0x55555555 at 0x80000000 — **no VSR/handler is
installed** in the traced horizon (30M steps, ending in the steady-state
front-panel key poll). An interrupt firing with IE=1 would vector into
zeroed RAM; the firmware evidently relies on enabling it later (paths not
reached in this horizon).

### 11.3 What is NOT known (and therefore NOT modeled)

- pending/ack register semantics of the 0xBFFE0480..0x4B0 block (no reads
  observed anywhere in the executed path; static scan cannot see dynamic
  pointer accesses, but none of the traced code polls it);
- per-source → vector mapping, priority order, steering to IP2..IP7;
- the meaning of the config words 0x1F2F / 0x7 / 0x17C;
- whether 0xBFFE0014 or 0xBFFE0B24 gates the IRQ line to the CPU, and on
  which side of the aggregation.

### 11.4 Emulator implementation (this milestone)

- **CPU (architecture-specified, not speculative)**: interrupt delivery in
  `Mips32::step` — an enabled (`IE=1`), unmasked (`Cause.IP & Status.IM`),
  non-nested (`EXL=0, ERL=0`) interrupt vectors before the next fetch;
  EPC/`Cause.BD` latch the interrupted context (delay-slot aware),
  `Cause.ExcCode=0`, `Status.EXL=1`. Vector: BEV=1 → 0xBFC00200+offset;
  BEV=0 → EBase (reset 0x80000000) + offset; `Cause.IV` selects 0x200 over
  0x180. `eret` now implements both paths (ERL→ErrorEPC/ERL=0,
  EXL→EPC/EXL=0) — the previous ErrorEPC-only behavior was a demonstrated
  correctness defect. `mtc0 Cause` write mask: only IV/DC/IP[1:0]
  writable; BD/ExcCode/IP[7:2] are hardware-owned. Hardware lines drive
  `Cause.IP[7:2]` via the new `CpuBackend::set_ext_irq_lines`.
- **Device (hypothesis H11)**: `IntController` at sysreg+0x480, 0x40 bytes
  — plain storage for the eight verified words + a level-triggered
  aggregate output (`sources & (en@+0x480 | en@+0x484)` → line, wired to
  CPU IP2 by hypothesis). No latch/ack invented; sources are injected
  through a shared handle (`Machine::raise_int_sources` /
  `clear_int_sources`), which is the intended virtual-device injection
  path. During a real boot no source is ever raised and `IE=0` blocks
  delivery, so the acceptance boot is bit-identical (asserted by
  `real_firmware_boot_is_unaffected_by_irq_wiring`).
- **Tests**: 7 CPU KATs (delivery/eret/BD/BEV/EXL-nesting/Cause-mask/
  level-retrigger), 4 COP0 KATs, 1 device KAT (replays the verified init
  program and checks the level output), 2 machine integration tests
  (injection through the public API; lockstep determinism) + the
  real-firmware regression guard.

## 12. Front-panel key port 0xBFFE384C (Milestone G, decoder + injection)

The steady-state poll loop of the decompressed main firmware (DRAM
0x82002548..0x82002630, `$s6 = 0xBFFE8000`) gives the complete register
decode — **Verified** by disassembly and by executing a virtual key press
under the emulator:

| register | direction | semantics |
|---|---|---|
| **0xBFFE384C** (word) | read | raw key-line bits; the dispatcher masks 15:0 and tests **bits 0, 2, 4, 5, 6** (`andi $a0,$v0,1`, `ext $a0,$v0,2/4/5/6,1`). 0 = no key (steady state). |
| **0xBFFE384C** (word) | write | the dispatcher **writes the detected key's keycode into the same port**: bit 0 → `0x100`, bit 2 → `0x400`, bit 4 → `0x1000`, bit 5 → `0x2000` (`$t1/$t4/$t5/$t6`, set up at 0x82002540..0x82002550). Keycodes occupy bits 8..13, disjoint from the raw key lines. |
| 0xBFFE387C | r/w | latch sampled by the bit-4 handler (`lw` before/after a 0→1 write to 0xBFFE3878); two samples combined `sample<<16 | sample` into gp state. |
| 0xBFFE3878 | write | 0 → 1 strobe around the 0xBFFE387C sample (trigger). |
| 0xBFFE3824 / 0xBFFE3820 | read | read by the bit-6 path and the loop-exit path (values from the boot-time block at 0xBFFE3800+). |
| 0xBFFE3874 | — | loaded into `$t7`, not observed read in the dispatcher. |

Bit 6 takes a separate long path (`beql` → 0x82002ECC → `lw -0x47DC($a2)`
= 0xBFFE3824 → j 0x82002308) — likely the standby/power key with dedicated
handling. Button *names* for bits 0/2/4/5 are not decoded here (the
keycode consumer lives outside the dispatcher; mapping keycodes to UI
actions is future work).

### 12.1 Emulation model and virtual button injection

`devices::KeyPort` (4 bytes carved at sysreg+0x384C):

- **Read** = current pressed-key level from a shared handle — level
  semantics, i.e. a virtual button press sets bits and a release clears
  them. While a line is held the dispatcher re-detects it every pass and
  emits the keycode again (observed in the emulator as a stream of 0x400s
  — the auto-repeat a physically held key would produce). Consumers
  wanting single-press semantics should release after the first event.
- **Write** = captured verbatim into a shared key-event queue
  (`Machine::take_key_events`). Writes do not alter the pressed level
  (documented falsifiable choice: latch/ack-on-write would read back 0;
  the level model matches all observed traffic).
- **Injection API**: `Machine::press_key(bits)` / `release_key(bits)` /
  `take_key_events()` — the intended front-end for a virtual front panel.

End-to-end proof (`tests/interrupt.rs::
real_firmware_reacts_to_injected_key_press`): boot the real firmware to
the idle poll, `press_key(0b0100)`, run 20k instructions → the firmware's
own dispatcher writes keycode `0x400` into the port (event queue gains
`[0x400, …]`); after `release_key` the queue stays empty. The response is
produced entirely by the firmware's scan code — the emulator injects only
the electrical level, never a fake event.

### 12.2 Unknowns

- Physical button labels per bit (needs the keycode consumer).
- Whether a real keypad ACK/latch exists behind the write (level model
  chosen; would show up as duplicate events if wrong).
- Semantics of the 0xBFFE3800..0x387C cluster beyond the sampled
  registers (boot-programmed values recorded in the sysreg dump).

## 13. Hardware decompression engine 0xBFFE3800 (Milestone H, boot-stage evidence)

Milestone H identified why the boot parked in the "key-scan" poll: the loop
is **the decompression engine's wait loop** (keys stay live inside it), and
the blocker was a peripheral, not a UI idle loop.

### 13.1 The "LD eCos" pipeline — VERIFIED by execution trace

After the "LD eCos v1.16 1506f" banner the loader (executing at
0xA2000000/0x82000000, the decompressed 0x3ADC-byte payload):

1. reads a 28-byte descriptor from flash 0x10010 via the ops table at
   0x82004BA0 (ops[4] = SPI byte-read through the 0x98000000 flash mirror):
   `src=0x1002C, len=0x868C, dst=0x81600000, flags&3=1`;
2. for flag==1: zero-fills [0xA2040000, 0xA2088000) (0x48000 bytes — the
   engine's 32 KiB LZ window at 0x02040000..0x02048000 plus 256 KiB of
   scratch at 0x02048000..0x02088000), copies the container
   flash→0xA2088000;
3. calls the driver at 0xA2001F50 with (image, 0x81600000, 0).

### 13.2 The driver's engine program — VERIFIED register writes

* clock gates: 0xBFFE0024 |= 0x2000, 0xBFFE0044 |=/~ 0x2000;
* engine regs (complete program, re-verified from the emulator's
  `decomp-engine` access log — `Machine::access_log`, example
  `examples/engine_probe.rs`): 0xBFFE3800 = 1 then 0x90 (mode); 0xBFFE3814
  = 0; 0xBFFE3840 = 0x20; 0xBFFE3858 = 0xC0; **input base** 0xBFFE3804/0808
  = 0x0208/0x8000 (=0x02088000, the copied image); **input end**
  0xBFFE380C/3810 = round128(src + container_len) = 0x0209/0x0700 (the
  driver computes `(base + len + 0x7F) & ~0x7F` at 0xA20021D4..E8 and
  splits it hi/lo — the 128-byte-aligned end of the compressed input);
  **window base** 0xBFFE385C/60 = 0x0204/0x0000 (0x02040000) with end
  0xBFFE3850/54 = 0x0204/0x8000 (32 KiB); **output** 0xBFFE386C/70 =
  0x0160/0x0000 (=0x01600000); 0xBFFE3864 = type (1); **input length**
  0xBFFE3834/0x3838 (32-bit) = 0x8692 — the container length 0x868C **+ 6**
  added at 0xA2002224; 0xBFFE3830 = 0x68 (constant, semantics unknown);
  **block header** 0xBFFE383C = `(BTYPE << 1) | BFINAL` = 5 (built at
  0xA20022C0..E4 from the driver's own bit-reader parse of the stream
  header); chunk registers 0xBFFE3828 = 0x400, 0xBFFE382C = header u16@2;
  event-countdown mirror 0xBFFE3874 = limit − 1 = 77;
* the driver parses the container header: bytes[0:2] = "SP", u32@4 (BE) =
  0x8684 = stream length, then a DEFLATE dynamic block header — HLIT/HDIST/
  HCLEN at 5/5/4 bits + 17×3-bit code-length codes (all standard RFC 1951);
* it builds the Huffman tables **in software** (DRAM 0x82005074+) and
  uploads them through the command/data pair 0xBFFE3844/0xBFFE3848:
  round A: 0xC000 + 285 lit/len symbols in code order, 0xC180 + 15
  first-code halfwords, 0xC190 + 15 cumulative halfwords; round B:
  0xC000, 0xC1C0 + 28 distance symbols, 0xC1A0 + first codes, 0xC1B0 +
  cumulative; each round ends with a 0x0000 terminator;
* **verification**: the uploaded arrays are exactly the canonical Huffman
  codes of the stream's own code lengths (uploaded first[] = canonical
  first-code table, cum[] = cumulative counts, symbol list = symbols sorted
  by (length, code)) — the upload and the stream agree bit-for-bit;
* start: 0xBFFE3824 = 1, then the poll loop at 0xA2002548 (which is also
  the front-panel key scan — keys remain live during the wait).

### 13.3 The wait loop's exits — VERIFIED control flow (decisive re-read)

The poll loop at 0xA2002548 checks the key lines **in order 0, 2, 4, 5, 6**
(all branch targets are handlers for the line being 1 — v1 = 1 throughout,
set at 0xA200253C):

* **bit 6 → failure epilogue.** 0xA2002ECC/0xA2002EE4 → 0xA2002308 (clears
  bit 13 of **0xBFFE0024**, the clock-gate register — the negative offset
  lands there, not on 0x3824) → epilogue with **v0 = 0** (delay slot
  `move v0, zero` at 0xA200231C). The loader's check at 0xA2001BB4 turns
  v0 = 0 into "LD eCos fail". Bit 6 alone is therefore an *abnormal*
  completion; modeling the engine as asserting bit 6 on completion is
  what produced the earlier "LD eCos fail" dead-end.
* **bit 5 → the success path.** The handler writes keycode 0x2000 (the
  ack) into 0xBFFE384C, then computes `A_new = A + B + 1` from counters
  A = 0x82003B14 (reset to 0 before polling) and B = 0x82003B18
  (**initialized to u16@2 − 1 = 77 at header parse**, also mirrored into
  engine register 0xBFFE3874), against the limit 0x82003B20 =
  u16@2 = 78. With B = 77 the **first event** yields A = 78, the
  `(A < limit)` test fails, the event flag at 0x82003B1C is set, and
  execution enters the accounting block at 0xA2002630.
* The accounting block reads engine registers 0xBFFE381C and 0xBFFE3820
  (fields of the latter: bits 15:13 accumulated into 0x82003B00, bits
  12:3, bits 2:0; the register is acked with a zero write), computes
  `(R381C + (acc << 16)) * (8 << state)` + field10 with state =
  0x82003AF4 = 4, stores the result, gates the engine clock off
  (0xBFFE0024 bit 13) and **returns v0 = R3828 × limit = 0x400 × 78 =
  0x13800** (non-zero → success).
* Loader consequence (0xA2001BB4): non-zero → returns desc[2]
  (0x81600000) → the caller masks the address into kseg1, flushes
  caches, prints "Go eCos" and "Boot from SPI", and `jr`s to
  0xA1600000.

The wait-loop state at poll time is pinned by the DRAM dump: A = 0,
B = 77, limit = 78, flag = 0, header byte counter = 134 (container-
relative; the parse pointer sits at image+0x86), state = 4.

### 13.4 Engine decode semantics as modeled (and the open residue)

Modeled (devices::DecompEngineLo/Hi + EngineShared, machine service loop,
`inflate::inflate_engine`):

* register storage and the start handshake (0xBFFE3824 bit 0 → the run
  loop services the request);
* the decode: tables re-derived from the stream's dynamic block header
  (bit-for-bit equal to the uploaded tables — the uploads are recorded
  but not interpreted), symbols from the **programmed start bit**
  (0xBFFE3818 = 1088 container bits = **1024 stream bits**; the 16 bits
  between the 1008-bit standard header end and 1024 are zero padding,
  verified in the stream), running to the end of the input length
  register 0xBFFE3838 = 0x8692, with **end-of-block symbols treated as
  markers, not terminators** (the driver expects the whole declared
  stream; a zlib-style stop at the first BFINAL block leaves the image
  truncated at 28,256 of the expected bytes);
* completion raises the **event line (bit 5)**, cleared by the driver's
  0x2000 ack write.

Verified result: the real, unmodified firmware boots through
"LD eCos v1.16 1506f" → engine decode (75,657 bytes written to
0x01600000 through the bus) → bit-5 event → driver returns 0x13800 →
**"Go eCos" + "Boot from SPI"** → jump executed to 0xA1600000
(~1.31 M instructions, deterministic). This is the first time the boot
chain enters the eCos stage.

**Open — the engine's LZ value semantics (the next blocker).** The bit
stream parse is provably exact (the whole 275,488-bit input decodes as a
valid symbol sequence under the uploaded canonical tables; a ±1 code
boundary shift fails immediately), but the output bytes are not yet valid
MIPS code (~83% word-level validity vs ~99% for known-good code from the
same toolchain; byte-alignment phase drifts). Ruled out empirically:
extra-bit polarity (LSB/MSB), distance base offsets ±1, distance-list
rotations, halfword/byte swaps, code-bit reversal (fails instantly),
lengths-at-256 remapping (dies early), and alternate start bits (1008/
1072/1088 stream-relative). The engine's length/distance base tables and
window mechanics are hardware constants not present in the firmware
image; they will be pinned down from the eCos stage's execution traces
(the first faulting instruction localizes the first wrongly decoded
region) — see §14.

### 13.5 What the success path proves

Every value on the driver's success path is real firmware state — no
fabricated responses: the counters (A/B/limit), the event flag, the
engine registers (R3828 = 0x400, the limit mirror in 0xBFFE382C, the
countdown in 0xBFFE3874) are all written by the firmware's own code
before the poll. The only emulated behavior is the engine completing
its decode autonomously and raising its event line — the DMA-style
semantics the register program implies. The front-panel keys remain
live inside the wait loop (the poll is shared), and the virtual button
injection (§12) keeps working through it.

### 13.6 The driver's engine interface — fully decoded (Milestone J), and
### the emulator-side decode defect it exposed

Disassembly of the driver (payload 0xA2000000+0x1F50..0x2790, committed
analysis in `tools/fw_driver_disasm.py`) pins every piece of the engine
interface:

* **Start bit (0xBFFE3818)**: the driver computes
  `header_end = stream_len + 8 + bfinal` (container bits), rounds the
  byte count up to a 128-byte boundary (`(x + 0x7F) & ~0x7F` — the
  observed 1024-bit stream start), adds the 64 container-header bits,
  masks `& 0x7FFF`, and adjusts to a 32-bit boundary when
  `header_bits & 0x1F != 0` (path at 0xA200272C adds 0x20). For this
  container: 1008 header bits → 1024 stream bits → R3818 = 1088.
* **Table uploads are writes to engine-internal table RAM**, addressed
  by the command word (0xBFFE3844 = command `0xC000 | addr`, data on
  0xBFFE3848): lit symbols `0x000`, lit first-codes `0x180`, lit
  cumulative counts `0x190` (15 halfwords each), dist first-codes
  `0x1A0`, dist cumulative `0x1B0`, dist symbols `0x1C0`. Each round
  arms with `0xC000`, waits for ready (R3824 bit 1), then uploads.
  The tables are built in software at DRAM 0x82005074 (lengths),
  0x82005578 (counts), 0x820055B8 (cumulative), 0x820055F8 (symbol
  table) by the builder at 0xA2001C5C — a canonical counting sort:
  `symtab[exclusive_cumulative[len[s]]++] = s` for each symbol with
  `len[s] != 0`.
* **The uploaded tables are canonical, with one indexing convention**:
  the DRAM dump of the built tables (post-boot, still intact) shows the
  uploaded `first[L] = first_canonical[L+1]` (e.g. dist: first[5]=16,
  first[6]=60, first[7]=126 — exactly the canonical first codes of
  lengths 6/7/8 for count[5]=8, count[6]=14, count[7]=3), i.e. the
  engine compares a code of length L against `first[L-1]`; the
  cumulative table is inclusive (`cum[L] = Σ_{l≤L} count[l]`), giving
  the exclusive symbol offset as `cum[L-1]`. Net effect: the code →
  symbol assignment is the standard RFC 1951 canonical assignment.
  Only 28 of the 30 dist symbols have codes (lengths 5–9); the lit
  upload is 285 words (286 minus the one zero-length symbol).
* **Completion accounting** (0xA2002630): the block reads R381C and
  R3820, accumulates R3820 bits 15:13 into a gp counter (acking R3820
  with a zero write when nonzero), and computes
  `(R381C + (acc << 16)) * (8 << state) + field10` with
  `state` = gp variable = 4 → a ×128 unit count (624 × 128 = 79,872 =
  78 × 1024, corroborating the chunk-programmed output size). The
  driver's return value is simply `R3828 × limit = 0x400 × 78`.
* **The wait loop's countdown** (gp[-0x7FB4]) is preloaded to
  `limit − 1` so the *first* event the (slow) poll observes completes
  the count — the engine fires one event per 0x400 output bytes and
  the poll cannot catch them all at hardware speed.

**The decode defect (found and fixed in this milestone).** The
emulator's `inflate_engine`/`inflate_block` match copy read its source
from a *frozen* index: `let n = window.len()` was captured before the
copy loop and `window[n - d]` was re-read for every byte of the match.
Every match therefore produced a constant fill of its first source
byte instead of a sliding-window copy. The defect is proven, not
suspected: re-implementing the frozen-pointer semantics in the
analysis tools (`tools/fw_engine_lz2.py`) reproduces the previous
engine-model output **byte-for-byte (100.000% over all 75,398 bytes)**.
The first visible corruption (output byte 195, a `len=7 dist=8` match
decoded as seven copies of `0xCF`) is exactly the first match whose
source run was non-constant; earlier matches agreed only because the
region is zero-dominated. A unit KAT with an overlapping
(dist=2, len=8) match now pins the live-window semantics
(`overlapping_match_replicates_from_live_window`).

**Consequence for §13.4's open items**: the frozen-pointer fill decode
was the vehicle for every earlier "engine semantics" experiment, so
measurements quoted there (~83% word validity, the byte-alignment
phase-drift claim, and the ruled-out list) described the *buggy*
realization, not the hardware. They are superseded by this section and
by §14.

## 14. Next milestone — the engine's LZ value semantics (revised twice)

With the decode defect removed (§13.6) and the register program fully
decoded (§13.2), the state of the art after the Milestone K re-examination:

### 14.1 What is certain

* The bit-stream parse is exact and independent: a from-scratch Python
  canonical-Huffman walk (`tools/fw_engine_lz.py`) decodes all 25,027
  symbols (15,761 literals, 9,264 matches, 2 end-of-block markers) to the
  end of the declared stream with **zero bad codes**; the engine consumes
  exactly the RFC 1951 bit positions (same Huffman codes, same extra-bit
  counts). Any hypothesis that changes what bits are read is dead.
* The 15,761 literal bytes are the image's true bytes (a literal symbol is
  a byte on any sane engine), up to their positions.
* The 458 matches whose RFC distance exceeds the current output position
  are **not** evidence of a wrong distance table: the driver zero-fills the
  32 KiB window before starting the engine (§13.1), so a compressor with a
  32 KiB zero preset dictionary legitimately emits them (they copy preset
  zeros). The window-legality "oracle" is therefore void, and the
  compressor is a standard deflate with a preset dictionary.
* The engine's completion accounting (§13.6) computes
  `(R381C + (acc<<16)) * 128 + field10 = 624 × 128 = 79,872` — consistent
  with either (a) an exact decode of 79,872 bytes (Σ match lengths then
  must be RFC + 4,474), or (b) an exact decode of RFC's 75,657 bytes with
  the engine rounding output up to full 1 KiB chunks (78 allocated vs 74
  needed — the header's 78 then over-allocates by 4 chunks). Both readings
  stay open.

### 14.2 Calibrated output-validity oracles (new — prior numbers were unreliable)

Word-"validity" scoring is only meaningful after calibration against
known-good data from the same image (reproduce with capstone; the numbers
below are this document's reference):

| Data | Lenient (capstone decodes) | Strict (common code instructions only) |
|---|---|---|
| stage-1 zeroBoot (pure code) | 98.8% | 91.1% |
| stage-2 DDR loader | 97.7% | 94.8% |
| satellite DB (pure data) | 34.5% | 23.5% |
| **uniform random words** | **62.5%** | — |
| RFC 1951 decode of the SP container | 61–62% | 45.9% |

Two conclusions: (1) the lenient metric cannot distinguish garbage from
data (random scores 62.5%), so earlier "~79–83% valid" claims were noise
around the garbage level; (2) under the strict metric the RFC decode
(45.9%) is far from code (91–95%) — the match-produced ~79% of the image
is genuinely wrong, not "partly valid code".

Byte statistics agree: the decode's byte histogram matches known-good code
closely (cosine 0.975–0.986) with **no** XOR/ADD transform (constant key
correlation peaks at 0), but its zero-byte ratio is 24.7% vs 39.6–57.3%
for real code and its entropy is 6.22 bits/byte vs ≤5.15 for real code —
the decode has too few zero runs and too much entropy, i.e. matches copy
plausible-but-wrong content.

### 14.3 Systematically refuted (clean data, strict metric; tools this milestone)

* window presets 0x00 and 0xFF (the head is literal-certain in both);
* output transforms: word/halfword rotations (broad "gains" are a
  validator artifact — pure data also "improves" from 31% to 83% under
  rotation), byte-lane swaps/reversals, per-position XOR/ADD families
  (i·a, i>>s, masks 0x0F..0xFF);
* length remaps: `eb+k` (k=3..5), `base+eb±1`, `base` alone;
* distance remaps: `deb+k` (k=1,2), `base` alone, `base+deb±1`,
  `nextbase−deb`;
* ring-buffer indexing: absolute (`ring[dist±1]` instead of
  `ring[write−dist]`) forward and reversed, plus reversed relative copies;
* (from Milestone J, unchanged) extra-bit polarity, code-bit reversal,
  distance-list rotations, halfword/byte swaps of the output,
  lengths-at-256 remapping, alternate start bits.

### 14.4 What this forces

The symbol stream, the Huffman assignment, the literal bytes and the
engine's zero-preset 32 KiB ring are all verified; a *table-level* remap
of lengths or distances is refuted far beyond ±1; and per-match greedy
corrections recover local validity (97.5%) but with mutually inconsistent
deltas — so the wrongness is **structural**, not a wrong base table. The
remaining hypothesis classes:

1. the engine's copy hardware has byte-lane/position-dependent behavior
   the driver never exercises in a way the header reveals (mode word
   0xBFFE3800 = 0x90, and the unknown constants 0xBFFE3830 = 0x68,
   0xBFFE3840 = 0x20, 0xBFFE3858 = 0xC0 are the only unexplained
   programming inputs left);
2. the container's plaintext is transformed *before* compression by the
   SDK tool (the engine would then decode correctly and the loader jumps
   into a transformed image — but the boot executes it directly, so the
   transform must be code-compatible, e.g. a byte-lane permutation done
   at link time; the head literal `60 81 00 00` is exactly the load
   address 0x81600000 >> 16, which fits a header word, and the first ~111
   bytes decode to a sparse non-code table under any length mapping);
3. the descriptor's flag==1 path is not the engine path that produced the
   shipped boots (flags 2/3 would select a different engine mode).

The concrete next steps: (1) disassemble the stage-2 decompressor that
produced the known-good 0xA2000000 payload from the 0xCED9D8CC container —
it is the only other consumer of the SDK's compression tool and will name
the container/codec and any pre-transform (Q4); (2) search the main
firmware's eCos routines for the engine's register block usage (the main
firmware talks to 0xBFFE38xx again after boot, which may expose more
modes); (3) execute the eCos-stage image with `run --trace` from
0xA1600000 and map strict-validity against the producing match classes
per 1 KiB chunk to test whether corruption is chunk-periodic (which would
point at the 0x400-granularity copy hardware).

## 15. Milestone L — the boot chain's container formats resolved; engine LZ
## narrowed by five refutation families

Every claim below is reproducible from a fresh clone with the committed
tools (`tools/fw_*.py`, capstone ≥ 5) and the pristine flash image.

### 15.1 The driver payload is stored PLAINTEXT (Q4 resolved for the driver)

The stage-2 loader's "copy fw to RAM" step does **not** decrypt or
decompress anything. The driver payload (0x3ADC bytes) is stored verbatim
in the flash at file offset **0x2F6C** — inside the stage-1 32 KiB region,
not in the 0x020000+ main-firmware area:

* post-boot DRAM dump (phys 0x02000000, 1.4 M-step boot) vs
  `flash[0x2F6C..0x2F6C+0x3ADC]`: **15068/15068 bytes = 100.000% verbatim**;
* the payload is surrounded by zero padding; its code statistics are real
  (strict validity 98–100% over the whole image, 61.8% zero bytes);
* pinned in code by `tests/boot.rs::driver_payload_is_stored_plaintext_in_flash`.

Consequences: the SDK ships this module uncompressed; the "encrypted/
compressed" label of §4 applied to the wrong region for this module; and
the eCos "SP" container is the **only** compressed artifact in the
observed boot chain. There is no software DEFLATE decoder producing
known-good output anywhere in the boot code — the hardware engine is the
only consumer of "SP" containers.

### 15.2 The stage-1 data area descriptor table and two checksum anchors

The stage-1 data area (0x1210+) holds a table whose first entry reads:

| file offset | value | reading |
|---|---|---|
| 0x1210 | 0x00000018 | entry/handle word (semantics open) |
| 0x1214 | 0xA2000000 | load address (kseg1 DRAM) |
| 0x1218 | 0x00010000 | source field (interpretation **open**, Q8) |
| 0x121C | 0x0000135C | second source field (**open**, Q8) |
| 0x1220 | 0x00003ADC | payload size — matches the plaintext payload |
| 0x1224 | 0xB56FB05D | checksum word (**open**, Q8) |
| 0x1228 | 0x0000007F | flags/config (**open**) |

Later entries reference 0xBFFE0000 (the sysreg window) and look like
register-init data rather than descriptors. Two checksum anchors are now
known — once the algorithm is recovered from the loader code, both must
validate:

| coverage | expected checksum |
|---|---|
| driver payload (flash 0x2F6C..0x6948) | `0xB56FB05D` (desc word @0x1224) |
| eCos descriptor (§13.1) | `0xB5EBF9FF` (desc[6] @0x10028) |

Neither simple sums (u8/u16/u32 LE/BE), XOR, CRC32, Adler-32 nor the BSD
sum over the candidate regions reproduces either word
(`tools/fw_checksum_oracle.py`); the algorithm lives in as-yet-unlocated
loader code. **Q8: locate the checksum routine and validate both anchors.**

### 15.3 The 0x10000 package: header, descriptor stride, and flag dispatch

The 0xCED9D8CC package at 0x10000 decodes as: 12-byte package header
(magic, version 1, 0x40000000, size 0x1B000) followed by a **descriptor
table whose entries are 28 bytes each at 0x100010 + index*28** (disassembly
of the loader at 0xA200197C..0xA2001C30: `sll/sllv/subu` computes index*28,
`jalr ops[1]` reads 0x1C bytes into the stack frame). Descriptor fields:

| word | meaning (eCos entry values) |
|---|---|
| 0 | source flash address (0x0001002C) |
| 1 | container length (0x0000868C) |
| 2 | destination (0x81600000) |
| 3 | length copy (0x0000868C) |
| 4 | method flags &3 (1 = hardware engine) |
| 5 | driver argument, passed as `a2 & 0xFFFF` (0) |
| 6 | checksum 0xB5EBF9FF — **never read by the loader** |

The flags value dispatches the load method (all three verified in the
disassembly):

* **flags 0** — direct SPI read `ops[1](src → dst, len)`: plain copy;
* **flags 1** — the hardware-engine path (zero-fill window, stage container
  to 0xA2088000, call the driver at 0xA2001F50 — our boot's path);
* **flags 2** — stage container to 0xA2088000, then call the routine at
  **0xA2002EF8** with (dst, 0, staging, len) — a **software LZMA decoder**.

### 15.4 flags 2 is an LZMA decoder (new firmware capability, verified)

The routine at 0xA2002EF8 parses a 13-byte LZMA header: byte 0 = props
(decoded by the classic div-45 / mod-45 / div-9 loops at 0xA2002FCC into
pb/lp/lc, bounded `< 0xE1`), bytes 5..13 = **u64 LE uncompressed size**
(assembled by the shift-accumulate loop at 0xA2002F68), then calls the
decoder at **0xA200304C** with (state, src+13, len−13, &out, dst, size).
This is a complete second codec in the SDK image and the first
identified software decompressor; it is *not* used by the observed boot
(flags 1) but is fully present in the known-good driver payload.

### 15.5 The engine window-usage scan: no additional modes (§14.4 item 2)

An exhaustive scan of the known-good driver image for accesses to
0xBFFE3800..0xBFFE3880 (s6-relative and lui-based) finds engine traffic
only inside the already-decoded regions: the loader (0xA2001A10..0xA2001C30),
the engine driver (0xA2001F50..0xA2002790), and the bit-6 failure path
(0xA2002ECC/0xA2002EEC). **No additional engine modes are exercised
anywhere in the image** — the unexplained constants (0xBFFE3830 = 0x68,
0xBFFE3840 = 0x20, 0xBFFE3858 = 0xC0, mode 0xBFFE3800 = 0x90) are the
complete set of engine programming inputs, as far as this firmware goes.

### 15.6 The LZ value semantics: five families refuted; structure mapped

All experiments decode the verified symbol stream (25,027 symbols, exact
parse) and score the result with the calibrated strict-validity oracle
(`tools/fw_ring_sweep.py`: stage-1 98.2%, stage-2 97.4%, satellite DB
30.2%, PRNG 35.9% — real code separates cleanly). RFC baseline:
**48.3%**, zeros 24.7%, entropy 6.22 (reproduces §14.2 exactly).

1. **Ring-size sweep (window aliasing)** — `ring[(write−dist) mod W]` for
   W ∈ {512, 1024, 2048, 4096, 8192, 16384, 24576, 32768}, pre-zeroed
   ring, live-window copies: all 47.4–48.3%, no improvement; W=32768
   reproduces the RFC output byte-for-byte (implementation cross-check).
   The driver's 32 KiB zero-fill matches the engine's true window; the
   0x400 chunk register is not a smaller LZ window.
2. **Word-aligned copy source** (DMA fetch granularity): source rounded
   down/up to word or halfword, copy length exact or rounded to
   word/halfword — all 46.8–48.4%. (The output-size delta 4,474 also
   matches no natural length-rounding class: odd-len count is 4,711.)
3. **Snapshot copy** (DMA without overlap handling): only 142 of 9,264
   matches have dist < len — the family cannot move the result.
4. **Position-periodic write transforms** — XOR/ADD/SUB with key
   (x mod P), P ∈ {16..32768}: best 52.5% (xor, P=64) — an artifact-level
   gain, not a solution (`tools/fw_keydetect.py`).
5. **Size-constrained length-base-table search** — under reading (a)
   (exact 78×1024 output), the base-table contribution must rise by
   exactly +4,474 with per-extra-bit-group offsets g0..g5: exactly 64
   solutions exist; **all 64 score 49.1–50.6% strict** (`tools/fw_length_search.py`).
   The length-table family does not contain the answer either.

**Corruption structure** (`tools/fw_corruption_map.py`): strict validity
**decays monotonically** from 67.2% (chunk 0) through ~52% (chunk 10) to
a ~44–50% plateau; it is **not chunk-periodic** at 1 KiB or 128 B
granularity (refuting the "0x400-granularity copy hardware" reading of
§14.4 step 3), and the match-class mix (literal / dist ≤ 1K / dist > 1K
shares) is flat across the image — the decay correlates with no
producing-match class. The plateau (~45%) sits between PRNG (35.9%) and
real code (98%): the decode settles into self-consistent plausible
garbage through its own copy chains.

**Distance statistics** (`tools/fw_dist_stats.py`): distances span
1..32,743 (44.2% of matches exceed 1 KiB) — the compressor used the full
32 KiB window, so the engine must too; there is no "compressor used a
small window" escape hatch.

### 15.7 What remains open

* **Q7 (new)**: how the stage-1/2 loader's copy path reconciles the
  descriptor source fields (0x10000 / 0x135C) with the plaintext payload
  placement at 0x2F6C — disassemble the stage-1/2 copy loop.
* **Q8 (new)**: the checksum algorithm (two anchors, §15.2) — its
  recovery would give an offline validity oracle strictly stronger than
  instruction statistics.
* **§14 engine LZ value semantics** — now narrowed by five refuted
  families (§15.6) plus the §14.3 list, with the corruption structure
  mapped. The remaining hypothesis space is concentrated on per-match
  value behavior that is *not* explainable by any uniform table, window,
  alignment, or write-transform model tested so far.
