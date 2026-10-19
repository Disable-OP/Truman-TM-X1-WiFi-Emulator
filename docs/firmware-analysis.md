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

- Descriptor table (mirror window, file offsets): [0x1214] = **0xA2000000**
  load address, [0x1218] = 0x00010000 source offset, [0x1220] = 0x00003ADC size.
- "copy fw to RAM" + "Go!": the loader decompresses/decrypts the payload and
  copies it to kseg1 0xA2000000 (phys 0x02000000) — **this transformation is
  performed by the firmware's own code running under our interpreter**; no
  dedicated crypto hardware had to be modeled for it to work.
- "1ST BOOT" banner: printed via **unrolled immediate stores** (PCs
  0xA2000340..0xA2000448) — the banner bytes never exist in memory, which is
  why file/DRAM scans cannot find them.
- Steady state (current milestone): the eCos application polls the
  key/status register 0xBFFE384C (bits 0/2/4/5/6) with gp-relative state at
  0x82003B1C — an idle wait for front-panel input. Modeled register reads 0,
  matching a receiver with no key pressed.

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
