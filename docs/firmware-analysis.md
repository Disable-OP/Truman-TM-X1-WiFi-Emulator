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
| 0x98010000 / 0x98000000 | 74+10 | kseg0 → physical 0x18010000 / 0x18000000 — the flash mirror: 0x98000000 serves stage-1/2 **code fetch**, 0x98010000 serves **data reads** (the 0x10000-package descriptors during "LD eCos"; §16.4) | **Resolved** (Q2) |
| 0x82000000 | 55 | kseg0 → physical 0x02000000 (DRAM data area) | **Probable** (DRAM) |
| 0x80000000 | 16 | kseg0 DRAM base | **Probable** (DRAM) |
| 0xA0000000 | 6 | kseg1 alias of DRAM base (uncached) | **Probable** |
| 0xB0000000 / 0xF0000000 / 0x90000000 | 6/6/5 | **Address-classification masks, not peripheral bases** (Milestone M, §16.4): the sites are `and` with 0xF0000000 + `beq` against 0x90000000/0xB0000000/0xF0000000 in the loader's address-attribute dispatcher; no load/store is issued through them | **Resolved** (code decoded) |
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
| Q2 | Which peripherals live at 0x98010000 / 0x98000000 / 0xB0000000 / 0xF0000000? | **Resolved in Milestone M (§16.4)**: 0x98000000/0x98010000 = the flash mirror (code fetch + data reads); 0xB0000000/0x90000000/0xF0000000 = address-classification masks in the loader's dispatcher, never dereferenced. The phys 0x10000000/0x30000000 stubs stay in the access map (never touched by the observed boot). |
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
| 0x1218 | 0x00010000 | never read by the loader (§22.3); semantics still open |
| 0x121C | 0x0000135C | source **offset** from the hardcoded data-area base 0x1C10 (resolved §22.3: 0x1C10 + 0x135C = 0x2F6C) |
| 0x1220 | 0x00003ADC | payload size — matches the plaintext payload |
| 0x1224 | 0x5DB06FB5 | checksum word (**open**, Q8; bytes `b5 6f b0 5d` — the value was previously misread as 0xB56FB05D by byte order; corrected §22.2, runtime verdict §22.4) |
| 0x1228 | 0x0000007F | flags/config (**open**) |

Later entries reference 0xBFFE0000 (the sysreg window) and look like
register-init data rather than descriptors. Two checksum anchors are now
known — once the algorithm is recovered from the loader code, both must
validate:

| coverage | expected checksum |
|---|---|
| driver payload (flash 0x2F6C..0x6948) | `0x5DB06FB5` (desc word @0x1224; bytes `b5 6f b0 5d` — corrected from the byte-swapped reading, §22.2) |
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

## 16. Milestone M — stream structure, checksum, position-shift, and Q2

All experiments are reproducible from the committed tools
(`tools/fw_stream_structure.py`, `tools/fw_ck_sweep.py`,
`tools/fw_shift_predicate.py`, `examples/window_correlation.rs`) against
the pristine flash image.

### 16.1 The SP stream is NOT one deflate block; no standard continuation exists

The RFC walk treats the container as a single dynamic block and finds
symbol 256 (EOB) twice at mid-stream (RFC-out 28,260 and 45,137). New
structural facts (`fw_stream_structure.py`):

* **Block 1 is a well-formed FINAL dynamic block** (bfinal=1, btype=2):
  honored as a block boundary it ends at bit **106,259** with **28,260
  bytes** of output and 9,606 symbols. Strict validity of that prefix:
  52.3% — the same bytes the single-block walk produces up to EOB#1.
* **169,229 stream bits remain** after block 1 — the tail must produce
  the remaining ~51,612 bytes of the programmed 79,872.
* **No standard continuation exists.** Exhaustive per-bit search over
  bits 106,259..107,159 (`find-b2`): every plausible dynamic/fixed block
  header either fails to decode or terminates degenerately (all
  fixed-table candidates converge on endbit 106,953 with 33–42 symbols
  and ≤181 bytes of output — a fixed-decode artifact, not structure).
  The immediate continuation decodes as btype=1 (fixed) and hits invalid
  length symbol 286 after 46 bits; the byte-aligned continuation parses
  a plausible-looking dynamic header (hlit=260, hdist=10 at bit 106,264)
  but its decode runs to the stream end **without ever reaching an EOB**
  — misaligned garbage, not a block.
* A stored-block continuation is refuted: no LEN/NLEN complement pair
  exists at any byte boundary 13,282..13,340.
* **The litlen and dist tables are complete (Kraft sum = 1)** — so the
  walk's "zero bad codes over 25,027 symbols" never protected the tail:
  complete tables decode ANY bit pattern. The single-block parse of
  everything after EOB#1 was never actually verified.

Conclusion: the container's tail carries structure outside standard
DEFLATE block framing — per-chunk framing, a dialect use of symbol 256,
or a second codec section. Until the tail is decoded, the exact-output
size constraint (Σ lengths = RFC + 4,474) should be treated as provisional
(it assumes the tail symbols are all consumed as literals/matches).

### 16.2 Checksum algorithm not in the standard families

The two anchors (§15.2) were swept against ~250 standard checksum
families (`fw_ck_sweep.py`): CRC-32 catalog (10 polys × init 0/0xFFFFFFFF
× refin/refout × xorout 0/0xFFFFFFFF), 14 CRC-16 families, Adler-32
variants, Fletcher-16/32, FNV-1/1a, DJB2/sdbm/JS, Jenkins one-at-a-time,
sums/XORs (8/16/32, word-wise LE/BE), BSD/SysV, 15 rotations of
ROR/ROL-accumulate (+ and ^), and weighted sums — each against the driver
payload (flash 0x2F6C..0x6948 → 0xB56FB05D) and the container
(0x1002C..0x96B8 → 0xB5EBF9FF, under the container-coverage hypothesis).
**No family reproduces either anchor.** The algorithm is custom or the
coverage differs; Q8 stays open, and with it the strongest offline oracle.

### 16.3 Position-shift reframes the length search; exact-count predicates

Every prior length search kept RFC copy accounting — later token
positions never moved. But the first MATCH in the stream produces output
byte 2, so only bytes 0..1 (literals `60 81`) are positionally certain.
If the engine's matches emit more bytes than RFC accounting, **all
subsequent positions shift**, which would explain simultaneously:

* why the decoded head is a "sparse non-code table" instead of the eCos
  vector/entry code (the boot walks NOPs from 0xA1600000 until it faults
  on the literal word `00 00 FF FF` at +0x50 — table bytes, not code);
* the programmed 78×1024 = 79,872 vs the RFC 75,398 (Δ = 4,474);
* why every fixed-positions search plateaued at ~50%.

If the shift comes from a uniform-per-predicate extra emission, the
predicate must satisfy count × adjustment = 4,474 (4,474 = 2 × 2,237;
2,237 is prime). `fw_shift_predicate.py` enumerates threshold,
conjunction, singleton, modulo and chunk predicates over (len, dist,
out). Exact hits at adjustment +2 (2,237 matches each):

| predicate | matches |
|---|---|
| `dist < 96` | 2,237 |
| `dist ≥ 3485` | 2,237 |
| `len ≥ 5 & dist < 3073` | 2,237 |

None is a natural hardware boundary (96/3485/3073 are not ladder edges),
so the predicate family is reported but not promoted; the search's real
conclusion is that **combined length+distance semantics** (not any
uniform table remap, and not any position-fixed model) is the remaining
hypothesis class.

### 16.4 Q2 resolved: window naming with runtime + static evidence

`examples/window_correlation.rs` segments the real boot by UART
milestones (10,000-step segments; per-phase per-window access deltas via
`Machine::access_log`), producing the phase table this section cites.

* **0x98000000 (phys 0x18000000, offsets 0x0000..0xFFFF)** — code fetch:
  stage-1 execution (phase "jump to second boot!!"), stage-2 DDR loader
  (0x0160..), driver execution after "Go!" (0x5D10..). One window, ~10k
  read accesses per 10k-step phase — the execution mirror.
* **0x98010000 (mirror offset 0x10000)** — data: exactly during the
  "LD eCos v1.16 1506f" phase, 44 read accesses land at mirror offsets
  0x10000.. — the **0x10000-package header/descriptor reads**. The
  "0x98010000 window" is not a separate peripheral; it is the flash
  mirror's data path. (Cumulative mirror usage: 49,468 distinct offsets,
  range 0x002C..0x186B7, 34,488 of them ≥ 0x10000.)
* **0xB0000000 / 0x90000000 / 0xF0000000 (phys 0x10000000 / 0x30000000)**
  — **never accessed by the observed boot** (zero accesses in 1.3 M
  steps). Static decoding of every lui site (`lui $r, 0xB000/0x9000/
  0xF000` in stage-1/2 and the driver) shows the constants are used in
  **address-classification chains**: `and` with 0xF0000000, then `beq`
  against 0x90000000 / 0xB0000000 / 0xF0000000 selecting handler
  branches (e.g. stage-2 0x0B410, driver 0x04F1C..0x05168) — masks on
  the *high nibble of arbitrary addresses*, never dereferenced bases.
  The earlier "vendor peripheral spaces" reading (§6) is corrected.

Model updates shipped with this milestone: `MachineConfig` documents the
resolved roles (flash mirror; never-accessed stubs), and the stubs stay
in the instrumented access map so any future firmware that touches them
becomes immediately visible.

### 16.5 What remains open

* **The SP tail structure (§16.1)** — now the primary LZ blocker: decode
  the 169,229 bits after block 1. Next probes: (a) per-chunk framing
  derived from the driver's 128-byte rounding rule; (b) dialect meaning
  of mid-stream symbol 256 (2 occurrences); (c) whether the tail's bit
  positions align to the engine's start-bit + k×1024 grid.
  *(All three resolved-negative in §25: the chunk-resync dialect family
  is refuted; the body gap 1008..1023 is zero padding.)*
* **Q8 checksum** (§16.2) — custom algorithm or different coverage;
  disassemble the loader's checksum routine via an access trace on the
  descriptor reads.
* **§14 LZ value semantics** — refuted families now include: all §14.3 +
  §15.6 lists, standard block framings (§16.1), ~250 checksum families
  (as an oracle hunt), and position-fixed length remaps (§16.3). The
  remaining space: combined length+distance semantics with position
  shift, and whatever the tail structure reveals.

## 17. Milestone N — the engine is a PNG decoder; the output is an image

### 17.1 Identification (external, verifiable)

The SPHE15xx platform register header (`arch/mips/include/asm/mach-sphe15xx/
regfile.h` in the public `Team-Proton/linux-sunplus-15xx` tree; the same SoC
line, CPU `mips,mips24KEc` per the platform DTS) names the entire
0xBFFE3800..0x387C block **PNG_DEC_*** — a hardware PNG decoder. Every
register the firmware programs maps onto a PNG semantic:

| register | kernel name | firmware program |
|---|---|---|
| 0xBFFE3800 | PNG_DEC_MODE | 1, then 0x90 |
| 0xBFFE3804/08 | PNG_BS_STR_ADDR_M/L | bitstream base (the copied container) |
| 0xBFFE380C/10 | PNG_BS_END_ADDR_M/L | round128(base + stream_len + 8) |
| 0xBFFE3814 | PNG_BS_BUF_ADDR | 0 |
| 0xBFFE3818 | PNG_BS_OFFSET | start bit (1088 = 1024 + 64 container bits) |
| 0xBFFE381C | PNG_BS_USED_CNT | engine-internal (completion feedback) |
| 0xBFFE3820 | PNG_BS_USED_BIT | engine-internal (completion feedback) |
| 0xBFFE3824 | PNG_DEC_CTRL | start (bit 0), ready (bit 1) |
| 0xBFFE3828 | PNG_IMG_WIDTH | **0x400 — driver constant (1024)** |
| 0xBFFE382C | PNG_IMG_HEIGHT | container u16@2 = 78 |
| 0xBFFE3830 | PNG_IMG_TYPE | **0x68 — driver constant** |
| 0xBFFE3834/38 | PNG_CHUNK_SIZE_M/L | stream_len + 8 + (len&1) + 6 |
| 0xBFFE383C | PNG_BLK_INFO | (btype<<1)\|bfinal = 5 |
| 0xBFFE3840 | PNG_HUF_TB_ADDR | 0x20 |
| 0xBFFE3844/48 | PNG_HUF_TB_CTRL/DATA | Huffman table upload FIFO |
| 0xBFFE384C | PNG_INTERRUPT | event/ack line (shared with the key scan) |
| 0xBFFE3850/54 | PNG_SCL_EXT_ADDR_M/L | scaling buffer 0x02048000 |
| 0xBFFE3858 | PNG_SCL_BUF_ADDR | 0xC0 |
| 0xBFFE385C/60 | PNG_BW_EXT_ADDR_M/L | band buffer 0x02040000 (32 KiB = 32 rows) |
| 0xBFFE3864 | PNG_REM_CHUNK_SIZE_M | 1 |
| 0xBFFE386C/70 | PNG_PXL_EXT_ADDR_M/L | pixel output 0x01600000 |
| 0xBFFE3874 | PNG_PXL_LINE | row countdown preload (77 = limit−1) |

Re-read from the driver disassembly with the PNG names: PNG_IMG_WIDTH =
0x400 and PNG_IMG_TYPE = 0x68 are **hardcoded constants** (0xA2002134,
0xA200222C); PNG_IMG_HEIGHT = the container's BE u16@2; PNG_CHUNK_SIZE =
stream_len + 8 + (len&1) + 6 (the +6 is a constant fudge added at
0xA2002224); PNG_PXL_LINE is preloaded with height−1 (0xA200214C..58).
The driver has **no software post-processing pass** — after the accounting
block (0xA2002630..0xA20026C0) it gates the clock and returns; the
engine's pixel output is the final image.

### 17.2 What the boot actually does

The "decompression" stage renders a **1024×78 8-bpp image into
0x01600000 and the boot then jumps into the pixel buffer**
(0xA1600000 = kseg1 alias of the PNG_PXL_EXT_ADDR). The eCos binary is
shipped as the *pixel data* of a PNG-engine-compatible stream: the vendor
tool used the boot ROM's PNG decoder as a decompressor. Everything that
looked like a decompression quirk is image mechanics:

* the "79,872 output bytes" = 78 rows × 1024 pixels — an **allocation**,
  not a decode-length claim;
* the per-0x400-byte "events" = **per-row completion** (PNG_PXL_LINE
  counts rows; 78 rows, one event each);
* the "LZ window" at 0x02040000 = the PNG **band buffer** (32 rows ×
  1024 B); the 256 KiB "scratch" = the PNG **scaling buffer**
  (PNG_SCL_EXT_ADDR) — both zero-filled by the driver;
* the completion accounting `(USED_CNT + acc<<16) × 128 + field10`
  counts **completed 128-pixel units**: 624 × 128 = 79,872 px — pure
  row accounting, *not* a decoded-byte count.

**The §14.1/§16.3 size-delta premise dissolves.** The engine completes
every programmed row; when the bitstream ends early (the stream decodes
to 75,398 bytes ≈ 73.6 rows) the remaining rows come from the zero-filled
band buffer, and a decode longer than the image would stop at the image
end. The +4,474 "gap" is row padding — the container legitimately holds
73.6 rows of content, and the tail "structure" question (§16.1) is
moot: the stream is one block consumed to input end, exactly as the
emulator already models.

### 17.3 Refutations shipped with this milestone (new tools)

All against the pristine flash; strict validity = the calibrated
MIPS oracle (§14.2).

* `tools/fw_shift_decode.py` — actually DECODES the §16.3 predicate
  candidates instead of only counting them: `dist<96 +2` 48.5%,
  `dist≥3485 +2` 48.2%, `len≥5&dist<3073 +2` 46.8%, `(dist<96|dist≥3485)
  +1` 48.0% vs the RFC baseline 48.3% — the position-shift family with
  copy-extension semantics is **refuted by decoding** (the three
  equal-count predicates are mutually disjoint, so at most one could
  have been real).
* `tools/fw_field_order.py` — all four match-token field orders
  (RFC; len,dist,extras; len,dist,dist_eb,len_eb; dist-first) decode to
  75.4–75.5 kB at 46.8–47.9% — field-order variation refuted.
* `tools/fw_tail_probe.py` — Kraft-completeness filter (validated: it
  finds the known-good header at bit 0 and nothing else in 0..1100) over
  bits 106,259..110,355 after EOB#1: **no** dynamic header exists there
  (the byte-aligned candidate at 106,264 has litlen Kraft 5.25).
* `tools/fw_png_rows.py` — raw-8-bit filter bytes between 1024-token rows:
  row filters come out random (241, 254, 251, …) and the certain head
  `60 81` is destroyed — the raw-filter geometry is refuted at row 0.
* `tools/fw_png_attack.py` — 682 exact-count (predicate, adjustment)
  pairs for the 79,950 filtered-stream target: **zero** pass the PNG
  filter-byte check (row starts ∈ {0..4}).
* `tools/fw_png_ladder.py` — 135 group-constant length-ladder solutions
  for the 79,950 target: zero pass the filter-byte oracle.
* global distance-offset sweep −16..+16: flat 48.1–49.7%.

The PNG filter-byte structure is also inconsistent with RFC semantics at
row 0 (the first output byte is the binary's own `60 81`, not a filter
value), so the container is **not** a filtered PNG zlib stream: with
PNG_IMG_TYPE = 0x68 the engine appears to consume raw (filter-less)
image rows.

### 17.4 What this reframes — and what stays open

* The decode target is now **the pixel image**: 75,398 bytes ≈ 73.6 rows
  of real content + padding. The strict-validity oracle must be applied
  to the image (the eCos binary), which is what the existing tools do —
  the 67%→45% decay profile (§16 profile) is a *content* defect, not a
  framing defect.
* The remaining blocker is unchanged in kind but narrowed in scope: the
  engine's LZ **value** semantics (which bytes a match copies) for a
  stream whose parse, tables, start bit and row geometry are all
  verified. §14.3/§15.6/§17.3 refutations now cover: table remaps
  (uniform, per-group exact-size), distance remaps (±k, rotations,
  global offsets ±16), ring sizes/indexing, presets, byte transforms,
  extra-bit polarity, start bits, field orders, block framings,
  position-shift predicates, and PNG filter geometries.
* Sharpest open hypotheses: (a) the band buffer's write granularity vs
  the compressor's reference model (a tool that computed distances
  against a byte-wise window while the engine commits rows would produce
  exactly the observed near-miss copies — but it must be reconciled with
  the real device booting correctly); (b) the compressor's distance
  domain (band/stream vs pixel); (c) Q8 (checksum) still open.

### 17.5 Milestone O — the ladder hunt (session record)

All experiments against the pristine flash; strict validity = the
calibrated oracle; tools committed.

* `tools/fw_pixel_lz.py` — fixed-filter pixel-domain LZ (literal
  unfiltered with a fixed PNG filter, matches copying pixels): None/Sub/
  Up/Average/Paeth score 49.7/41.2/42.6/38.8/40.6% — refuted.
* Row-wise fixed-filter unfiltering of the RFC decode (no filter bytes;
  the filter from IMG_TYPE): None 48.3%, Sub 36.5%, Up 36.7%, Average
  35.2%, Paeth 36.2% — refuted in this direction too.
* `tools/fw_beam_lz.py` — beam search over per-match length adjustments
  (delta in {0,1,2}, exact incremental word-validity scoring): reaches
  **73.8%** strict (vs 48.3% baseline) but the winning delta sequence has
  NO structure by len, dist, alignment, column or match index — it is
  oracle exploitation, not a semantics. Two facts survive: (a) the
  validity oracle is satisfiable far above the baseline by per-match
  choices, (b) the extra-byte rate stays ~1.08/match throughout.
* Uniform +1/+2/+3 per match WITH image truncation at 79,872 (the PNG
  framing invalidates the old full-output scoring of size-overflowing
  ladders): 50.3/50.4/51.1% — still refuted, but the truncated head
  region improves monotonically (56.6→60.2% over the first 16 KiB).
* `examples/pixel_precontent.rs` — the emulator's DRAM at the pixel
  buffer (phys 0x01600000) is zero before the decode, so the dist>output
  matches copy zeros in any emulator-side replay regardless of window
  placement; the real device's pre-decode content there is unknown.
* `tools/fw_fixed_ladder.py` — **structural result**: walks that do NOT
  consume the distance extra bits (a fixed 30-entry distance ladder, the
  natural simple-hardware design) consume the stream in a way that
  FILLS the 79,872-byte image exactly (m2: image_full with 26,549
  symbols; m4 — no extra bits at all — likewise). Every model that
  consumes dist extras under-shoots (75.4 kB). The ladder mini-grid
  (DBASE / dsym+1 / +1 / +2 / halved, len ±1) stays at 46.6–49.6% — the
  ladder VALUES are silicon ROM, not RFC's.
* Distance-symbol histogram (for the ladder hunt): symbols 12/14/16/18/
  20/21/22/26 dominate (5–6% each) at RFC bases 65/129/257/513/1025/
  1537/2049/8193 — power-of-2-ish distances, exactly the repetition
  structure of real binaries (jump tables, row pitches). This supports
  an RFC-like ladder ordering, leaving the length side as the likelier
  defect.
* Row-boundary predicates: 49 matches cross a 1024-B row, 6 end on one,
  9 start on one — none yields the 2,237 count, and no boundary-crossing
  rule explains the delta.

Next probes (Milestone P): (1) treat the length ladder as 29 unknown ROM
values and search it under the m2 (fixed-dist) bit accounting, with the
image-truncated validity oracle and the 73.8% beam bound as the sanity
ceiling; (2) extract the real device's pre-decode DRAM content at
0x01600000 from hardware (or bound it: DDR-training patterns vs loader
staging) to resolve the dist>output copy semantics; (3) Q8 checksum
remains the decisive missing oracle.

## 18. Milestone P — the size anchor and the ladder verdict (session record)

### 18.1 The size anchor (corrects the Milestone O "exact fill" reading)

Under the m2 bit accounting (length extra bits consumed per the RFC
counts, distance extra bits not consumed — §17.5), the symbol path is
**value-independent**: the Huffman parse of the fixed bits yields the
same symbol sequence for every length-ladder candidate. The full-parse
decode length is therefore a LINEAR function of the ladder,

```
S(L) = n_lit + extra_sum + sum_s(count[s] * L[s])
     = 21,054 + 1,873 + sum_s(count[s] * L[s])            (m2 parse)
```

with per-symbol match counts [4175, 4451, 1111, 532, 495, 506, 123, 130,
256, 115, 133, 68, 127, 148, 61, 26, 76, 55, 23, 21, 21, 4, 2, 7, 2, 5,
4, 1, 12] (12,690 matches + 21,054 literals = 33,744 symbols; the parse
consumes 275,488 bits = 100.0% of the stream, 8.16 bits/symbol — a
compressor-like density).

With the RFC ladder this evaluates to **S = 103,014 bytes — 23,142 bytes
beyond the 79,872-byte image**. Milestone O's "m2 fills the image
exactly" was the walk's image CAP firing (out crossed 79,872 with ~22%
of the stream still unread), not an input-end property. Since the vendor
tool encoded the image and the parse consumes every bit to input end,
the engine's TRUE ladder must bring S(L) to the image size. This anchor
has teeth: it prunes the entire degenerate-exploitation region that the
§17.5 beam search fell into (high scores by racing through the stream
with huge lengths, leaving most of it unread).

### 18.2 The ladder search and its verdict (`examples/ladder_search.rs`)

The search treats the 29 length-ladder values as unknown ROM constants
under the m2 accounting, with the image-truncated strict validity oracle
(§14.2) — re-based here on the crate's own MIPS32r2 decoder
(`cpu::mips32::describe` tokens, calibrated on flash[0..0x10000), 49
common mnemonics) so the search runs at Rust speed:

* **Phase 0** — 477 in-band seed ladders (greedy count-weighted
  decrements from RFC into the size band S ∈ [75,000, 80,130], then
  band-preserving jitter).
* **Phase 1** — every seed scores 56.8–57.1% (my oracle): a tight
  cluster at the baseline, no outlier structure.
* **Phase 2** — band-preserving coordinate descent (single + pair moves
  over the high-count symbols): best 58.51%, ladder
  `[1, 1, 4, 6, 7, 7, 9, 10, 11, 1, 11, 17, 19, 23, 27, 31, 35, 43, 51,
  59, 67, 83, 105, 115, 131, 163, 194, 227, 256]` — non-monotone,
  unstructured (L[0]=L[1]=L[9]=1 tweaks), unstable across seeds.
* **Phase 3** — monotone projection of the winner: 55.70% ≈ baseline; a
  ROM-like ordered ladder buys nothing.

**Capstone cross-check** (`tools/fw_ladder_verify.py`, the published
§14.2 oracle): RFC m2 baseline 47.0%, search winner 49.5%, monotone
projection 47.3%. The +2.5-point lift of the winner is the same
magnitude the beam search's per-match deltas bought by oracle
exploitation (§17.5) — and unlike a semantics, it shows no structure.

(The two oracles differ in absolute percentage — different decoder and
calibration — so numbers are compared within each oracle only; every
conclusion rests on the capstone cross-check.)

**Verdict: the m2 length-ladder family (with the distance ladder at its
RFC values) is REFUTED.** No size-feasible ladder scores materially
above the baseline; the only ladders that score high (83.3% in the
unconstrained pre-run of this tool) violate the size anchor by
construction — their walks hit the image cap after only 4,633 of the
33,744 parsed symbols, leaving ~80% of the stream unconsumed (a vendor
tool does not encode 4x the image it ships). Together with the distance
ladder mini-grid and field-order refutations (§17.5, §17.3), the
"custom silicon ROM ladder" hypothesis now has both of its halves
bounded: neither a re-valued distance ladder nor a re-valued length
ladder (nor both at RFC shape) produces a structured validity jump under
a size-feasible accounting.

### 18.3 Pre-decode DRAM below the pixel base: bounded (`examples/dram_prebound.rs`)

§17.5 item 2 asked for the real device's pre-decode DRAM content below
the pixel buffer (phys 0x01600000), which dist>output copies would read
if the engine's LZ window were the output stream itself. Emulator-side
bound, both passes on the real firmware through the full boot (1,301,783
steps to the known post-jump stop at 0xA1600050):

* **Canary pass** (`AddressSpace::poke_phys`, new diagnostics API — the
  CPU-facing store still rejects ROM, so this cannot smuggle writes into
  running code): seeding 0x01500000..0x01600000 with 0xA5A5A5A5 and
  booting changes **nothing** — 0 boot-written bytes in 0 ranges across
  the whole 1 MiB.
* **Fresh pass**: 0 non-zero bytes — the region is never touched.
* Determinism check: trivially clean (nothing written, nothing read);
  the boot never branches on the region's prior content.

Consequences:

1. The loader stages NOTHING below the pixel base; the only software
   writes near the decode are the driver's own zero-fills of the band
   buffer (0x02040000) and scaling buffer (§17.2).
2. On the real device, below-base content is therefore determined solely
   by DRAM initialization / DDR-training residue — not observable from
   software, and deterministic across boots only if the training's last
   write pattern there is fixed (not provable from this dump).
3. **Device determinism + (1) imply the engine's dist>output source
   cannot be raw below-base DRAM**: the boot jumps into the decoded
   image and must do so identically on every power cycle, so any
   semantic reading non-deterministic residue would make the boot
   non-deterministic. The zero-filled band-buffer window (the emulator's
   model) remains the only software-visible deterministic choice.
4. Impact bound under the RFC parse (m1, `tools/fw_engine_lz.py`
   semantics): exactly **458 matches** reach beyond the output start,
   copying **3,001 bytes = 3.98%** of the 75,398-byte block-1 content
   from the zero preset. Even if the window semantics were wrong in
   exactly this dimension, at most 4% of the output differs — the 48%
   validity baseline cannot be explained by window placement alone.

### 18.4 What stays open

* The engine's LZ value semantics: with length-ladder (this milestone),
  distance-ladder (§17.5), field-order, framing, preset, shift and
  filter families all refuted under size-feasible accountings, the
  remaining hypothesis space is the ACCOUNTING itself — how the engine
  consumes bits per symbol (e.g. a non-RFC extra-bit scheme, or the m4
  all-fixed accounting with its own size anchor solved for BOTH ladders
  jointly — 59 unknowns against the same linear anchor, feasible with
  the §18.2 machinery).
* Q8 (the desc[6] checksum) remains the decisive missing oracle (§14).
* DDR-training residue below the pixel base: bounded, not extracted
  (§18.3 item 2) — needs hardware, not more software.

## 19. Milestone Q — the executable-head oracle: every tested accounting
### produces a non-executable eCos entry (session record)

### 19.1 The head paradox, stated

The boot chain provably jumps to the pixel-buffer base with the driver's
return value live: the loader's `jr v0` targets `0xA1600000` exactly
(emulator trace of the real firmware, register dump: `v0=0xa1600000`,
`v1=0x00013800` — the driver's success return `0x400 × 78`), so LZ output
byte 0 IS the first executed instruction word. Under the m1..m4 accountings
(§17.5) the decoded head is:

```
0000: 00008160  add $s0,$0,$0     (literals 60 81 + match zeros — hard
0004: 00000000  nop                in EVERY accounting: bytes 0-1 are the
...                                 first two litlen codes under any
0014: 02140000  sll (m1/m3)        convention, and bytes 2-3 can only be
0018: 01000000  beq +0 (m2/m4)     {00,60,81} — the window holds only
...                                 the preset zeros and the 60 81 pair)
0050: 0000ffff  SPECIAL funct 0x3F — RESERVED (m1/m3)
```

A reserved-instruction fault at +0x50 cannot happen on a booting device:
**the true decode's head differs from every model we test.** This is a new
oracle, stronger than the validity percentage: the eCos entry must be
executable MIPS from word 0 (harmless ops allowed, but the first
reserved/trap word must never be reached before a branch).

### 19.2 What the oracle refuted (all tools committed)

* `tools/fw_first_corruption.py` — per-byte emit attribution (literals are
  ground truth under a given accounting; matches can be wrong), a MIPS32r2
  word classifier (executable-head scanner), and the `--survey` mode. It
  pins WHERE each accounting's head dies and on WHAT:
  * m1/m3: word +0x50 = `0x0000ffff` — built from **literals `ff ff`**
    (ground truth under m1) — fatal.
  * m2: word +0x18 = `0x6200003b` (opcode 0x18, reserved) — **literal
    bytes 3b/62** — fatal.
  * m4: word +0x6c = `0xa5000000` (SH to kuseg with $s4=0 → TLB fault)
    then +0x74 `0x00000097` (funct 0x17, reserved) — **literal bytes
    a5/97** — fatal.
* `tools/fw_head_sweep.py` — the UNIFORM extra-width family (len/dist
  extra fields read at fixed widths 0..8 and 'r', masked per symbol; the
  natural simple-hardware read): 100 combos — **every one reserved-faults
  at +0x4..+0x50**. Uniform widths are refuted by the head.
* Start-bit variants (stream-relative 1008/1072/1088/1152/1216 vs
  container 1088−64=1024) × {m1,m2,m4}: all reserved-fault within +0x100.
* Table-RAM convention variants (§13.6's first[]/cum[] indexing):
  * the canonical reading (cum exclusive, first canon) reproduces the
    m1..m4 heads exactly (validates the harness);
  * first[] interval shift (+1 length): dies at the FIRST match
    (bad distance at bit 1035) — refuted;
  * cum[] used inclusively (symbol index off by count[L]): the parse dies
    within ~100 bytes (bad distance) — refuted.
* The vendor's entry signature (`addiu $zero,$zero,0x6fb5`, present in
  both the stage-1 loader and the driver payload) appears NOWHERE in any
  decode → the eCos stage is a different toolchain (expected) and the
  entry is not hidden at an offset inside the buffer.
* Output-transform spot checks: word byte-swap → opcode 0x18 at word 0
  (dead); halfword swap / bit-reverse → no branch structure possible
  (dead); 16bpp/RGBA/1bpp pixel-format expansions contradict the
  completion accounting (624 × 128 = 79,872 = bytes, §13.6) — dead.

### 19.3 The structural conclusion (narrows the remaining space)

In every tested accounting the head dies on **literal bytes** (fixed by
the parse), not on match bytes. Two consequences:

1. **Length/distance ladder values cannot fix the head.** The m2/m4
   ladder-solve programs (§18.4) are therefore NOT the next step: no
   ladder choice changes a literal. (They remain relevant for the image
   BODY, but they cannot resolve the entry.)
2. The remaining hypothesis space is the **symbol→value mapping or an
   output transform**: which value each litlen code produces (the engine's
   syms[] table-RAM interpretation), or a transformation between the LZ
   output and the pixel buffer that we have not modeled. Both must keep
   the verified parse (the code tables are canonical; the driver's upload
   convention analysis §13.6 holds) while changing what the decoded
   symbols MEAN.

Suggestive (not evidence): m2/m4's head contains `beq $0,$0,+0` at word 1
and — under m4 — `blez $v1` at word +0x18, where `v1 = 0x13800` is exactly
the driver's return value at the jump (register dump). A vendor eCos entry
that validates the loader's return code is plausible; the pattern dies on
later literals under every tested mapping.

### 19.4 The head-branch DFS (`tools/fw_head_dfs.py`) — run, and what it showed

DFS over per-match distance assignments for the head (literals fixed,
match spans parse-fixed, live-window copy), pruning at the first
reserved/trap word and stopping at a non-trivial branch/jump ("escape"):

* m1: an "escape" at word +0x2c = `00008f48` (JR $a3) — but via d=1
  self-copy floods (the head becomes `60 81 81 81 …`) and $a3 = 0 at
  entry: a jump to 0 = kuseg TLB fault. **Spurious.**
* m2: 400k DFS nodes exhausted, **no escape at all** — the m2 head cannot
  be made executable by distance choices within that budget.
* m4: an "escape" at word +0x18 = `18623b3b` (BLEZ $v1, +0x3b3b) — again
  a d=1 self-copy flood feeding the offset. **Spurious as semantics.**

Conclusion: the raw head-branch criterion is satisfiable only by
degenerate self-copy assignments, which no compressor would emit, and m2
— the most structured head — admits NO escape. A distance-ladder
transform therefore cannot rescue the head for m2 and produces only
garbage for m1/m4: the transform is NOT ladder-shaped. Per §19.3 the
search moves to the symbol-mapping family (per-length symbol
permutations compatible with the uploaded syms[]/first[]/cum[] arrays),
or to the decisive instrument: an entry harness that boots the emulator
from 0xA1600000 with the loader's register state and counts executed
instructions per candidate decode — the register-aware oracle the DFS
lacks.

## 20. Milestone R — the entry harness: the register-aware dynamic oracle
### (session record)

Milestone Q ended by naming two instruments: the symbol-mapping family and
"an entry harness that boots the emulator from 0xA1600000 with the loader's
register state and counts executed instructions per candidate decode — the
register-aware oracle the DFS lacks" (§19.4). Milestone R builds that
harness, hardens it against its own failure mode, and runs the first
candidate family through it.

### 20.1 The harness (Rust, tested)

* `Machine::run_until_pc(max_steps, watch)` — the boot run loop stops
  *before executing* the instruction at `watch`, returning the exact
  post-jump machine state (`StopReason::EntryReached`).
* `CpuBackend::restore(snapshot)` + `Machine::restore_cpu_state` — full
  GPR/hi/lo/PC restore (never in a delay slot after restore).
* `AddressSpace::copy_out`/`copy_in` — bulk diagnostic snapshot/restore of
  RAM-backed regions (the full 64 MiB DRAM is snapshotted once after boot
  and restored before every candidate, so evaluations are bit-independent).
* `examples/entry_oracle.rs` — boots the real firmware with the watch at
  0xA1600000, captures the entry state, then per candidate: restore DRAM +
  registers, zero-pad the candidate to the programmed image size (the
  §17.2 band-buffer padding semantics), write it into the PNG pixel buffer
  (phys 0x01600000), and execute from 0xA1600000 with a bounded budget.
* Covered by `tests/entry.rs` (5 tests): the entry register state (v0 =
  0xA1600000, v1 = 0x13800, §19.1's register dump), the baseline faulting
  dynamically at the static oracle's predicted word, full state restoration
  between candidates (store-then-fault candidate, bit-identical replays),
  and the step-limit path (self-loop candidate runs clean to budget).

### 20.2 The litlen table structure the harness must respect

`tools/fw_syms_probe.py` pins the uploaded table's shape (the driver builds
a canonical counting sort, §13.6, and uploads BYTE entries — 285 entries
fit the 0xC000..0xC180 table-RAM window, words would not):

* all 286 litlen symbols of this stream are coded (zero-length: none);
* **canonical rank(s) != s**: the two most frequent symbols are 1 and 2
  (ranks 0/1), symbol 0 sits at rank 2, and the length symbols 276/285
  occupy ranks 256/257 — as bytes `(s - 256)` (20/29), with literals
  continuing at ranks 258+ (93, 94, 121, ...).

Consequences: a byte-valued table cannot represent symbols >= 256, so any
engine that classifies tokens from the table ALONE must use the rank
boundary — which misclassifies 60+ symbols here (literals at ranks >= 258
would be length selectors, length symbols at ranks < 256 would be
literals). Either the engine has class side-information we have not
modeled, or the class assignment is rank-driven and the parse's token
STREAM (not its bit positions) differs from RFC — both stay open.

### 20.3 The first family: class-preserving literal-value maps — REFUTED

`tools/fw_syms_family.py` generates candidates whose bit-level parse is
bit-identical to the verified RFC m1 walk (same ranks, same classes, same
lengths/distances/extra-bit consumption) and changes ONLY the byte each
literal rank emits (`render(events, f)`; matches copy the live window, the
band is the zero preset, output is zero-padded to 79,872 = 1024 x 78):

* V1 (class-preserving, one shared parse): identity; `xor k` / `add k` for
  k = 1..255 (table-RAM scramble sweeps, 510 candidates); bitrev8;
  nibble-swap; `lut-by-value` (the engine misreads the rank-ordered table
  as a symbol-value-indexed LUT); `rank-shift +-1/+-2` (table-RAM base
  misalignment — literal byte = table[rank(s) +- d]).
* V2 (structure representatives, dedicated walks): per-length-group symbol
  reversal (§19.4's permutation family name-sake — permutes literal ranks
  with length ranks, so the token classes change); the m2/m4 accountings
  (§17.5) re-walked for dynamic confirmation of §19.2.

Self-check: two independent walkers (event-record render vs rank-walk)
produce byte-identical identity outputs, and the static head oracle
(`fw_first_corruption.word_class`, §19.2's classifier) reproduces the
recorded m1/m2/m4 verdicts exactly (m1 FATAL +0x50 `0000ffff`; m2 FATAL
+0x18 `6200003b`; m4 branch at +0x18).

**Dynamic verdict (522 candidates, budget 65,536 steps each): 519 fault in
the head; the 3 "survivors" (`add +6`, `add +10`, `xor 0x0A`) are NOP-sled
artifacts, not eCos survival** — they branch once into the zero-filled
DRAM below the pixel buffer and then execute 0x00000000 words to the
budget (65,534/65,536 fetched words zero, 65,536 distinct linear PCs, zero
UART bytes). The harness now measures every budget-survivor for exactly
this: distinct-PC count, zero-word fetch share, and UART emission — a real
eCos entry executes diverse code and talks; a sled does neither.

The refutation record (dynamic, loader register state live):

| family | survived | first fault |
|---|---|---|
| identity (= engine model, m1) | 20 steps | +0x50 `0000ffff` reserved (§19.2 confirmed) |
| xor k (255) | <= 32 steps | head reserved/unimplemented words (deepest: `xor70` +0x74); survivor `xor0a` = sled |
| add k (255) | <= 21 steps | head reserved words (incl. one trap at +0x50); survivors `add06/add0a` = sleds |
| bitrev8 / nibswap | 5 / 20 steps | +0x14 COP0-rs unsupported / +0x50 reserved |
| lut-by-value | 0 steps | word 0 `00001154` reserved |
| rank-shift +-1/+-2 | 0..24 steps | word 0 / +0x34 reserved; +1 variant branches to +0xb058 then load-faults |
| per-length reversal | 2 steps | +0x08 `0000007c` reserved |
| m2 accounting | 6 steps | +0x18 `6200003b` reserved (§19.2 confirmed) |
| m4 accounting | 14 steps | +0x38 `c5380018` (COP1-class, unimplemented) |

**Dynamic correction of §19.2/§19.4 for m4**: the static straight-line scan
predicted death at +0x6c/+0x74; the register-aware oracle shows the head's
`blez $v1` at +0x18 FALLS THROUGH (v1 = 0x13800 > 0, the driver's live
return value), and the head dies at +0x38 instead. The static word-scan
cannot follow control flow; the harness can — this is the instrument's
point.

### 20.4 What this forces

* The value-side family with parse-preserving literal maps is dead in its
  entirety: no byte bijection of the "literal symbol = byte value" reading
  produces an executable head. Combined with §19.3 (ladder values cannot
  fix the head) and §19.4 (the transform is not ladder-shaped), the
  surviving hypotheses for the head paradox are now sharply:
  1. the class assignment is rank-driven (§20.2) and the token STREAM
     differs from RFC — i.e. the engine's litlen table semantics encode
     the literal/length split differently (the driver's byte upload cannot
     carry it; a rank-boundary rule with an engine-side length-ladder is
     the natural hardware reading);
  2. an unmodeled output transform between the LZ output and the pixel
     buffer (§14.4 item 2, still not excluded by any §19/§20 test);
  3. the container plaintext is pre-transformed by the SDK tool (the
     engine decodes correctly and the boot executes the transformed image
     — §14.4 item 2's link-time byte-lane permutation reading).
* The harness is now the project's decisive instrument: any future
  candidate decode (new accounting, new class rule, new transform inverse)
  is one file + one run away from a register-aware verdict with
  sled-discrimination built in.

Next (Milestone S candidates): (1) implement the rank-boundary class rule
as an engine-semantics variant (rank < 256 = literal byte T[rank]; rank =
256 = EOB; rank > 256 = length selector T[rank] into a hardware length
ladder) and run the family through the harness — the m4/m2-style static
refutations do not transfer because the token stream changes; (2) the
unmodeled-transform family (byte-lane permutations of the OUTPUT, then
word-domain re-examination); (3) Q8 checksum as the decisive missing
oracle.

## 21. Milestone S — the rank-boundary class rule and the output-transform
### family: both refuted by the register-aware oracle (session record)

### 21.1 The upload inventory pins the class-rule input space

§13.6's upload log is complete evidence of what the engine ever receives
through the 0xBFFE3844/0xBFFE3848 table FIFO:

* round A: 0xC000 + 285 litlen symbol BYTES in code order, 0xC180 + 15
  first-code halfwords, 0xC190 + 15 cumulative halfwords;
* round B: 0xC000 (again), 0xC1C0 + 28 distance symbols, 0xC1A0 + first,
  0xC1B0 + cumulative; each round ends with a 0x0000 terminator.

No per-symbol class metadata is uploaded anywhere. The class of a decoded
litlen token is therefore a function of the canonical rank r, the uploaded
byte T[r], the first[]/cum[] code machinery and the engine's own ladder
ROM — nothing else. The natural single-boundary readings of that space:

* **value boundary** (RFC: s<256 literal / 256 EOB / s>256 length) —
  refuted in §20.3 (522 candidates, dynamic);
* **rank boundary** (r<256 / 256 / 257+; the 286 coded symbols give
  exactly 29 match ranks 257..285, indexing a 29-entry hardware ladder —
  the natural simple-hardware reading) — this section;
* **T-byte boundary** (T=0 EOB / 1..29 length / ≥30 literal; the length
  symbols upload as s−256 ∈ 1..29 and symbol 256 uploads as 0x00) —
  this section.

External check (2026-10): no public PNG_DEC driver exists beyond the
platform register header already used in §17.1 (Team-Proton/
linux-sunplus-15xx contains no PNG_DEC code); the engine's LZ semantics
are undocumented publicly, so the refutation work below is the evidence.

### 21.2 The rank-boundary rule — refuted at word 0, ladder-independently

Tool: `tools/fw_rank_transform.py` — a parameterised walker (class rule ×
length ladder × extra-bit accounting × start bit) with three self-checks
that gate every verdict: the sym-rule m1/m2/m4 walks reproduce §19.2/§20.3
's static verdicts byte-for-byte (FATAL +0x50 `0000ffff` / FATAL +0x18
`6200003b` / branch +0x18), and the m1 identity output byte-matches
`fw_syms_family.render` (independent renderer).

Under the rank rule the head is `60 81 03 43 1a 6c 02 1c` — for the first
time the head carries real byte diversity: tokens 3–4 are length symbols
demoted to literals by the rule, emitting T = 03/43, and word 0
(0x43038160) is NOT statically fatal (the static scanner's first
branch-classified word is +0x4). **The head is ladder-independent**: no
match occurs before word 0's bytes are emitted, so all ten ladder ×
accounting variants (RFC/u0/u1/u3/u4/u8 × m1/m2, incl. m3) produce the
identical head — one dynamic verdict covers the entire ladder space.

Dynamic verdict: **dies at step 0** — `0x43038160` = COP0 rs=0x18 (BC0
class), which the MIPS 24KEc raises as a reserved-instruction exception
too; this is hardware-faithful, not an emulator artifact. The static
classifier's COP0 class is coarser than the CPU's — the prefilter is
advisory, the register-aware oracle is decisive. Secondary evidence: the
rank walks end `eof@275488` with only 45.6–52.9 kB emitted — none fills
the 79,872-byte image the completion accounting programs.

### 21.3 Start-bit variants under the rank rule — refuted dynamically

§19.2's start-bit sweep (1008..1216) was value-class only and does not
transfer — the rank rule changes which tokens are literals. Swept 1008/
1072/1088/1152/1216 × m1/m2 (`fw_rank_transform`): 1008/1072 die
statically (+0xc/+0x0); 1088's head starts with `J` (b2 02 02 0a) and the
oracle kills it at step 2 — fetch fault at 0xA8080AC8, unmapped; 1152's
head starts with `JAL 0xA0000C08` and dies at step 1 — the delay slot
word 0x00003001 is an unknown instruction. All refuted.

### 21.4 The T-byte boundary — statically refuted

T=0 → EOB, 1..29 → length selector (RFC ladder indexed T−1), ≥30 →
literal: the m1/m2/u3 variants all fill the image (79,936 ≥ cap; the
T=0/NUL collision turns 575–651 tokens into EOB markers) but the heads
die statically at +0x74/+0x14/+0x74, before any branch — never executed.

### 21.5 The output-transform family — refuted, zero survivors

Byte-lane permutations of the OUTPUT over both the m1 and m2 bases (96
candidates total with §21.2–§21.4): all 23 non-identity period-4 lane
permutations, the 7 period-8 byte rotations, swap16, bitrev8, nibswap,
the 1024-byte row mirror, and two image-shaped transposes (78×1024 and
1024×78 column-major writes — a strided/planar output order is NOT
excluded by the hardware evidence, since the byte order of the pixel
buffer is model-implied; only the byte count is pinned by the completion
accounting).

The static prefilter passes 24 of them; the register-aware oracle kills
every one within ≤ 14 steps (word-0 unknowns `0x00006081`/`0x00600081`,
alignment/write faults at +0x30/+0x34, the transp78 trap at +0x30,
bitrev8's COP0-unsupported word at +0x14). **Zero budget-survivors** —
unlike §20.3's three NOP-sled artifacts. The transform family is dead in
its entirety for periods 2..8 and for both image-shaped transposes.

### 21.6 Where this leaves the head paradox

The class rule is f(rank, T, first, cum, ladder-ROM); every single-
boundary f is now dynamically refuted (value §20.3, rank §21.2, T-byte
§21.4), across the m1..m4 accountings, RFC/uniform ladders, and the
start-bit variants (§19.2 value-class, §21.3 rank-class). Boolean
combinations of the boundaries (e.g. literal iff r<256 AND T≥30) have no
natural reading — a selector byte ≥ 30 cannot index a 29-entry ladder —
and are not pursued. The surviving hypothesis space:

1. **Q8 checksum** — the decisive missing oracle: a verified
   container/image checksum discriminates decodes without booting;
   §16.2's ~250 standard families missed; the next escalation is the
   loader's own checksum code path (if any) and per-descriptor
   accumulation schemes.
2. **Hardware-side residue** — the real device's pre-decode DRAM at
   0x01600000 (bounded but unknown, §18.3) and DDR training residue;
   needs hardware.
3. **Two-tier class rules with side information we have not found** — no
   upload evidence exists for any (§21.1); pursuit requires new evidence,
   not new search.

The head paradox is now *evidence-bounded*: within everything the
uploads and the runtime access log prove about the engine, no natural
decoding produces an executable entry. Milestone T candidates: Q8
escalation, or freeze the paradox and return to device coverage
(front-panel keycode labels, timer/interrupt completion) pending
hardware access.

## 22. Milestone T — the Q8 runtime verdict: the loader has no checksum
### code path; one anchor corrected (session record)

### 22.1 The instrument: physical-range watchpoints

§21.6 left Q8's escalation as the top remaining item: "the loader's own
checksum code path (if any) and per-descriptor accumulation schemes". The
emulator is the ideal instrument for exactly that question, so Milestone T
first built the missing capability — physical-range watchpoints — and
then observed a full real boot through them.

`AddressSpace::add_watch(base, len, kinds)` records every successful
CPU-issued load/store/fetch inside a physical range with the issuing
instruction's (step, PC), the virtual and physical address, the width and
the value (`WatchHit`); the machine run loops stamp the per-step context
so attribution is exact. Recording is inert while no watch is installed,
hits are capped (overflow flagged, never truncated silently), and
`Machine::run`/`run_until_pc`/`run_traced` all stamp. Covered by 9
mechanics tests (`tests/watch.rs`) — value/kind/width/region attribution,
half-open ranges, kind filtering, cap/overflow, drain, clear, the failed
ROM store recording nothing.

The consumer is `examples/q8_watch.rs`: it boots the real firmware to the
eCos entry with READ watches over the stage-1 descriptor words (file
0x1210..0x1280), the 0x10000-package descriptor entry (0x10010..0x1002C),
the driver payload (0x2F6C..0x6948) and the eCos container
(0x1002C..0x96B8) — installed separately on BOTH flash windows (XIP
0x1FC00000 and the Q2 linear mirror 0x18000000, so the trace also shows
which window the loader uses) — plus a READ_WRITE watch on the payload's
DRAM destination (phys 0x02000000..0x02003ADC). A second phase continues
past the entry with a data-read watch on the pixel buffer. 5 integration
tests (`tests/q8.rs`) pin every runtime fact below.

### 22.2 The anchor correction: desc word 0x1224 is 0x5DB06FB5

Bytes at file 0x1224 are `b5 6f b0 5d`, i.e. the little-endian word is
**0x5DB06FB5**. §15.2 recorded the anchor as `0xB56FB05D` — a byte-order
error (the byte sequence read as if big-endian). The eCos anchor is
unaffected: bytes `ff f9 eb b5` at 0x10028 are LE `0xB5EBF9FF` as
documented, and the runtime trace (§22.3, last four desc bytes) confirms
it. `tools/fw_ck_sweep.py` now targets 0x5DB06FB5; its negative result
stands regardless, because it always compared against both byte orders
(`targets1 = {P1_EXP, bswap(P1_EXP)}`), and re-running it with the
corrected constant reconfirms: **no standard family reproduces either
anchor** (§16.2 stands with the corrected value).

### 22.3 What the boot actually does — instruction-level findings

**The stage-1 loader reads exactly three descriptor words** (steps
122447/122453/122456 of the observed boot, all through the mirror window
0x98000000; the XIP window is never read — Q2's usage split confirmed
again):

| word | value | reader PC | role |
|---|---|---|---|
| 0x121C | 0x135C | 0x9800aa7c | source **offset** |
| 0x1214 | 0xA2000000 | 0x9800aa94 | destination |
| 0x1220 | 0x3ADC | 0x9800aaa0 | length |

The disassembly resolves the open "source field" reading of §15.2 in the
same stroke: the routine computes the source as a **hardcoded base plus
the descriptor offset** —

```
9800aa74: lui    $v1, 0xb800
9800aa78: ori    $v0, $v1, 0x121c
9800aa7c: lw     $a2, ($v0)          # a2 = 0x135C   (source offset)
9800aa84: ori    $a1, $v1, 0x1c10    # a1 = 0xB8001C10 (data-area base)
9800aa98: addu   $s3, $a2, $a1       # source = 0xB8002F6C
```

so the driver payload lives at data-area base 0x1C10 + offset 0x135C =
file 0x2F6C, exactly where §15.1 found it. Word 0x1218 (0x00010000) is
never read; it is not part of the source computation.

**The copy is pure data movement.** The executed path (the `jal
0x9800cd60` check returning non-zero) is `lwl/lwr` from the mirror + `sw`
to 0xA2000000 — one pass, no accumulator, no second read of any byte
(the two reader PCs are the unaligned-load pair of the same word). The
watched payload region sees exactly 2 × 15,068 = 30,136 byte reads and
3,767 word stores; nothing else ever touches it. The driver desc
checksum word 0x1224 is **never read** by anyone, and the constant
0xB56FB05D appears **nowhere in the whole 4 MiB image** (as a word, or
as a lui/ori / lui/addiu immediate pair).

**The 19 post-copy destination reads are the driver's config table, not
a digest.** After the copy, driver-payload code at 0xa20027ec/0xa200283c/
0xa2002888 reads 19 words from DRAM 0x02001684..0x020016CF — an
18-entry table with values 0x10,0x11,0x12,0,8,7,9,6,0xa,5,0xb,4,0xc,3,
0xd,2,0xe,1,0xf inside the driver's own image. A checksum over 0x3ADC
bytes would need ≥ 0x3ADC accesses; 19 word reads is a parameter fetch.

**The container stream is a byte copy with no accumulator.** The driver
loop at 0xa2001c08 (`lbu; addiu; sb; bnez` with the source OR'd with
0x98000000) reads all 28 descriptor bytes and all 34,444 container bytes
in one pass and stores them onward (the engine table FIFO uploads);
its return value is the constant zero (`move $v0, $zero` at both exits).
The desc[6] checksum bytes ARE read (ff f9 eb b5, part of the 28-byte
parse) — but nothing computes against them: the loop body has no xor/
add/rotate into any register.

**Constant scan**: 0xB5EBF9FF appears exactly once in the image — as the
desc word itself. Neither anchor is embedded in code, so no comparison
against either value exists in the flash.

**After the entry, no validation pass either.** Phase 2 continues past
0xA1600000 with a data-read watch on the pixel buffer (79,872 bytes) and
re-armed payload/container watches: the model head runs to its known
+0x50 fault (§19.2) with **zero** data-read hits — no bulk read of the
copied payload, the container or the image itself occurs.

### 22.4 The verdict on Q8

Within the observed boot chain — stage-1 (zeroBoot + copy), the driver,
and the eCos head under the engine model — **no code ever computes or
validates either checksum anchor**. The escalation §21.6 named ("the
loader's own checksum code path, if any") terminates with *none*: the
checksum words are packaging metadata, written by the vendor's build
tooling and consumed nowhere on-device. Consequently the algorithm is
not recoverable from the firmware (there is no code to reverse), and the
checksum oracle Q8 promised **does not exist in the device**. The
"per-descriptor accumulation" alternative dies with it: the only
per-descriptor code that touches the anchor bytes is a straight copy.
Remaining theoretical validators are outside the observed evidence
(engine-internal hardware behavior, post-boot eCos phases beyond the
model's reach) — both require hardware, the same boundary §21.6
recorded.

### 22.5 Where this leaves the head paradox

Q8 was the last *in-principle-available* offline oracle for the engine
LZ head paradox (§21.6 item 1); with its runtime refutation the paradox
space reduces to §21.6 item 2 (hardware-side residue: the real device's
pre-decode DRAM at 0x01600000 and DDR training residue) — the
two-tier-class-rules branch remains evidence-barred (§21.1). The
emulator-side program is therefore complete for this question: every
hypothesis class reachable without hardware has been tested and
refuted, and the refutation tooling (static oracles, the entry harness,
watchpoints) is in place for whatever a hardware session exposes next.

## 23. Milestone U — device coverage: the Cause-register corrections, the
### COP0 timer interrupt, and the keycode-consumer verdict

Per §21.6/§22.5 the emulator-side program on the engine LZ head paradox is
complete pending hardware; this milestone returns to device coverage (the
§21.6 candidate list "front-panel keycode labels, timer/interrupt
completion"). Three workstreams shipped: two architecture corrections with
an architecture-completion (the COP0 Cause layout and the Count/Compare
timer interrupt), the keycode-consumer hunt (static + dynamic, closed
negative), and the SoC timer register program pinned as evidence.

### 23.1 Scope and method

Everything below is either (a) architecture-defined behavior completed to
spec in the CPU model (covered by unit/integration tests; the real boot is
unaffected and remains pinned), or (b) runtime evidence extracted from the
real firmware with the existing instruments (access logs, watchpoints,
key injection). No peripheral behavior is invented: the timer block keeps
its verified counter-only semantics, and the keycode verdict is a closed
negative, not a guess.

### 23.2 The COP0 Cause-register corrections and the Count/Compare timer

**Corrections (externally cross-checked).** The Cause bit assignments in
`src/cpu/cop0.rs` were wrong in two places and are now pinned to the
architecture by a regression test (`cause_bit_layout_matches_architecture`)
whose constants are the Linux `arch/mips/include/asm/mipsregs.h` `CAUSEF_*`
definitions (checked 2026-10, torvalds/linux master):

| field | was | is | authority |
|---|---|---|---|
| BD (branch delay) | `1 << 22` | `1 << 31` | `CAUSEF_BD` (bit 31) |
| IV (interrupt vector) | `1 << 30` | `1 << 23` | `CAUSEF_IV` (bit 23) |
| TI (timer pending) | absent | `1 << 30`, read-only | `CAUSEF_TI` |

The old IV sat on TI's bit (bit 30 is TI, not IV) and the old BD sat on
WP's bit; a Cause write with `0x4000_0000` selected the special interrupt
vector for the wrong reason. The firmware never reads IV/BD and never
takes an interrupt (IE = 0, §11), so the real boot is unaffected — pinned
by the untouched boot tests.

**Timer completion (architecture, not invention).** The CPU model had
Count ticking (+2/instruction) but no Compare interrupt. Milestone U adds
the MIPS32r2 Count/Compare timer as the architecture defines it:

* a pending latch (internal state) set when Count **transitions onto**
  the Compare value — reset equality (Count = Compare = 0) is not a match;
  the first wrap-around match is 2^31 instructions away;
* a write to Compare **clears** the interrupt (the architecture's ack);
* the interrupt is input **IP7** (Cause bit 15, composed into reads
  alongside the platform-driven IP[7:2]), reported by the read-only
  **Cause.TI** bit, gated by **Cause.DC** (documented falsifiable reading:
  DC blocks delivery and TI while the latch persists), and delivered
  through the standard IM7/IE/EXL path;
* one deliberate model choice: the match is an edge on the +2/instruction
  Count model, so an odd Compare value never matches (documented; the
  firmware never writes Compare at all — §11.2).

Tests: 7 COP0 unit tests (layout pins, latch/clear/DC/wrap semantics) and
`tests/interrupt.rs::count_compare_timer_interrupt_delivers_via_ip7_and_acks`
— a synthetic program arms `Status = IE|IM7` and `Compare = 0x20`, idles,
takes the timer interrupt at EBase+0x180 (BEV=0), the handler acks with a
far Compare write, stores a marker and `eret`s back into the idle loop;
the test pins the marker, ExcCode = 0, TI/IP7 cleared after the ack, EXL
cleared by eret and EPC in the loop.

### 23.3 The keycode consumer: closed negative (static + dynamic)

§12.2's open item was the keycode consumer: the dispatcher writes
keycodes 0x100/0x400/0x1000/0x2000 into 0xBFFE384C — who reads them back?
Two independent hunts, both negative:

**Static** (`tools/fw_keycode_consumer.py`): chunked linear disassembly of
the whole 0x3ADC payload (99.0% coverage) with a conservative
constant-propagation pass ($s6 = 0xBFFE8000 seeded; lui/ori/addiu/li/move
tracking; registers invalidated on calls and loads). Exactly **six**
effective-address resolves to 0xBFFE384C exist in the payload: the
dispatcher's setup read (0x82002518 — `addiu $a1, $s6, -0x47b4` then
`lw ($a1)`, just before §12's documented loop range) and five accesses
inside the steady-state loop (four keycode stores + the loop re-read at
0x82002564). No other code in the payload touches the port, in either
segment alias. The keycode-constant scan finds only the dispatcher's four
`addiu $t*, $zero, keycode` pairs plus three unrelated constants (the
driver's IMG_WIDTH 0x400 at 0x82002134/0x820030cc, a config 0x1000 at
0x82000c30).

**Dynamic** (`examples/keycode_consumer.rs`,
`tests/interrupt.rs::keycode_port_traffic_comes_only_from_the_dispatcher`):
boot the real firmware ~30M steps with a READ|WRITE watchpoint on the port
(phys 0x1FFE384C) and key-line bit 2 held from reset. Every port access
issues from the dispatcher's PCs (0xA2002518/0xA2002564/0xA20025B0);
**no other PC ever reads or writes the port**. The write stream is pure
keycode 0x400 (auto-repeat while held, §12.1); the read stream is
constant `0x24` — key-line bit 2 **plus bit 5, the engine event line**,
live on the same port read (runtime confirmation of §13.3's shared-port
design; the model's engine completes synchronously, so bit 5 is already
set at the first poll). Counts hit the watch cap (2^20 hits) — the
attribution is on distinct PCs and value sets, unaffected by the cap.

**Verdict**: the keycode written by the dispatcher is consumed beyond the
observed horizon — by the eCos stage (still unreachable: §22.5) or by
hardware. Physical button labels per key line remain unknown, and §12.2's
"keycode→UI mapping" item is closed as evidence-bounded rather than
pursued by guesswork. The bit-6 = standby/failure reading (§12/§13.3) and
the per-line keycodes stand as the complete known mapping.

### 23.4 The SoC timer block: register program pinned

The timer (+0x600..0x620, §5) was modeled as a verified counter; the
complete register program of the real boot is now pinned by
`tests/timer.rs::real_firmware_timer_program_is_pinned` (extracted with
the new `examples/timer_probe.rs`):

* writes: `+0x00 = 0` exactly once (the delay() software reset),
  `+0x0C = 0x8095` exactly once, `+0x1C = 0` exactly once — **no other
  register in the block is ever written**;
* reads: the counter at +0x00 exactly 226 times (one `delay(10)`-style
  poll; the maximum value observed through reads is 0x385 = 901 =
  90×10 + 1 — the poll loop's one-tick overshoot past the 90×N target);
* no IRQ line of this block is observed or claimed — the block is used
  polled-only, and the model keeps that semantics (unit tests for the
  count/reset/config-store behavior included; unmodeled-register reads
  stay logged and visible).

### 23.5 Where this leaves device coverage

The §21.6 device-coverage candidates are both closed:

1. **front-panel keycode labels** — evidence-bounded closed negative
   (§23.3); the known mapping is key line → keycode (0/2/4/5 →
   0x100/0x400/0x1000/0x2000, bit 6 = standby/failure path), with the
   consumer beyond the horizon;
2. **timer/interrupt completion** — the CPU-side interrupt architecture
   is now complete *including* the Count/Compare timer (§23.2), the SoC
   timer block's program is pinned (§23.4), and the platform interrupt
   controller keeps its verified init-program semantics (the firmware
   never enables Status.IE, §11 — delivery remains injection-only, as
   tested).

Remaining emulator-side work is capability, not evidence: performance
(roadmap item 5), and whatever a hardware session exposes (§22.5).

## 24. Milestone V — performance: the device-log ring defect (13.4× boot
### throughput), measured and fixed without semantic change

After Milestone U the emulator-side evidence program was complete (§22.5,
§23.5); the remaining emulator-side work was capability, not evidence —
specifically roadmap item 5 ("~5 M instr/s today; a decode cache would lift
the 100 M-step runs from ~20 s to a few seconds"). This milestone delivers
that capability, with the mechanism chosen by measurement rather than by
plan.

### 24.1 Method

* `examples/bench.rs` — reproducible harness: untraced `Machine::run` of the
  real firmware, wall-clock throughput, stop reason. The workload is the
  deterministic boot chain (identical to the acceptance test; it halts at
  the §19.2 model-head fault after exactly 1,301,783 steps), so before/after
  numbers compare identical instruction mixes.
* An isolation probe (temporary, removed after use) ran a minimal
  `addiu/bne` loop directly against `Mips32::step` over a one-region bus:
  **71.6 M steps/s**. The raw interpreter core was never the bottleneck —
  the 43× gap to the 1.45 M instr/s full-machine rate had to be device/bus
  side. (A pure-Rust integer-loop calibration on the same host measured
  ~507 M iter/s, ruling out a slow environment.)

### 24.2 The defect: O(n) shifts in every device access log

All device models record every access into a shared capped log
(`AccessLog`). The record path was:

```rust
if log.len() >= self.log_capacity { log.remove(0); }   // VecDeque needed
log.push(entry);
```

`Vec::remove(0)` shifts the whole 8,192–16,384-entry buffer (128–256 KiB)
on **every logged access once the log is full** — and the boot's hot windows
(param RAM, DDR PHY/controller, UART, keyport, the engine register block)
are logged on every access. This one line dominated the entire run: 12
record sites across `devices.rs`. Replaced by a `VecDeque` ring
(`push_back`/`pop_front`, O(1)) with identical semantics (capped,
oldest-dropped — no recorded-content change; the touchmap and all
log-consuming tests are unchanged).

### 24.3 Secondary costs fixed (each measured, all semantics preserved)

* **Per-step trace formatting**: untraced run loops built and dropped a
  formatted `String` per executed instruction (`StepReport::desc`). The
  `CpuBackend::set_describe` flag now gates it; `run`/`run_until_pc` turn it
  off, `run_traced` keeps it on. Pinned by
  `cpu::mips32::tests::set_describe_off_keeps_execution_identical_and_empties_desc`
  (identical PC sequences with and without descriptions; `desc` empty iff
  off). This was worth +16% on its own (1.45 → 1.68 M instr/s).
* **Interrupt-line re-programming per step**: `set_ext_irq_lines` now
  returns early when the platform lines are unchanged (the steady state for
  the whole observed boot, IE = 0).
* **Watch arming**: `watch_armed` short-circuits on an empty watch list
  (the un-instrumented case; the zero-overhead path test already pinned the
  no-watch behavior).
* **Region lookup memo**: a 4-slot direct-mapped hot-region memo
  (hashed 4 KiB page key — a plain `page % 4` parks every region in slot 0,
  since all machine bases are page-aligned with page numbers ≡ 0 mod 4),
  range-validated on hit, filled on miss; `add_region` asserts the
  non-overlap invariant the memo relies on. Transparency pinned by
  `memory::tests::hot_region_memo_is_transparent_under_slot_conflicts`.
  Measured alone this was neutral on the real machine (the two hottest
  regions are already early in the region vec) — it is kept as
  insurance for region-count growth, at zero measured cost.

### 24.4 Results

| workload | before | after | speedup |
|---|---|---|---|
| deterministic boot chain (1,301,783 steps, `bench`) | 1.45 M instr/s | 19.4–19.6 M instr/s | **13.4×** |
| full boot test (`tests/boot.rs`, 7 tests) | 1.37 s | 0.13 s | 10.5× |
| two 30 M-step dispatcher-poll boots (`keycode_consumer`) | ~41 s | 3.06 s | 13.4× |
| whole test suite (106 tests, release) | ~29 s | ~3.2 s | 9× |

At 19.4 M instr/s a 100 M-step run costs ~5 s — roadmap item 5's target
("from ~20 s to a few seconds") is met, with margin.

**Semantic equivalence.** The pre-existing 104 tests pass unchanged —
including the deterministic boot-chain pin (steps within the pinned window),
the §20 entry-harness register-state pins, the §22 watchpoint pins and the
§23 COP0/timer pins — plus the two new regression tests above. The bench
reports an identical step count and stop fault before and after.

### 24.5 The decode-cache question

The roadmap had assumed a decode cache would be the mechanism. Measurement
refuted that plan: the interpreter core (fetch + decode + execute) measured
71.6 M steps/s against the 1.45 M full-machine rate, so decoding was < 2% of
the cost. The actual defect was in the device layer. A decode cache remains
available future work (it would still lift the raw core), but no longer
bottlenecks any documented workflow.

---

## 25. Milestone W — the chunk-resync dialect: §16.5's three open probes
### tested together and refuted (session record)

The three probes §16.5 left open — (a) per-chunk framing from the driver's
128-byte rounding rule, (b) the dialect meaning of the two mid-stream EOB
symbols, (c) tail alignment to the engine's start-bit + k×1024 grid — are
combined into one decisive experiment
(`tools/fw_chunk_resync.py`, committed and reproducible).

### 25.1 New structural fact: the body gap is all-zero padding

The dynamic block header ends at stream bit **1008**; the engine's
`PNG_BS_OFFSET = 1088` (container-relative) puts the body at stream bit
**1024**. The 16 bits between them — stream bits 1008..1023 — are **all
zero** (`gap` probe): the encoder padded the header up to the 128-bit
boundary, and the driver's body start (1088 = 64 SP-header bits + 1024)
is exactly `round_up(1008, 128) + 64`. This makes the "body = stream bit
1024" convention used by every tool since Milestone M an evidence-backed
fact, not a convention choice.

### 25.2 The hypothesis: same tables, grid resync

The premise is forced by evidence: the driver uploads the Huffman tables
**once** (§13.2's verified program has exactly one round A and one round
B), so the tail MUST decode with block 1's tables; the last remaining
freedom was the bit position each "chunk" starts at and what EOB does.
H-chunk: after the first EOB (bit 106,259, out 28,260) the engine resyncs
to the next grid position (origin + k·period) and keeps decoding with the
same tables; symbol 256 ends a chunk, not the stream.

Four semantics swept: **A** eob-grid (EOB jumps to the first grid position
≥ EOB end), **B** hard-grid (the decoder additionally resets at every grid
line, aborting codes that cross it), **C** eob-byte control (EOB → next
byte boundary), **D** continuous control (the RFC walk itself). Grids:
origin ∈ {0, 64, 512, 1008} × period ∈ {512, 1024, 2048, 4096} — origin 0
covers the hardware lattice (BS_OFFSET 1088 = stream 1024 + k·1024 ≡
k·1024), origin 64 the container-byte lattice, origin 1008 the unpadded
body lattice.

### 25.3 Baselines reproduced exactly

| walk | out | EOB#1 (bit, out) | EOB#2 (bit, out) | head |
|---|---|---|---|---|
| body 1024, continuous (D) | **75,398** | (106,259, 28,260) | (168,418, 45,137) | `60 81` |
| body 1024, eob-byte (C) | 75,496 | (106,259, 28,260) | (168,418, 45,236) | `60 81` |
| body 1008, continuous (D) | 75,394 | (106,259, 28,256) | (168,418, 45,133) | `00 00` |

The continuous walk reproduces §16.1/§16.3's anchor numbers exactly
(75,398 / 28,260 / 45,137), validating the tool; the earlier "EOB at
106,245" (§16.1 wording) is the EOB code's START bit — it ends at
106,259 as the chain probe reported.

### 25.4 Sweep verdict — the framing family is refuted

All 33 grid variants plus both controls (36 walks) complete to the stream
end (no invalid-symbol fault — complete tables decode anything, as §16.1
warned), each finds **exactly 2 EOBs**, and:

| metric | range over all variants | required |
|---|---|---|
| output bytes | 74,443 – 76,799 | 79,872 (Δ ≥ 3,073 everywhere) |
| strict validity | 46.2 – 48.8 % | ~90 %+ (real code) |
| ASCII runs ≥ 6 | 62 – 115 (no eCos markers) | the image's strings |

No variant meets the success criteria (fault-free, out within one row of
79,872, validity ≥ 90 %). The best output size (mode B, origin 64,
period 512: 76,799) is still 3,073 bytes short, and its validity is 47.1 %
— the familiar plateau. Honoring the mid-stream EOBs as chunk ends (probe
b) and every natural grid alignment (probes a, c) changes neither the
size nor the content quality. **The tail is not a grid-framed continuation
under block 1's tables.**

### 25.5 What this closes — and what it cannot

With §25.2's premise evidence-forced, the refutation covers the entire
"same-tables, re-framed tail" class: per-chunk framing (a), EOB dialects
(b), and grid alignment (c) are closed together. Combined with §16.1
(no new block header exists in the tail) and §17.5 (no filter geometry),
the only remaining hypotheses for the SP tail are the ones §21.6 already
bounded: **combined length+distance value semantics with position shift**
(§16.3's surviving class) or hardware-side residue that needs a real
device. The head paradox and the size gap (−4,474 under the ±2-per-match
predicates) stand as recorded; nothing in this sweep moves either.

## 26. Milestone X — the combined length+distance class: tested jointly
### and refuted (session record)

§25.5 left exactly one in-silico hypothesis class standing. Every prior
length search had run with RFC distances (§15.6's 64 exact-size ladders
and §18.2's 477-seed descent — all ~50 % strict validity), and every prior
distance probe had run with RFC lengths at fixed accounting (global
offsets ±16, §17.3; ring aliases, word-aligned sources, snapshot copy,
§15.6). The joint class — length semantics × distance semantics with the
position shift both imply — had never been swept as such. This milestone
closes it (`tools/fw_combined_semantics.py`, committed and reproducible).

### 26.1 Method

The bit-stream parse is evidence-pinned (§14.1: same Huffman codes, same
extra-bit counts; "any hypothesis that changes what bits are read is
dead"), so a value-semantics variant is fully described by two tables —
`len = f(ls) + eb`, `dist = g(ds) + deb` — plus, for the §16.3 predicate
families, a fixed extra-emission predicate over the RFC populations.
The sweep crosses:

* **Length families (72)** — L0 RFC control; L1 the three §16.3
  exact-count predicates (`dist<96`, `dist≥3485`, `len≥5&dist<3073`,
  each +2 on its 2,237 RFC-population matches ⇒ size exactly 79,872);
  L2 the group-uniform ladder family with the exact size equation
  `Σ count[li]·g_group(li) = 4,474`, g ∈ 0..12 — **64 solutions**,
  reproducing §15.6's count exactly; L3 code-shifted ladders
  `f(ls) = LBASE[clamp(ls+d)]`, d ∈ {−2,−1,+1,+2} (a miswired ladder
  ROM row, keeping each code's extra-bit count).
* **Distance families (35)** — D0 RFC control; D1 global offsets
  k ∈ {±1..±8192} (16 magnitudes, both signs — extends §17.3's ±16
  search by three octaves); D2 code-shifted `g(ds) = DBASE[clamp(ds+d)]`,
  d ∈ {−2,−1,+1,+2}. Extra-bit counts stay pinned in every family.

2,520 combinations decoded on the continuous walk (mid-stream EOBs as
no-ops, §25.3 baseline) with the 32 KiB zero preset (§13.1/§18.3), each
scored by three pre-registered oracles: **size** within one row of
79,872, **head** (first 16 words strictly decodable, no COP0-reserved-rs,
no BREAK), **content** (strict capstone validity ≥ 90 % — real code
measures 91–95 %, §14.2 — plus ASCII runs and eCos markers reported).

### 26.2 Baselines reproduce

| row | out | strict |
|---|---|---|
| L0 × D0 (RFC control) | **75,398** | 48.3 % |
| L2 (64 exact-size ladders) × D0 | 79,872 | best 49.2 % |
| L1 predicates × D0 | 79,872 | ≤ 50.5 % |

The control reproduces §25.3's anchor (75,398) byte-for-byte and the
§15.6 ladder-solution count (64, all ~50 %), validating the tool against
both prior experiments. (Implementation note: the tool's copy loop was
cross-checked byte-identical against `fw_ring_sweep.rfc_decode` — a
self-overlap guard bug found during this cross-check was fixed before
any measurement was taken; all numbers above are post-fix.)

### 26.3 Sweep verdict — zero survivors

| metric | range over all 2,520 combinations | required |
|---|---|---|
| output bytes (size-qualified rows) | 78,848 – 79,872 | 79,872 ± 1,024 |
| strict validity | median 47.8 %, **best 55.0 %** | ≥ 90 % |
| head oracle (size-qualified) | 2,381 pass the static filter | — |
| ASCII runs ≥ 6 | 19 – 42 | the image's strings |
| eCos markers (`eCos`/`RedBoot`/`SPHE`/`1506`/…) | **0 combinations** | present |

The best size-qualified content score anywhere (L2 ladder × D1 k=+8192:
55.0 %) is garbage-plateau territory — the calibrated data band is
91–98 % for real code (§14.2) vs 23–48 % for data/garbage — and the
+8192 offset that tops the table is itself a bias artifact (it pushes
most copies into the zero preset, trading corruption for uniformity).
The three §16.3 predicate families peak at 54.2 % (× k=+8192); with RFC
distances they stay ≤ 50.5 % — the exact Δ = 4,474 size match does not
buy content. No combination produces one eCos marker.

### 26.4 What this closes — and the honest remainder

With §26, the §16.3/§25.5 surviving class is refuted within its natural
family space: length ladders (exact-size, predicate, shifted) × distance
maps (offsets to ±8192, code-shifted), position-shifting in every length
family, on the evidence-pinned parse, under all three oracles. Combined
with the prior closures — §16.1 (no new headers), §17.5 (no filter
geometry), §19.2/§21.2/§21.4 (start-bit, rank-class, T-byte), §20.3
(literal-value maps), §21.5 (output transforms), §22.4 (no on-device
checksum oracle), §25 (framing/resync) — **every hypothesis class
reachable without hardware is now refuted with data**. The formal
remainder is exactly: (a) §18.4's m2/m4-style extra-bit-*accounting*
joint solve (59 unknowns), which §19.3's head-DFS already showed cannot
fix the head (escapes only via degenerate self-copy floods) and whose
start-bit × m1/m2/m4 variants §19.2 refuted dynamically; and (b)
hardware-side residue (§22.5). The emulator-side program on the SP
stream is complete: the engine's LZ value semantics are not recoverable
from the firmware and the tools, and any further progress requires a
real device's post-decode DRAM dump (the §22.5 instrument set is ready
for it).

