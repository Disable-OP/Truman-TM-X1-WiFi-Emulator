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

## 27. Milestone Y — the hardware-session handoff: ingestion and the
### pre-registered decision tree (session record)

§26.4 closed the in-silico program with exactly one remainder: a real
device's post-decode DRAM dump (§22.5). The missing piece was not
analysis but logistics — nothing ingested a capture, and no decision
tree was registered for the two decisive experiments. Milestone Y adds
exactly that, and nothing else.

### 27.1 What was added

* **Pure comparison analysis** (`src/machine.rs`): `compare_dumps`
  (matching count/per-mille, first divergence, 1024-byte divergent-block
  map, longest match run; size mismatch compares the shorter prefix and
  records both lengths) and `classify_residue` (non-zero structure of a
  capture against the §18.3 all-zero expectation). Integer-only,
  deterministic, pinned by `tests/hwsession.rs` (10 tests incl. the
  real-firmware reference extraction and a boot-determinism pin).
* **One-command ingestion** (`examples/dram_compare.rs`): `--post` (P1:
  boot, extract the engine's pixel reference, compare), `--pre` (P2:
  residue classification), `--ref` (offline file-vs-file). Exit codes
  distinguish clean matches (0) from actionable divergence (1).
* **The capture protocol** (`docs/hardware-session.md`): windows, sizes
  (79,872 = 0x400 × 78 from the engine's registers), halt points, the
  JTAG/EJTAG plan (OpenOCD `mips_m4k`, `dump_image`; explicitly
  unvalidated on this board), capture-integrity checklist, and the
  pre-registered decision tree for P1/P2 outcomes.

### 27.2 Verification without hardware

The full path is exercised with the tools the repo already trusts: the
real firmware boots, the pixel reference is extracted (79,872 bytes,
1024×78), and the comparator localizes synthetic perturbations exactly
(single-byte flip → `first_divergence` at the flipped offset, one
divergent block; two flips → two blocks; the boot is byte-deterministic
across runs, re-pinned through the comparator's own extraction path).
The capture protocol itself remains unvalidated until a hardware
session — as this document says everywhere it could.

### 27.3 What this changes

Nothing about the evidence. §26's refutations stand; the SP-stream
program remains hardware-blocked. What changes is the cost of the
hardware session: the moment a capture exists, one command produces the
verdict and its block map, and the decision tree says what each verdict
means before anyone sees the data. That is the whole milestone.
## 28. Milestone Z — the per-step loop, measured and closed (session record)

§24.5 left two questions open: whether tick batching could help, and
whether the decode cache was ever the right mechanism. Milestone Z
answers both with the §24 method — probe first, fix only what the
measurement convicts, pin equivalence by the suite — and closes the
loop-overhead program.

### 28.1 Method and probe

`examples/loop_probe.rs` (committed, reproducible) anchors the full boot
run and then measures every per-step loop component in isolation with
public API only: `bus.tick()` fan-out (whole / intc-only / timer-only
buses), the engine request poll, the pending-hint check, the watch
check, and the raw-core ceiling on a synthetic self-loop. Two probe
defects were found and fixed before any number was recorded: the
raw-core microbench must call `set_describe(false)` (the `CpuBackend`
contract defaults descriptions ON for direct steppers — a String per
step, §24.3's own finding), and microbenches must defeat constant
folding (`black_box`/counter sinks).

### 28.2 Attribution (pre-Z, on the 1,301,783-step boot chain)

| component | ns/instr | note |
|---|---|---|
| full machine.run | 51.3 (19.50 M/s) | the §24.4 anchor, reproduced |
| raw core (self-loop) | 15.7 (63.8 M/s) | §24.1's ceiling, re-confirmed |
| bus.tick fan-out | 15.5 | intc 8.7 + timer 7.1 |
| engine poll | 2.2 | `borrow_mut` + take per step |
| has_watches + IRQ wiring | ~1.5 | both already fast-pathed in V |

The remaining gap (~16 ns) is data-access dispatch and the access-log
evidence recording — the machine's own contract, not overhead.

### 28.3 Three fixes, all semantics-preserving

* **IntController change-detection tick (§28)**: the output line is a
  pure function of (sources, enable registers); the registers already
  recompute on every write, so the per-step recompute — two `BTreeMap`
  lookups per instruction — collapses to one sources read + compare.
  A raised source still asserts exactly on the next step boundary
  (`raise_int_sources`'s contract), pinned by
  `tests/looptune.rs::intc_change_detection_tick_matches_unconditional_recompute`.
* **Timer counter shared cell (§28)**: the counter moves to a
  `Rc<Cell<u32>>` the run loops increment directly — one count per
  executed instruction, as before, but without the tickable-fan-out
  trait dispatch. `Device::tick` still increments (standalone buses
  unchanged); `tests/looptune.rs` pins the shared-cell semantics and
  the software-reset path (§9).
* **Engine request hint (§28)**: a `Rc<Cell<bool>>` beside the
  `RefCell` makes the per-step poll a plain load; `take_request` stays
  authoritative and clears the hint. One inversion defect in the first
  implementation was caught by its own regression test before any
  measurement was taken.

### 28.4 Results

| metric | pre-Z | post-Z |
|---|---|---|
| deterministic boot chain | 19.50 M instr/s | **25.4 M instr/s (+30 %)** |
| bus.tick fan-out | 15.5 ns | 3.1 ns |
| machine-path timer increment | 7.1 ns | ~0.5 ns |
| engine poll | 2.2 ns | ~0.3 ns |
| raw core | 63.8 M/s | 63.8 M/s (untouched) |
| boot-chain steps | 1,301,783 | 1,301,783 (identical) |

Equivalence: all 116 prior tests pass unchanged (including the boot
chain step pin and the §23.4 timer-program pin), plus 3 new regression
pins in `tests/looptune.rs`.

### 28.5 Verdicts on the two open questions

* **Tick batching/decimation: refuted for this emulator.** The boot's
  `delay(10)` polls read the timer counter every iteration; deferring
  ticks would change the observable count at read time. QEMU's icount
  decimation reaches the same conclusion and pays for it with explicit
  MMIO machinery (restore-the-budget / single-instruction re-blocks,
  QEMU `docs/devel/tcg-icount.rst`); this repo pins determinism by
  tests instead and keeps per-instruction ticks — now at ~3.6 ns total
  for timer + intc.
* **Decode cache (§24.5): verdict unchanged, now with post-V data.**
  The raw core is 63.8 M/s and untouched by the loop fixes; the machine
  gap remains device/dispatch-side. A decode cache would only raise the
  raw-core ceiling, not the boot rate — it stays optional future work,
  correctly de-prioritized.

The per-step loop program is complete: every removable cost identified
by measurement has been removed, and what remains is the evidence
model's own cost.

## 29. Independent reproduction on a fresh toolchain (session record)

No code change in this session. Its purpose was independent
verification of the 0.22.0 state after an environment reset, because
the P1/P2 decision tree (§22.5, `docs/hardware-session.md`) leans on
one claim above all: that the emulator-side reference is stable. A
reconstructed environment is the cheapest way to test that claim
without hardware.

### 29.1 Environment and method

This session began from a reset environment: the workspace snapshot
had reverted to a pre-Milestone-N commit and the toolchain was gone.
The recovery path is itself part of the verification — it exercises
the reconstruction a new contributor would face:

* toolchain reinstalled from scratch (rustup stable, minimal profile,
  plus rustfmt/clippy — rustc 1.99.1);
* the stale `target/` of the old snapshot invalidated by the
  toolchain change, so every crate rebuilt from source;
* the working tree synced to `origin/main` at `71fc805` (Milestone Z)
  only after verifying the local diff was mode-only and that `HEAD`
  was contained in the remote — nothing was discarded.

The four gates were then run in CI's order, and the remote CI history
was audited via the API (read-only).

### 29.2 Results

* **All four gates green**: `cargo fmt --check`; `cargo clippy
  --all-targets --all-features -- -D warnings` (zero diagnostics);
  `cargo test --release` — **119/119**, including the 1,301,783-step
  boot-chain pin and the real-firmware pixel-reference extraction;
  `cargo check --all-targets`.
* **Determinism spans environment reconstruction**: the boot-chain
  step count and the 79,872-byte reference window reproduce
  byte-identically after the full rebuild. The `--post`/`--pre`
  verdicts therefore compare a device capture against a reference
  that is stable across reconstructed environments, not merely across
  runs in one.
* **CI audit**: 14/14 workflow runs green, from `23471b2` (Milestone
  M, the first push after CI was added) through `71fc805` — the remote
  gates have executed and passed on every push, not only locally.

### 29.3 What this does not claim

No new device data, no new refutations, no performance re-measurement
(the §28 numbers were recorded on the earlier toolchain and are not
revised). §26's and §28's verdicts stand unchanged. The single next
step for the boot blocker remains the hardware session
(`docs/hardware-session.md`): P2 cold-boot capture → `dram_compare
--pre`, then P1 → `dram_compare --post`, one command per experiment.

## 30. Milestone AA — the vendor driver lineage: the engine program named,
### the test vector, and the offline program closed (session record)

The engine's register block is no longer inferred: the Sunplus SPHE-family
U-Boot image decoder (`sp_imgdec.c`, same SoC line, sibling device's GPL
dump; archived under `tools/reference/` with attribution) programs the same
0xBFFE3800 block with the same constants the Truman loader writes. Every
previously-unexplained constant now has a vendor-named meaning.

### 30.1 The register program, named by the vendor's own code

* `PNG_DEC_MODE` = 1 (reset), then `((4&7)<<2)|(1<<7)` = **0x90**:
  `bs_dma_size=4` (128-byte DMA) and **`zlib_mode=1`** — "0: decoding PNG
  images, 1: decoding ZLIB streams".
* `PNG_HUF_TB_ADDR=0x20`, `PNG_SCL_BUF_ADDR=0xC0`: internal SRAM
  partitioning (32 + 160 words), identical constants.
* `PNG_CHUNK_SIZE = stream_len + 8 + (len%2) + 6` — the Truman driver's
  "+6 fudge" appears **verbatim in the vendor driver, with the same
  `// ???` comment**.
* `PNG_BS_OFFSET` = `(byte_offset<<3) − bit_offset`, rounded UP to a
  32-bit boundary = the symbol start bit (1088 on this container).
* `PNG_IMG_WIDTH=1024`, `PNG_IMG_HEIGHT=u16@2` — "Set decompressed data
  size, unit: K-byte"; `PNG_PXL_LINE = height−1` (row countdown).
* `PNG_IMG_TYPE = (color_type<<5)|bit_depth`; **0x68 = palette/8bpp** —
  "Set color type and bit-depth as 8-bit palette **to avoid output
  translation**": raw byte passthrough, no pixel conversion.
* `PNG_REM_CHUNK_SIZE_M` = the **scramble key** ("2-bytes scramble
  unit"); the container header check is `zlib_header == 0x5350` ("SP") —
  the vendor's "**ZLIB/Scrambler header**".

Consequences. First, `zlib_mode=1` means the engine decodes **raw
DEFLATE** — there is no PNG row filtering in this mode, which is *why*
the §17.3 filter-family refutations all failed to help: the families
were dead because the mode never filters. Second, the Truman loader is
this SDK's lineage: the "SP" container is the vendor's ZLIB/Scrambler
format, and the whole engine program maps 1:1 onto `imgdec_png_decode_zlib()`.

### 30.2 The SDK's own test container (new instrument)

`cmd_sp_png_dec.c` embeds a test "SP" container (height 9, stream
0xE98 bytes) compressed by the SDK's `sp_gzip` tool with scramble key
0xa6fc (`SP_PNG_DEC_OP_MODE_TEST_PATTERN` path). Raw RFC inflate of that
vector: the dynamic header parses canonically, the **literals decode to
readable sparse hex-text in a zero sea**, and the walk dies with
"invalid distance too far back". Three direct facts:

1. the SDK compressor does **not** emit RFC-conformant distances — the
   hardware value semantics differ, exactly as the Truman stream's
   behavior indicated;
2. the container's literal bytes are **not scrambled** (the key is
   written, the literals are clean ASCII) — the scramble facility is
   inactive for these streams, and the "2-byte scramble unit" is not an
   output transform we are missing;
3. zero-preset references are legitimate (the sparse test pattern's zero
   sea) — §14.1's reading, now confirmed on vendor-authored data.

### 30.3 Two new refutations, and one impossibility

* **Per-code delta family refuted.** The most general "different silicon
  ROM ladder" form — length and distance value tables as RFC±δ per code —
  was beam-fitted on the full 25,027-symbol stream with the calibrated
  strict-validity oracle and zero-preset collapse
  (`tools/fw_value_fit.py`). It runs to stream end and **plateaus at
  45.4 %** with no per-code structure (the fitted deltas are scattered
  noise of the same magnitude §17.5's beam bought by oracle
  exploitation). Together with §18.2 (ladder search), §17.5 (mini-grid),
  and §26 (joint sweep), the value-table family is closed in every
  parameterized form tried.
* **The m4 accounting is underdetermined offline.** With no extra bits
  consumed (59 ROM values), the decoded pattern *defines itself*: any
  self-consistent sparse assignment satisfies the stream, the literals,
  and the size anchor simultaneously. `tools/fw_m4_fit.py` demonstrates
  the collapse on the test vector (the zero-fill attractor reaches the
  image size with ~90 % of the stream unconsumed; ranking by validity,
  size, or consumption each evicts the true state or keeps only
  degenerate ones). No in-silico oracle — alphabet, size, validity, or
  consumption — separates candidates without knowing the true pattern.
* **No offline source of the answer remains.** Exhaustive scans find no
  plaintext copy of the eCos image anywhere in the 4 MiB flash (entry
  markers `0x3400xxxx`, "SP" containers, capstone-decodable code regions
  — the only tail-region content is config/MAC records). The Q8 checksum
  algorithm is not recoverable from the known pair (driver payload →
  0x5DB06FB5) by a ~500-variant sweep across CRC-32 catalogs,
  Fletcher/Adler, sums, hashes, and 2-byte-unit accumulations
  (`tools/fw_checksum_hunt.py`) — it lives in the vendor's build
  tooling. The `sp_gzip` compressor is not publicly available
  (searched; only the U-Boot *consumer* side is public).

### 30.4 What this changes

The engine model's register program is now fully named and the SDK
lineage proven; the blocker is exactly the hardware's length/distance
value tables — one 32 KiB-window decoder whose ROM values the vendor
never published. The offline program is closed with new evidence rather
than assumption: §22.5's hardware boundary now holds not just
family-by-family but structurally (§30.3). The hardware session
(`docs/hardware-session.md`) remains the single unlock — a P1 capture
(`dram_compare --post`) decides every candidate decode in one command —
and this milestone makes that session trivial to prepare: the register
program, the container format, and the engine's programming sequence are
all documented from the vendor's own code.

## 31. Engineering correction: the hot-region memo vs. nested device regions (v0.23.1)

Found during session reconstruction (a clean checkout of 601a22e fails the
boot tests in a **debug** build with the Milestone V layout assertion):

```
overlapping regions: ddrctrl [0x1ffe1500..0x1ffe16a0) vs ddrphy [0x1ffe1580..0x1ffe15e0)
```

### 31.1 The defect

The `DdrPhy` training block (0xBFFE1580..0xBFFE15E0) has always been
registered *inside* the `DdrCtrl` span (0xBFFE1500..0xBFFE16A0); the
pre-Milestone-V bus resolved the overlap by linear-scan first match
(PHY registered first, so PHY accesses win), and `DdrCtrl`'s doc note
documented that reliance. Milestone V added the 4-slot hot-region memo
(§24) together with a `debug_assert!` that regions do not overlap — the
soundness condition the memo's re-validation actually needs, because the
two DDR windows share one 4 KiB page: a slot last filled with `ddrctrl`
validates for an access landing in the PHY window and misroutes it
(the cached-range check cannot see the nested region). Debug builds
took the assert; release builds (CI's `cargo test --release`) compiled
the assert out and kept the memo. Deterministic misrouting did not
perturb the pinned boot chain — all boot markers, the step range, and
the final-PC pin reproduce identically — but the invariant the memo
itself declares was violated in the shipped layout, and local debug
test runs have been impossible since Milestone V without noticing.

### 31.2 The fix

`DdrCtrl` is now exposed as two **non-overlapping** windows backed by
one shared register file (`DdrCtrlShared`, the same pattern the
decompression engine already uses): `ddrctrl` (0x1500..0x1580) and
`ddrctrl-hi` (0x15E0..0x16A0, absolute offset = local + 0xE0). Both
windows record into the shared `ddrctrl` access log with absolute
offsets, so the access log content is byte-identical to the
single-region device; register identity (+0x1504/+0x1600/+0x1628
status bits) is keyed on the absolute offset and unchanged.

### 31.3 Verification

- 119/119 tests pass in **debug** (first clean debug run since
  Milestone V) *and* release; fmt, clippy (`-D warnings`), check green.
- The boot-chain test's observable pins (console markers through
  "Go eCos"/"Boot from SPI", step range, final PC page 0xA1600000) are
  unchanged — confirming the release-mode memo misroute was benign for
  the pinned horizon and the fix is behavior-preserving.
- No new evidence about the §30 blocker; this is a correctness repair
  to the emulator's own memory subsystem, not a firmware finding.

### 31.4 The value-table seam (v0.23.2)

§30.2/§30.3 bound the hardware's LZ value semantics to a 59-constant
ladder that offline fitting cannot recover. The engine decode path now
exposes exactly that ladder as an injectable value
(`inflate::ValueTables`, RFC 1951 §3.2.5 by default): the machine's
decode goes through the named `ENGINE_VALUE_TABLES` constant, and
`inflate_engine_with` accepts any recovered assignment — every family
the offline analyses bounded is representable (per-code deltas, uniform
extra widths, the m4 no-extra-bits accounting). A differential test
hand-encodes a minimal dynamic block whose bits carry no value
information and pins different outputs under the RFC and custom
ladders, proving the tables flow through the engine path end-to-end.
The hardware-session edit is therefore literally one constant: replace
`ENGINE_VALUE_TABLES` with the capture-recovered values and run
`dram_compare --post` (docs/hardware-session.md).

## 32. Milestone AD — the post-handoff horizon, mapped (v0.24.0)

### 32.1 The instrument

Milestone AC made the engine's value tables injectable; the other half
of the post-capture procedure is knowing what the machine does once the
decompressed stage starts executing. The engine now answers that
directly: `Machine::map_handoff(pre, post, head)` boots the real chain
up to the loader's `jr v0` entry (`MachineConfig::handoff_entry`,
default 0xA1600000 — the PNG_DEC pixel output address, §17), stopping
before the stage's first instruction with the loader's register state
live, then executes up to `post` further steps under the post-handoff
instrument:

- a per-4 KiB-page executed-instruction histogram (`PageHistogram`,
  recorded per actually-executed instruction — a faulting instruction is
  not counted);
- MMIO watchpoints over every modeled device window (the sysreg page and
  the three vendor windows), recording each post-handoff access with its
  issuing `(step, PC)`, kind, width and value;
- an optional per-instruction head trace of the first `head` post-handoff
  instructions;
- the UART console bytes emitted after the handoff.

Everything is bounded by construction (pre/post budgets, watch cap with
an explicit overflow flag, a 256-entry page histogram, a capped head
trace) and deterministic (identical maps from fresh machines; identical
counts in debug and release). The CLI exposes it as
`truman-emulator map <firmware> [--pre-steps N] [--post-steps N]
[--head N]`. The map is evidence-only: it asserts nothing about
stage-internal semantics, and it is value-table-agnostic — the same
command that documents the RFC decode's dead end today maps the real
menu path the moment the recovered tables land (§32.3).

### 32.2 The current horizon: 20 instructions, no I/O, one trap

Measured on the committed image (SHA-256 prefix `5ca012b2…`), the map is:

| Field | Value |
|---|---|
| handoff step | 1,301,763 (`run_until_pc` entry; +20 = the 1,301,783-step full-run trap pin of §17) |
| registers live | `v0`=0xA1600000, `v1`=0x00013800 (79,872 = 78×1024 — the driver's pixel-count return, §13.6), `a0`=0x00000400 (= 1024 = PNG_IMG_WIDTH), `sp`=0x82004B80 (the driver's DRAM stack) |
| post-handoff steps | 20 (all in the entry page 0xA1600000) |
| stop | `UnknownInstruction { pc: 0xA1600050, word: 0x0000FFFF }` |
| MMIO | none — zero accesses over every modeled device window |
| console | none — zero UART bytes after the handoff |

The 20 executed words are `00008160`, then mostly `00000000` (NOPs)
punctuated by zero-upper-half words (`0x02140000`, `0x00004800`,
`0x00008F00`, `0x00A5A703`, `0x00360000`, `0x20000000`, `0x00002000`,
`0x02AC2700`) — harmless NOPs and shifts — ending at the `FFFF` halfword
that kills the run. In dynamic terms this *is* the §19.1 head paradox:
the RFC-decoded head executes trivially and dies on a literal within
+0x50, exactly as §19.3's static oracle predicted ("heads die on
literals; ladder values cannot fix the head — the true decode's head
must differ in kind"). The instrument therefore adds no new doubt about
the blocker; it measures the blocker's blast radius precisely: from the
handoff to death there is no device access and no output, so every
post-handoff behavior the product ever shows (menu render, key
dispatch, anything) lies beyond a wall this decode never gets past.

### 32.3 What this pins — and the post-capture procedure

- The menu path's first unmapped step is now a measured quantity: the
  stage dies at entry+0x50 having touched nothing. When the capture-
  recovered value tables replace `ENGINE_VALUE_TABLES` (§31.4), the
  post-capture procedure is exactly: swap the constant, run `cargo test`
  (all pins re-verify), then run `map` — the executed-page histogram
  becomes the stage's real code map, the MMIO record becomes its device
  init sequence in order, and the console delta shows what it prints. No
  further instrumentation is anticipated. (Extended by §33: the
  data-write map — §32.3's map left the framebuffer question open, and
  the "no further instrumentation" line proved to be the gap itself.)
- Pinned in code (tests/map.rs): the synthetic instrument mechanics
  (pages, MMIO attribution, console delta, unreached-handoff shape,
  histogram ordering) and the real-firmware horizon above — handoff step,
  live registers, 20 steps, the trap word at 0xA1600050, empty MMIO,
  empty console, determinism across fresh machines.
- Fresh survey (this session): public sources still contain no PNG_DEC
  value-ROM evidence beyond the vendor driver lineage of §30. One
  same-SoC avenue remains *out-of-repo*: a second SP container from the
  same 1506HV family (another firmware dump encoded by the same vendor
  tool against the same hardware ROM) would over-determine the 59
  constants that one container leaves structurally underdetermined
  (§30.3). Obtaining one means downloading third-party firmware, which
  this project's constraints prohibit; the option is recorded here for
  the owner's decision and deliberately not exercised.

## 33. Milestone AE — the written-memory map: the framebuffer question (v0.25.0)

### 33.1 The gap §32 left, measured in its own test

The §32.3 post-capture procedure maps three of the four things "boot
AND render AND show the menu" needs: the executed-page histogram is the
stage's code map, the MMIO record is its device init sequence, the
console delta is what it prints. The fourth — *where the stage writes
data* — was invisible by construction. The handoff entry (0xA1600000)
is itself DRAM (the PNG_DEC pixel output buffer, §17); the menu surface
will be DRAM too; and the AD instrument's own test pins the principle
explicitly — its stage's marker store (`sw` to DRAM) is documented as
"must stay out of the MMIO record". A real menu render would have lit
up `exec_pages` and `mmio` and left the framebuffer, the one artifact
render verification needs, unobserved. §32.3 anticipated no further
instrumentation; that anticipation was itself the gap, and this section
closes it.

### 33.2 The instrument

`Machine::map_handoff` installs a fifth watch alongside §32.1's four
device-window watches: the whole DRAM span
(`MachineConfig::dram_base`/`dram_size`), **write-kind only** —
instruction fetches are the executed-page histogram's business, and
data reads are not evidence of rendering. Every post-handoff watch hit
is routed by kind + physical address (`record_post_hit`):

- a DRAM data store increments `writes_total`, the written-page
  histogram (`write_pages`; the same 4 KiB-page `PageHistogram` type,
  top 256), and — for the first `WRITE_SAMPLE_CAP` = 64 stores — a
  `WriteEvent { step, pc, vaddr, phys, size, value }` sample. The
  sample keeps the *first* stores deliberately: before a render loop
  saturates the pattern, its head exposes stride and placement (for
  this SoC's own constants: consecutive stores 0x400 apart are
  1024-byte rows — the PNG width a0 carries into the handoff, §32.2).
- every other hit stays in the MMIO record; nothing recorded is
  dropped, including any hypothetical hit that is neither a DRAM write
  nor a device-window access (defensive: the map loses no evidence).

The split is by classification, not by which watch fired, so the
instrument's semantics do not depend on watch ordering. Boundedness is
inherited from §32.1 (pre/post budgets, the shared 1 M-hit watch cap
with `mmio_overflow` — its doc now covers both records) plus the new
64-event sample cap. Determinism is inherited end-to-end and pinned.
The CPU core is untouched — zero hot-path changes; the counting rides
the bus watch machinery that has carried the Q8 instrument since §22.

### 33.3 The horizon, re-measured: the RFC stage stores nothing

The CLI `map` (same command, same defaults) now ends its MMIO section
with the data-write section. On the committed image (SHA-256 prefix
`5ca012b2…`), the §32.2 horizon gains one line, and it is empty:

| Field | Value |
|---|---|
| handoff / registers / steps / stop / MMIO / console | exactly §32.2 — 20 instructions, trap on 0xFFFF at 0xA1600050 |
| post-handoff DRAM stores | **0** (`writes_total` = 0; `write_pages` empty; `write_sample` empty) |

§32.2's "touches nothing" is now a measured statement about data writes
as well: the RFC-decoded stage's blast radius is fully mapped — no
device access, no data write, no output, from the handoff to death.
Pinned in `tests/map.rs` (6 tests): the synthetic marker store lands in
the write map with full attribution (step 5, kseg0 vaddr 0x80003000 /
phys 0x3000, Word, value 0x4D) while the MMIO record stays at exactly
the console store; a 100-store render-loop stage pins the counting
(100/100 histogrammed and totalled) against the capped sample (first
64, values advancing 0x40, 0x41, …, one page 0x5000); the real-firmware
pin carries the zero-writes measurement; determinism covers the new
fields. 126 tests, debug and release, all green; boot pins unchanged.

### 33.4 The post-capture procedure, restated

Unchanged in shape, richer in return: swap `ENGINE_VALUE_TABLES` for
the capture-recovered ROM values (§31.4), `cargo test` (all pins
re-verify), then `map`. The executed-page histogram is the code map,
the MMIO record the device init order, the console delta the startup
print, and the data-write map the framebuffer answer — written pages
show where the surface lives, the first-store sample shows stride and
placement without any semantic assumption. With §33 the anticipated
instrument set for the menu path is complete: code, devices, console,
memory. The blocker itself is unchanged — the 59 value-ROM constants
remain hardware-gated (§30.4); the fresh survey this session (web and
public code search, register-name and SoC-family queries) again found
no PNG_DEC value-ROM evidence beyond §30.1.


## 34. Milestone AG — the vendor completion protocol: the interrupt
### register, the probe window, and a new capture option (v0.26.0)

A close re-read of the already-archived vendor sources
(`tools/reference/`, byte-identical to upstream `vicyap/sony-xav-ax100`
— verified this session by fresh download + diff) extracts the parts of
the vendor driver that §30 did not need: the *completion* side of the
PNG_DEC programming flow. Three Truman registers that §10.3/§12 decoded
behaviorally are now vendor-NAMED, one of them changes an
interpretation this repo has carried since Milestone G, and one of them
is a new hardware-session capture option. No emulator semantics change.

### 34.1 0xBFFE384C is PNG_DEC_INTERRUPT — the "key dispatcher"
#### re-attributed

The vendor drivers' completion loop (`sp_imgdec.c`; the kernel HAL is
identical) polls one register and services these status bits, acking by
writing `bit << 8`:

| bit | vendor name | ack write |
|---|---|---|
| 0 | Bitstream empty interrupt | 0x100 |
| 1 | End of block interrupt | 0x200 |
| 2 | End of chunk interrupt | 0x400 |
| 3 | End of ZLIB interrupt | 0x800 |
| 4 | End of image interrupt | 0x1000 |
| 5 | Pixel buffer full (ready) interrupt | 0x2000 |
| 6 | Decode error interrupt | (error path) |

§12's dispatcher at DRAM 0x82002548..0x82002630 tests bits **0, 2, 4,
5, 6** of 0xBFFE384C and writes 0x100 / 0x400 / 0x1000 / 0x2000 — the
vendor bit map and the vendor ack encoding, exactly, minus the two
block-boundary bits (1/3) the single-block SP decode never asserts.
§13.3's engine poll is bit 5 = "Pixel buffer full (ready)" acked with
0x2000, vendor-named. And §12's bit-6 "standby key" long path reads
0xBFFE3824 = PNG_DEC_CTRL — the vendor error branch reads PNG_DEC_CTRL
to name the error type (DMA busy / chunk type / Adler checksum / decode
suspend / non-compressed length).

Consequences, none behavioral:

* The register is PNG_DEC_INTERRUPT. The §10.3 reading of this traffic
  as a "front-panel key port" emitting "keycodes" is re-attributed: the
  tested bits are engine interrupt lines, the written "keycodes" are
  their acks. §23.3's negative keycode-consumer verdict now has a
  structural explanation — no code consumes keycodes because the
  dispatcher produces none.
* The low-bit "key line" source on the 1506HV (real front-panel GPIOs,
  if any are muxed here) is unknown; `KeyPort`'s level model is
  retained as the documented falsifiable choice for exactly those bits,
  and `press_key` remains a register-level instrument. All observed
  traffic is engine-side and identical under both readings; every test
  pin holds (§12.1's end-to-end proof now demonstrates the firmware
  reacting to an injected *interrupt line*, which is what the hardware
  evidence says the bits are).
* The register table in `devices.rs` and the `MachineConfig::keyport_*`
  docs carry the vendor naming; field names are unchanged (API churn
  without a behavior delta buys nothing).

### 34.2 The probe window: 0xBFFE3878/0x387C = PNG_PROBE_ADDR /
#### PNG_PROBE_VALUE — the Adler-32 readout

The vendor drivers' end-of-image branch reads the decoder's **Adler-32
checksum through a two-register probe window**: write
`PNG_PROBE_ADDR = 0`, read `PNG_PROBE_VALUE` (high half); write 1, read
(low half); combine `hi << 16 | lo`. §12's dispatcher table recorded
this exact shape without naming it: "0xBFFE387C — latch sampled by the
bit-4 handler (lw before/after a 0→1 write to 0xBFFE3878); two samples
combined `sample<<16 | sample`". The bit-4 handler is the end-of-image
branch; the "strobe" is the two probe addresses. The JPEG side of the
same vendor HAL uses its huff-probe the same way, commented "for hw
debug" — the probe windows are the vendor's hardware-debug access into
decoder internals.

What the Truman loader does with it: its end-of-image handler samples
the same two halves into gp state (§12). Whether the engine *accumulates*
Adler-32 over a `zlib_mode=1` raw decode (the vendor readout is shown
in PNG image mode, where the PNG stream carries an expected Adler
tail) is **unverified** — the drivers never read it in raw mode, and
the emulator therefore does not model an accumulator (modeling it would
change the dispatcher's gp state on speculation; §30.3's discipline).
The sysreg stub at +0x3878/+0x387C keeps its logged-stub semantics.

The same re-read names the table-RAM internal layout the upload
commands address (§13.6's recorded-but-uninterpreted uploads): length
symbols @0x000, length mincode @0x180, length valptr @0x190; distance
mincode @0x1A0, distance valptr @0x1B0, distance symbols @0x1C0; the
0xC000 control word is burst mode + write-enable. The engine's syms/
mincode ("first")/valptr ("cum") partition the 32-word internal SRAM
exactly as the loader's uploads land.

### 34.3 The continuation protocol — one more §16.1 confirmation

The kernel HAL's end-of-block branch documents how a multi-block
stream continues: read the consumption readout
(`PNG_BS_USED_CNT`/`PNG_BS_USED_BIT`, layout `[15:13]` DMA-count
extension / `[12:3]` byte / `[2:0]` bit), software-parse the *next*
block header straight from the byte stream, rewrite `PNG_BS_OFFSET`,
clear the end-of-block latch (0x200), re-assert start. The Truman
loader performs exactly one table upload and one start (§13.6), and
§16.1/§25 proved the SP container's 169,229 tail bits admit no
standard continuation — the vendor protocol names what a continuation
would *require* (a next dynamic-block header for this software flow to
parse), and none exists. The `sp_gzip` consumer-side interface is also
named now (`cmd_sp_png_dec.c` usage line: `./sp_gzip file_in file_out
u16_scramble_key`), including its production shape: a `ZlibBlockCnt`
loop decoding while NAND-streams the next block, LZ-backward buffer
32 KiB, scanline buffer 256 KiB — the emulator's 32 KiB zero-filled
band-window model (§13.4), corroborated vendor-side. The compressor
tool itself remains unpublished (§30.3); only its interface is now
documented.

### 34.4 The new instrument: a comparable output fingerprint

`machine::adler32` (RFC 1950, integer-only, pinned against zlib's own
published vectors: `""` → 1, `"abc"` → 0x024d0127, `"Wikipedia"` →
0x11E60398) plus one printed line in `dram_compare` post/ref reports —
the emulator-side fingerprint to hold against a probe-window read.
On the committed image the P1 reference fingerprint is **pinned**:
`adler32(reference) = 0xcd8d03bf` (79,872-byte engine decode,
`tests/hwsession.rs`). If the device's engine accumulates the checksum
in raw mode, the whole P3 experiment below collapses to comparing two
32-bit integers; if it does not, the pin still guards the decode output
against silent drift.

### 34.5 What this changes, and what it does not

The boot blocker is untouched: the 59 value-ROM constants stay
hardware-gated (§30.4). This session's survey re-derives the same
vendor sources (fresh download, byte-identical to the archive),
re-confirms nothing public uploads a value ladder in any vendor flow,
and adds no new public evidence beyond §30.1 — but it converts two
behaviorally-decoded registers into vendor-named ones, explains a
closed verdict (§23.3) structurally, and adds a second, much cheaper
capture option (P3, docs/hardware-session.md) to the session protocol.
128 tests, debug and release, all green; boot pins unchanged.

## 35. Milestone AH — the value-table file seam: the post-capture swap as
### data, and the P3 elimination oracle as a loop (v0.27.0)

### 35.1 What was added

Milestones AC (§31.4) and AG (§34.4) left the value-table swap one
recompile away: the recovered ladder would be a Rust constant edit, then
`cargo test`, then the instruments. This milestone makes the ladder a
**data file** and the elimination row a **loop body**, so that the
hardware session's post-capture work is commands only:

* `ValueTables::to_bytes` / `from_bytes` (src/inflate.rs) — a
  deterministic 191-byte container (`SPHEVTBL`, version 1, LE u16
  bases, u8 extra widths, exact 29/30 code counts). The container is a
  *carrier*, not a hypothesis: it admits every family the offline
  analyses bounded (§18.2 ladder search, §19.2 uniform extra widths
  0..=8, §26 joint sweep, RFC±per-code deltas) plus the m4 direct-value
  accounting (any base constant; all extras zero). Validation rejects
  exactly what the decoder cannot execute or no analyzed family
  occupies: wrong magic/version/length, wrong counts, extra widths
  above 13 (the widest the RFC distance ladder itself defines, codes
  28–29). Rejections are typed (`ValueTablesError`) and name what
  failed.
* `MachineConfig::value_tables` (src/machine.rs) — the engine decode
  path now takes the active ladder from the machine config instead of
  the `ENGINE_VALUE_TABLES` static. The default is field-identical to
  the static (both the RFC ladder; pinned by `tests/valtables.rs`), so
  every boot pin is unchanged by construction.
* `--value-tables <file>` on `dram_compare --post` (post mode only —
  pre/ref do not decode), on the CLI `map` command, and a new
  `examples/ladder_eval.rs` — the P3 elimination-row executor: boot the
  real firmware with a candidate ladder, print the §19.1 head bytes and
  the window's Adler-32, and with `--fingerprint 0xCAPTURED` exit
  ELIMINATED (1) or SURVIVES (0). Without the flag it is report-only;
  the shell loop is the pre-registered "re-run the swept families under
  the fingerprint oracle" (docs/hardware-session.md).

### 35.2 Verification without hardware

7 new tests (`tests/valtables.rs`): round-trips (RFC and an m4-style
candidate), the RFC container encoding pinned byte-for-byte via its own
Adler-32 (`0x01c80db5`) plus structural offsets, all six rejection
variants with exact typed errors, the config-default-equals-pinned-static
invariant, and two gated real-firmware tests: the **file-loaded RFC
ladder reproduces the pinned reference decode** (`0xcd8d03bf` — the
config path is byte-equivalent to the static path), and a **file-loaded
candidate ladder changes the decode output** deterministically — the
§31.4 parametricity, now end-to-end through `MachineConfig`. 135 tests,
debug and release, green; fmt/clippy clean; boot pins unchanged
(handoff at step 1,301,763, death at +0x50, head `60 81 00 00…`,
fingerprint `0xcd8d03bf`).

A fresh survey for new public evidence (sp_gzip, SPHE15xx SDK/PNG_DEC
sources, Truman dumps) found nothing beyond the §34.5 re-derivation:
the vendor compressor remains unpublished, no new source uploads the
value ladder, and the boot blocker remains exactly the 59 value-ROM
constants (§30.4). The milestone deliberately claims no boot progress:
the blocker is data-gated, and this work shortens the distance between
the capture and the verdict, not the capture itself.

### 35.3 The post-capture procedure, restated

```sh
# P1 with a recovered/real ladder (the §32.3 edit, as data):
dram_compare --post firmware/*.bin p1_pixel.bin --value-tables recovered.spvt
# the P3 elimination row, as a loop over candidate ladders:
for f in candidates/*.spvt; do
  ladder_eval firmware/*.bin --value-tables "$f" --fingerprint 0xCAPTURED \
    || echo "eliminated: $f"
done
# the menu-path map under the surviving ladder:
truman-emulator map firmware/*.bin --value-tables recovered.spvt
```

## 36. Milestone AI — the divergence→symbol map: the P1 solve, fed (v0.28.0)

The hardware-session decision tree pre-registers the P1 DIVERGENT row's
next action — "*first_divergence + block map become the decisive
constraint; feed the solve (the first divergent byte identifies the first
match whose value semantics differ)*" — but until this milestone the
identification itself was manual: `dram_compare` reports the byte offset,
and mapping it to the producing symbol required re-deriving the decode by
hand. The capture side of that row is now one command.

### 36.1 The instrument

Three pieces, all passive observers of the existing decode path:

* **The event trace** (`src/inflate.rs`): `inflate_engine_with_traced` —
  the engine decode with a per-symbol [`DecodeEvent`] record (index,
  output span, kind). Kinds: `Literal` (the stream byte),
  `EndOfBlock` (the §13 marker), and `Match` with the ladder's full
  contribution named (len_code/extra_w/extra_v/length,
  dist_code/extra_w/extra_v/dist). The plain
  `inflate_engine_with` now delegates to the same core with tracing
  off — the decode is byte-identical by construction (one code path),
  and the fingerprint pin (`0xcd8d03bf`) plus the boot pins re-verify it.
* **The replay capture** (`src/devices.rs`, `src/machine.rs`): at engine
  service time the machine stores the exact decode inputs
  (`last_stream` — post-"SP"-header, post input-length clamp — and
  `last_start_bit`) and the decode's own output length
  (`last_decoded_len`, pre-PNG-row semantics). `Machine::engine_decode_inputs()`
  returns them, so a post-boot replay is byte-exact regardless of later
  DRAM writes to the container region. The engine decode's preset/cap
  constants become named (`ENGINE_PRESET_BYTES`, `ENGINE_MAX_OUT`).
* **The attribution** (`src/machine.rs`): `attribute_divergence(events,
  decoded_len, reference, captured, k)` — a lookup, not a search: the
  event whose output span covers byte *k*, with the byte's position
  inside that span. Bytes at or beyond `decoded_len` are the zero-filled
  band region (§17) and report no event.

The executor is `examples/solve_divergence.rs` — boots the real
firmware, extracts the reference window, compares (the `dram_compare
--post` semantics), and on divergence replays the boot's decode with
tracing, **verifies the replay byte-identical against the boot window
before attributing** (integrity gate; a mismatch is a hard error, never
a report), then prints the attribution with the documented meaning per
kind:

* **divergence on a Literal** — value tables cannot move a literal
  (§19.3): under a symbol-identical parse the divergence is not a
  value-table difference; the parse itself differs. The report routes to
  the capture-point suspicion check first (the P1 tree's own order).
* **divergence inside a Match span** — the named match: its first
  `offset_in_event` output bytes are capture-identical (the
  first-divergence definition), so its referenced source bytes were
  correct for that prefix; the length value and the symbol sequence
  around it are the remaining movers. This is the pre-registered
  "first match whose value semantics differ".
* **divergence at/beyond `decoded_len`** — band fill (§17): a non-zero
  capture byte there is a length-accounting difference (the §26.4
  family), not a value difference.
* **size Δ with an identical prefix** — the P1 PREFIX-IDENTICAL row.

`--out <file>` writes the full event trace as TSV (the reference
decode's per-symbol map); `--value-tables` boots and replays under a
candidate ladder (§35). Exit codes: 0 identical, 1 divergent /
size-Δ (actionable), 2 usage/IO/integrity error.

### 36.2 What the reference decode's own trace pins (new structural data)

The trace of the RFC reference decode (deterministic, pinned by
`tests/solve.rs`):

| quantity | value |
|---|---|
| events (symbols decoded) | 25,031 |
| literals | 15,762 (63.0%) |
| matches | 9,267 (37.0%) |
| end-of-block markers | 2 |
| decode's own output length | 75,408 bytes |
| image (engine W×H) | 79,872 bytes |
| zero-filled band region | 4,464 bytes (5.6%) |

Two consequences. First, the **§19.1 head paradox now decomposes
precisely**: the executable head `60 81 00 00 …` is event #0 (literal
0x60), event #1 (literal 0x81), event #2 (match lc=259, len=5,
dist=193+30=223 into the 32 KiB zero preset → the five zeros). Under a
symbol-identical parse, head bytes 0–1 are fixed by the stream itself —
no ladder value can move them — while bytes 2–7 are one preset-reaching
match (ladder-movable). §19.3's "heads die on literals" is thus exact:
the first two bytes of the executed stage are stream literals.
Second, the decode's own length is 75,408, not 79,872: the machine's
PNG-row fill supplies the final 4,464 bytes (§17), so the §26.4
length-Δ family has a measured shape on this container.

### 36.3 Verification without hardware

7 new tests (`tests/solve.rs`, +7 = 142 total):

* pure attribution units on hand-built event lists: match-span
  attribution with offset, literal attribution (the §19.3 kind), band
  region → no event, zero-span (end-of-block) events never cover a
  byte, and the compared-prefix bound (`should_panic`);
* real-firmware gates: the traced replay is **byte-exact** against the
  boot's own decode output and the event spans **tile** that output
  exactly (contiguous, no gaps/overlaps, total = 75,408); the event
  census and the §19.1 head decomposition are pinned event-by-event;
  and the end-to-end P1 DIVERGENT row — a candidate-ladder boot as the
  synthetic capture — first diverges at byte 5, attributed to the head
  match (event #2, span 2..7, offset 3), with the identical prefix
  re-checked byte-for-byte.

The example is smoke-verified on all five verdict paths (identical /
prefix-identical / literal divergence / match divergence / band
divergence) with the pinned boot chain unchanged (handoff step
1,301,763, trap at 0xA1600050, fingerprint `0xcd8d03bf`). fmt/clippy
clean; 142 tests, debug and release, green.

**No semantics change**: the decode path is one shared core with a
passive observer; every boot pin and the fingerprint re-verify. The
milestone claims no boot progress — the blocker stays the 59 value-ROM
constants (§30.4), data-gated — but the capture's decisive constraint
is now a command output instead of an analysis session.

### 36.4 Fresh evidence survey

A fresh public-evidence sweep (SPHE15xx/PNG_DEC/sp_gzip sources and
SDK mirrors, Sunplus toolchains, Truman dumps) found nothing beyond
§34.5/§35.2: the vendor compressor remains unpublished, no source
uploads a value ladder, and the same-SoC second-container option
remains owner-gated and unexercised (§32.3).

## 37. Milestone AJ — the capture→ladder solver: the P1 solve, mechanical (v0.29.0)

The hardware-session decision tree's last manual step is now a command.
`solve::solve_ladder` (src/solve.rs) inverts one true decode output — a P1
capture's decode region — into the engine's value ladder, and
`examples/ladder_solve.rs` wraps it as the session's one-command solve:
boot, compare (the `dram_compare --post` semantics), route through the
pre-registered P1 tree (identical → the capture-point suspicion row;
non-zero band → the §26.4 length-Δ row), then solve, verify by the
engine's own re-decode, and write the recovered `SPHEVTBL` container
(verified solves only) for the §35 `--value-tables` seam.

### 37.1 The search frame is evidence, and it collapses the problem

The search's single structural assumption is §30.2's own reading of the
vendor's test container: **the RFC frame parses canonically** — the bit
accounting (block header, Huffman codes, extra-bit widths) is stream
data, and the silicon unknowns are exactly the 29+30 base values (the
§30.4 "59-constant" blocker). Two consequences the solver exploits:

* **The bit-level parse is base-independent.** Symbol positions depend
  only on the code lengths and the (now fixed) extra-bit widths, so the
  entire symbol sequence is a deterministic function of the stream. A
  wrong base can never move the parse — it can only fail a content
  check. The search is a sequence of per-code base commitments with
  deferred contradictions, not a joint structure search; a Huffman code
  that scans complete input without resolving (a `BadCode`) refutes the
  whole target, not a branch.
* **The decode's own output length is a linear equation in the bases.**
  `Σ count[k]·base[k] = target.len() − literals − Σ extra` (the counts
  and the extra values are fixed stream data — `SumGuard::new` walks the
  fixed parse once). Every base pin passes a global feasibility interval
  check against that equation, which is what makes the search linear in
  practice: on the pinned reference decode the solve commits every match
  on its first candidate (9,267 attempts for 9,267 matches).

Per match, the length `L` and distance `D` must satisfy the target's own
periodicity — `target[j+i] == src(j+i−D)` for all `i < L`, with `src(x)`
the zero preset for `x < 0` and `target[x]` otherwise — the engine's
live-window copy semantics (§13.6), made exact by the output invariant.
A code's first occurrence commits its base from the smallest
content-consistent value; later occurrences check, and a contradiction
backtracks to the enclosing choice (the rewind is the plain
innermost-frame order — the output position couples every earlier length
into each check, so conflict-directed backjumping has no sound blame
here; the sound global pruning is the sum constraint). Candidate order
puts the baseline (RFC) values first — the null hypothesis this repo is
framed against — then a spiral by Manhattan distance around that point,
with every candidate passing the same content predicate. Ordering is a
heuristic; coverage is bounded by the documented caps ([`SolveLimits`]:
length ≥ 1 (the engine model's own floor — RFC's ≥ 3 is a ladder
property, not a decoder one), ≤ 258; distance ≤ 32768; the per-
occurrence distance scan; the spiral radius; the attempt budget) and a
cap or budget hit is a typed error (`SolveError`), never a silent
partial answer.

The final tables re-decode the stream through the real engine path
(`inflate_engine_with_traced`), and that byte-comparison is the reported
verdict (`verified`): the solver reports, and the harness writes, only
ladders the engine itself reproduces the capture with. Codes the stream
never exercises (or exercises only at the input-exhaustion boundary,
where the copy never runs) keep the baseline values and are reported as
`CodeStatus::Unconstrained` — they cannot have moved this decode.

### 37.2 What the reference decode's own solve pins (new structural data)

Solving the pinned reference decode from its own output (the RFC truth)
recovers the ladder exactly: **57/57 exercised codes pinned, all agreeing
with the RFC values byte-for-byte**, 2 distance codes unconstrained (the
stream never exercises them), in 9,267 candidate attempts (~16 ms
release, one attempt per match). The event census re-derives §36.2's
pins exactly (25,031 events, 9,267 matches, 75,408 bytes, the
`0xcd8d03bf` fingerprint after band fill). This is the solve's
self-test: the machinery that will recover the hardware ladder from a
P1 capture demonstrably recovers a known ladder from a known output.

The envelope is stated honestly: a capture whose true ladder sits at or
near the baseline solves as linearly as the self-test (the baseline
candidate commits first at every match); captures requiring larger
deviations spend the budget on the deferred-contradiction search and
fail typed (`NoSolution`/`BudgetExhausted`) with the caps as knobs. The
P3 fingerprint experiment (§34.4) is the session's cheap first read: a
matching fingerprint means the ladder IS the baseline and no solve is
needed; a differing one feeds the solve, and a budget failure there is
itself recorded information (the deviation is outside the enumerated
family — the §36 attribution names where).

### 37.3 Verification without hardware

9 new tests (`tests/solve_ladder.rs`): the synthetic bases-only ladder
hand-encoded at the RFC extra-bit widths is recovered from its own
output (verified re-decode, the width frame pinned RFC by construction,
deterministic across solves); an impossible (corrupted) target is typed
`NoSolution`; a zero attempt budget is typed `BudgetExhausted`, never a
wrong answer; and the real-firmware gates above (the reference solve,
the census, the fingerprint, the exact-RFC agreement, the machine-path
replay). The container round-trips through `ValueTables::to_bytes`, so a
solved ladder is data for the §35 seam without a recompile. 146 tests,
debug and release, green; fmt/clippy clean; boot pins unchanged
(handoff step 1,301,763, trap at 0xA1600050, fingerprint `0xcd8d03bf`).

**No emulator-semantics change**: the decode path is untouched (the
solver shares its parse via the same code); the pixel_dump harness gains
`--value-tables` (a candidate-ladder boot dump — the capture generator
the session's own smoke test used). The milestone claims no boot
progress — the blocker stays the 59 value-ROM constants (§30.4),
data-gated — but the capture→verdict distance is now zero commands
deep: capture, solve, swap, re-boot.

## 38. Milestone AK — the phantom trigger defect, disproven; the fresh-evidence survey (v0.29.1)

### 38.1 A false defect, diagnosed and refuted byte-level

This session's terminal toolchain intermittently ate the two-character
sequence that opens a bracketed lowercase-`m` token: contiguous-text
views (cat, `cat -A`, `git show`, grep, python `repr`) of
`.github/workflows/ci.yml` rendered the trigger line as
`branches: ain]`, while views whose output cannot carry the pair (the
Read tool, `od -c` column output) showed the true content. The corrupted
views produced a coherent but wrong diagnosis — that the push trigger is
the scalar `"ain]"`, which matches no branch, so push-triggered CI never
fired. The repo's own evidence rule (a plausible story is not evidence,
§30.3) settles it with bytes:

| blob | epoch | trigger-line bytes (decimal) | decode |
|---|---|---|---|
| `7cb66eb7` | Milestone M (creation) | `91 109 97 105 110 93` | `[main]` |
| `220819c8` | Milestone AF → origin/main → HEAD | `91 109 97 105 110 93` | `[main]` |
| worktree (`git hash-object`) | this session | same SHA `220819c8…` | unchanged |

The trigger was `branches: [main]` from day one; the AF commit changed
only the header comment and the gate list (1191 → 1420 bytes) — the
diffstat of a one-comment rewrite, and push CI has been firing since
Milestone M. No repo defect exists; nothing was fixed. The incident is
recorded because the failure mode is dangerous for an evidence-first
repo: the corrupted reads were consistent across four independent
shell-level tools, and acting on them would have produced a phantom
"repair" commit and a false docs claim. The reliable channels were blob
SHAs (`git rev-parse <commit>:<path>`), per-byte decimal dumps, and
hash-object equality between the worktree and origin — none of which
can carry the eaten pair.

### 38.2 The fresh-evidence survey (2026-10-25)

Both upstream sources of the archived vendor references were re-fetched
and compared byte-for-byte against `tools/reference/`:

| archived file | upstream | result | SHA-256 (both sides) |
|---|---|---|---|
| `sphe15xx-regfile.h` | Team-Proton/linux-sunplus-15xx `master`, raw blob, `linux-4.4.7/arch/mips/include/asm/mach-sphe15xx/regfile.h` | identical | `1572efc2e8b23299658a42f3a354e43bd6ba73d2676fb748431acd5a55b42b9a` |
| `sphe-sp_imgdec.c` | vicyap/sony-xav-ax100 `master` (5682442), blobless clone + `git cat-file`, `sony-xav-ax100/uboot/arch/arm/cpu/sphe-common/sp_imgdec.c` | identical | `eaa0d0b2d662d2551083e820895577bf704ccab48b50467c1e1089086ba14de7` |
| `sphe-sp_imgdec.h` | vicyap/sony-xav-ax100 `master`, same method | identical | `a15b0d45fe87d0b388728a5135ed8f8b283c2312c47fa9cb2cf079ce8507c4de` |
| `sphe-cmd_sp_png_dec.c` | vicyap/sony-xav-ax100 `master`, same method | identical | `23d9cdb70582074f64d7c140831ae36cb8aa0dab78120f82ee11ee35e468bf12` |

Method notes, for reproducibility: the raw-blob fetch of `regfile.h`
required no API quota; the vicyap tree nests one level deeper than the
`tools/reference/README.md` table implies (the repo root contains a
single `sony-xav-ax100/` directory), so the three U-Boot files were read
from the commit object of a `--depth 1 --filter=blob:none` clone rather
than guessed at over HTTP. The driver's own text confirms the §30.4
blocker reading once more: the "length/distance group base" table-RAM
writes at `sp_imgdec.c` +0x190/+0x1b0 are the Huffman `valptr[1..15]`
arrays (canonical-decode bookkeeping), not the LZ value bases — the
29+30 silicon constants the vendor never publishes in any reachable
source. No new SPHE15xx PNG_DEC driver, no `sp_gzip` release, no other
U-Boot dump surfaced. The blocker stays hardware-gated (§22.5, §26.4,
§30.4); `docs/hardware-session.md` remains the single unlock, and the
capture→solve→swap→re-boot chain (§37) is its full offline preparation.

## 39. Milestone AL — the CI record observed green; the second survey (v0.29.2, session record)

### 39.1 The gate battery, remotely observed

The GitHub Actions record for this repository was read via the API on
2026-10-25 (authenticated read-only): **26 workflow runs in total, the
first at 2026-10-23T22:13Z, every one concluded `success` — zero
failures**. This covers every push since CI's creation in Milestone M
through v0.29.1 (`112199e`, run 40159344860). Two record-keeping
consequences:

1. §38.1's byte-level conclusion about the trigger ("push CI has been
   firing since Milestone M") is confirmed by direct observation — the
   inference and the observation agree.
2. Each run executes the full battery (fmt, clippy 0-warnings across
   targets/features, `check --all-targets`, and the test suites in both
   debug and release), so the per-milestone gate claims recorded in
   this document carry a remote green data point, not only a local one.

### 39.2 The second survey (2026-10-25, later in the day)

No new SPHE15xx PNG_DEC sources, no `sp_gzip` publication, no
value-ladder constants anywhere reachable. The Truman-related hits are
third-party firmware distribution sites; the model variants listed
there (e.g. X1 PLUS) are not this device (X1 WiFi), and third-party
firmware downloads are outside this project's constraints in any case.
The §30.4 blocker stands: the 59 value-ROM constants are hardware-gated
(§22.5, §26.4), the capture→solve→swap→re-boot chain (§37) remains the
single unlock, and the hardware-session protocol (§27) is unchanged.

## 40. Milestone AM — the vendor vector's structural leak: the pattern backbone, the joint anchor system, and the post-capture vector gate (v0.30.0)

The SDK's own test container (`cmd_sp_png_dec.c`, `ZlibBuf`, 3,744 bytes,
scramble key `0xa6fc`, 9 chunks → 9,216 bytes; archived as
`tools/reference/sphe-cmd_sp_png_dec.c` and first analyzed in §30.2) is
**vendor-authored `sp_gzip` output for the same hardware value tables**
as the Truman stream. §30.2 recorded three facts from it (non-RFC
distances, clean literals, legitimate zero-preset references) and §30.3
used it to demonstrate the m4 accounting collapse — but the blind m4
search "without knowing the true pattern" (§30.3) never asked the
question this milestone asks: **how much of the vector's true 9,216-byte
pattern does the vector itself determine?** The answer, established
byte-by-byte below, is: more than any session has used so far — enough
to make the vector a permanent second oracle, though not enough to
replace the hardware session. Tool: `tools/fw_vector_probe.py`
(deterministic, offline, prints every number in this section).

### 40.1 The parse is clean, the block never ends: row-count completion

The dynamic header parses canonically at stream bit 0
(`bfinal=1, hlit=286, hdist=26, header_bits=304`), and the decoded
code-length alphabet is **exactly the observed output alphabet** —
5-bit codes for the sixteen hex characters, 9–11-bit codes for `\n`,
`\r`, NUL, and the length codes — a strong internal consistency check
(a wrong header read could not produce an alphabet so precisely matched
to the readable text that follows it).

The structural surprise: **no start bit makes the parse reach the
end-of-block symbol.** A 1-bit scan over `[304, 4096)` with the canonical
tables — every bit position, not just the aligned candidates — produces
**zero** walks that decode symbol 256 before input exhaustion
(`tools/fw_vector_probe.py` block [1]). Every walk instead runs cleanly
to input end (no `ERR_LIT`, no `ERR_DIST`), producing 1,400–1,439
matches. For the Truman stream the same walk reaches EOB twice (§36.4);
for the vector it never does, at any start bit.

The resolution is the vendor driver's own completion discipline
(`cmd_sp_png_dec.c`): the engine's completion is the **row-count
accounting** (§13.6 — `PNG_PXL_LINE = height−1`, one row event per
1,024 output bytes), so a 9-row pattern completes at byte 9,216 **whether
or not the block's EOB has been consumed**. The vector's block simply
never needs its EOB: the decode ends by accounting, not by symbol. (The
Truman stream's loader is the same lineage but its image completes via
the same row count — the two EOBs there are real block boundaries, a
different container structure.) Corollary: the no-EOB fact is itself
evidence that the canonical tables are not the hardware's tables — under
the true tables the parse differs — consistent with every §14–§30
refutation of RFC-family semantics.

### 40.2 The literal backbone: the pattern's text is in the stream

The engine-semantics decode (32 KiB zero preset, §13.1/§18.3) completes
at every candidate start bit — `ceil32(header)`=320, the DMA-window
1,024, and the vendor `PNG_BS_OFFSET` 1,088 — with **zero invalid
distances** under the preset (§30.2's "invalid distance too far back"
happens only under the empty RFC window). The output at each start bit
is the same *kind* of object: hex characters separated by 1–16-byte
zero runs, 19 `\n` + 9 `\r`, no other characters — and it over-runs the
9,216 anchor by **+570 / +377 / +358** bytes respectively.

The decisive observation is *where the text sits*: the output's
literal-produced fragments, read in event order, are themselves fluent
hex text — `c5f43df6d00e293648bc70cfe93488e7afc966ba…` — and the
match-produced zero runs sit **between** backbone fragments, never
inside one. The literal census makes this quantitative: of the vector's
1,535–1,577 literals, **1,523–1,565 are hex characters (≈99.2%)**, only
4 are NUL, 6 `\n`, 2 `\r`. The pattern's text content is therefore
**in the stream as literals**: the true pattern is the backbone plus
whatever the matches produce between backbone characters — under RFC
semantics those matches mostly hit the zero preset (the gaps), which is
why the RFC output reads as "sparse hex-text in a zero sea" (§30.2)
while remaining robust across start bits. The backbone pins the pattern
at ≈17% of its bytes outright, in order, at stream-derived values that
no ladder can change (literals are ladder-unreachable, §19.3 — here
that ceases to be a paradox and becomes the leak).

### 40.3 The joint two-stream length-anchor system

The length side of the ladder is now constrained by **two streams at
once**. With `d_c = base_true(c) − LBASE[c]` for the 29 length codes and
the per-code census `n(c)` of each stream's matches (the bit-level parse
is base-independent, §37), the anchors are linear:

* **Truman (equality, pinned):** the Rust engine decode produces 75,408
  bytes under RFC bases and must produce 79,872 (78×1,024, §17/§18.1):
  `Σ_c n_t(c)·d_c = +4,464`. Census cross-check: the Python walk gives
  75,398 (the ±10 is the zero-window tail convention, §36.4), with
  9,264 matches to the Rust engine's 9,267.
* **Vector (conditional equality):** under the no-waste prior — the
  compressor emits the block that decodes to exactly its 9,216-byte
  pattern, the engine stops at the 9th row — `Σ_c n_v(c)·d_c =
  9,216 − len_rfc(start)` = **−570 / −377 / −358** for the three
  candidate start bits. The vector's weights are overwhelmingly
  concentrated on the short-length class: `[1422, 15, 0, 0, 0, 2]`
  (classes by extra-bit width, E0 = codes 0–7 + 28) against Truman's
  `[8410, 415, 272, 131, 25, 11]`.

**The signs are opposite on the same dominant class.** The first
quantitative consequence independent of any hardware: under the vector's
anchor the bulk short-length class cannot grow (δ_E0 ≈ −0.4 on the
vector's 1,422 E0 matches) while Truman's anchor requires +4,464 net
across a census dominated by the same class — the true ladder cannot be
RFC-uniform *or* per-extra-bit-class uniform. The joint sweep over the
entire per-class uniform family (integer deltas bounded by base
validity `base ≥ 3`, δ ≤ +8, 2 equations / 6 unknowns solved exactly
over the 4 free dimensions) returns **zero survivors at all three
candidate start bits**. If a ladder satisfies both anchors, its deltas
are per-code structured: shorter codes at-or-below RFC, longer codes
above.

Registered caveat (the §30.3 discipline applied to ourselves): the
vector anchor is conditional on the no-waste reading of §40.1. If the
engine over-decodes into the scanline buffer after the 9th row (256 KiB
reserved, `cmd_sp_png_dec.c`), the vector's equation weakens to an
inequality `Σ_c n_v(c)·d_c ≥ −570`, and the per-class family survives
under Truman alone (δ_E0 ≈ +0.53 → vector Σ ≈ +757 ≥ −570). The
refutation above is therefore stated as conditional; the structural
conclusion (opposite signs) is what survives unconditionally, because
it follows from the two RFC deviations themselves (+350…+570 over-run
vs the −4,464 under-run) whenever the vector's true length is near its
9,216 anchor.

### 40.4 What this changes: the vector gate, and what it does not

**The post-capture vector gate (new, one command).** When the hardware
session's `ladder_solve` (§37) recovers a value ladder from the P1
capture, the ladder must also re-decode the **vector**: `sp_gzip`
compressed the vendor's own 9,216-byte pattern against the same ROM
tables, so the recovered ladder's vector decode must be
  (a) exactly the pattern size the container declares (9 chunks →
  9,216 bytes) within the §40.3 accounting, and
  (b) clean sparse hex text — only `[0-9a-f]`, `\n`, `\r`, NUL — with
  the pinned literal backbone in order (§40.2's census is the reference:
  1,535–1,577 literals, ≥99% hex).
A recovered ladder that fails the vector gate is wrong regardless of how
well it fits P1 — the vector is a second, vendor-authored constraint the
hardware session gets for free. `tools/fw_vector_probe.py` block [2/3]
is the reference implementation of the check's data side; the Rust-side
runner is a natural follow-up once a recovered `SPHEVTBL` exists (the
§35 seam already carries arbitrary tables through the engine model).

**What this does not change.** The vector alone still does not
determine the ladder offline: the §30.3 underdetermination extends to
it (the text oracle is satisfied by many ladders — the RFC ladder
itself already produces clean text at every start bit), the distance
side (30 bases) gains no equation from the size anchors, and no public
source publishes `sp_gzip` or the pattern's generator (survey re-run
this session; §39.2's conclusion stands). The §30.4 blocker remains
hardware-gated; the hardware-session protocol (§27) is unchanged except
that the post-capture procedure gains the vector gate as a mandatory
second validation, and the §37 solver's candidate ordering can exploit
§40.3's derived structure (short codes at-or-below RFC, long codes
above) as its baseline prior.

## 41. Milestone AN — the vector gate as code: the solve pipeline enforces the vendor's second validation (v0.31.0)

### 41.1 What was built

Milestone AM left the §40.4 vector gate as documentation plus a Python
reference implementation (`tools/fw_vector_probe.py`). Milestone AN turns
it into library code the solver executes. The new module
`src/vendor_vector.rs` extracts the vector container from the archived
vendor source (`tools/reference/sphe-cmd_sp_png_dec.c`, the
`ZlibBuf[16*234]` array), validates it against its own accounting (SP
magic, chunk count 9, declared stream length 0xe98 = 3,736, array size
3,744 — a corrupted or substituted reference is named and rejected before
any verdict could be computed from it), and re-decodes the vector under
candidate value tables with the engine's own semantics
(`inflate_engine_with_traced`, 32 KiB zero preset, decode to input end)
at the three §40 candidate start bits (320/1024/1088).

The gate is enforced where the ladder is born: `ladder_solve` now runs it
automatically on every verified solve, before `--out` is written. A
verified-but-gate-failing solve is reported (per-start-bit lines with
decode length, anchor delta, literal census, alphabet violations) and NOT
written — exit 1, the same "report is the actionable data" contract as an
unsolved divergence. A missing container is an environment error (exit
2), never a verdict. No skip flag exists: the gate is the protocol
(docs/hardware-session.md), and AM's wording stands mechanically — a
ladder that fits P1 but fails the vector gate is wrong.

### 41.2 The dual pin: Rust against the Python reference

The new test suite (`tests/vendor_vector.rs`, 6 tests; crate total 152,
debug and release) pins the §40 vector-side numbers in Rust, cross-checked
against the Python reference implementation's output this session:
container 3,744 bytes (head `53 50 00 09 00 00 0e 98`), chunks 9,
stream_len 0xe98; RFC decode completes at every candidate start bit with
zero invalid distances; literal census 1,577/1,538/1,535 (starts
320/1024/1088), matches 1,439/1,403/1,400; anchor over-runs exactly
+570/+377/+358; alphabet clean and backbone ≥990‰ hex at every start bit;
and the RFC ladder FAILS the gate overall — the accounting never hits the
9,216 anchor, which is precisely the gate's discriminating edge.

### 41.3 The frame/value split the implementation surfaced

The value-only test (RFC base values shifted +1, extra-bit widths
untouched) confirms §19.3 at fixed frame: the literal census is
byte-identical to RFC's while the accounting moves. The width-family test
(all widths zeroed) measures the converse: the stream RE-ALIGNS — 2,350
literals vs the RFC 1,538 at start 1024. §19.3's "literals are
stream-determined, ladder-independent" is therefore a fixed-frame
statement: the widths themselves are stream-position data. The gate's
conditions split accordingly — the alphabet and backbone checks fail fast
under any width-touching candidate and pin the container/parse/impl
otherwise, while within the value family the accounting alone
discriminates. Both routes end in a gate failure for a wrong ladder.

### 41.4 What this does not change

The §30.3 underdetermination extends to the vector unchanged: the text
oracle is satisfied by many ladders (the RFC ladder itself produces clean
text at every start bit — now pinned in Rust as well as Python), and the
distance side gains no equation from the size anchors. The §40.3 joint
anchor sweep remains the Python reference's job (per-class uniform
families, zero joint survivors); `fw_vector_probe.py` stays in the repo
as the independent cross-check. The §30.4 blocker stays hardware-gated;
`docs/hardware-session.md` remains the single unlock. Boot pins unchanged
(handoff step 1,301,763, trap 0xA1600050, fingerprint 0xcd8d03bf); no
emulator-semantics change; fmt clean, clippy 0 warnings,
`check --all-targets` clean, 152 tests debug and release.

### 41.5 Fresh-evidence survey (2026-10-25, fourth pass)

No new SPHE15xx PNG_DEC sources; Team-Proton/linux-sunplus-15xx remains
dormant (no new commits or issues); the sunplus-plus1 organization's
`sp_buildroot` is the unrelated SP7021 line; ST's `x-linux-spn1` is the
unrelated SPN1 mission board; no `sp_gzip` publication, no value-ladder
constants, no hardware-session reports from other owners. Truman
third-party firmware distribution sites remain out of scope (other model
variants; X1 PLUS is not X1 WiFi). The §30.4 blocker stays hardware-gated.

## 42. Milestone AO — the §40.3 class prior as the solver's ordering (v0.32.0)

### 42.1 What was built

AM closed with a declared follow-up: "the §37 solver's candidate ordering
can exploit §40.3's derived structure (short codes at-or-below RFC, long
codes above) as its baseline prior." Milestone AO implements exactly
that. `solve::length_class(li)` ports the reference's `class_of`
partition (class 0 = codes 257..=264 plus the fixed-258 code 285;
classes 1..=5 = the equal-extra-width groups), and
`SolveLimits::class_prior` re-orders the length-axis spiral: class-0
codes keep below-the-baseline-first, longer classes try above first.
The default stays the symmetric order; `ladder_solve` enables the prior
(the hardware-session solve runs prior-ordered).

The prior is an ORDERING, never a constraint: the searched family, the
per-candidate content predicate, the documented caps and the
attempt budget are identical either way, and the verdict is in every
case the engine's own verified re-decode. That boundary matters — the
§40.3 derivation kills the per-class-UNIFORM family and fixes the
opposite-sign structure, but it does not prove that every non-uniform
assignment has short ≤ RFC (a split within the short classes plus long
above would satisfy the same anchors), so pruning by the structure
would over-claim the evidence. Ordering encodes exactly what the
evidence supports: a direction.

### 42.2 The invariance pin

`tests/solve_ladder.rs` grows three tests (crate total 155, debug and
release). The class-boundary test pins `length_class` against the
reference partition. The invariance test builds a stream whose true
ladder has exactly the §40.3 structure — short base 5 → 4, long base
131 → 139 — and whose content makes that solution UNIQUE (any wrong
length drifts the output position so the trailing literal check dies:
the copy source is a varied 6-byte pattern that never contains 0x00).
Both orderings recover byte-identical verified tables equal to the
truth, and a second prior-ordered run is deterministic. The
ambiguous-content test replays the recovery suite's own periodic stream
(multiple verified assignments, the AJ fixture) under the prior: the
solve still verifies and its re-decode equals the capture — the
contract is untouched.

### 42.3 The measured economics, honestly

On the pinned unambiguous stream both orders examine the identical
candidate set (87,034) — the prior is directional alignment with the
evidence, not a search-size reduction. This is the honest reading of
what a re-ordering can do: the spiral is exhausted radius-by-radius,
and a truth at radius 8 requires reaching radius 8 under any order;
the wrong candidates inside radii 1..7 are the same set. The prior's
decision-relevant effect is therefore on AMBIGUOUS content — the real
capture is 78 KB of highly periodic pixel rows, where multiple verified
assignments exist and the solver reports the first-found one. There the
order decides which verified answer is reported: prior-ordered, the
first-found answer is biased toward the §40.3 structure instead of the
below-biased default. The `prior.nodes ≤ neutral.nodes` bound is
pinned in the test; equal counts are the expected case on unambiguous
content, and the alignment can only remove the final below-candidate
attempt when the truth is above at the outermost radius.

### 42.4 What this does not change

The §30.3 discipline is intact: coverage is bounded by the documented
caps and the budget, never by the heuristic; unconstrained codes stay
at the baseline and are reported `Unconstrained`; an unsolved solve is
a typed failure with the attribution as the actionable data. The
§30.4 blocker stays hardware-gated; the capture→solve→swap→re-boot
pipeline is unchanged except that its solve now runs evidence-ordered.
Boot pins unchanged (handoff step 1,301,763, trap 0xA1600050,
fingerprint 0xcd8d03bf); no emulator-semantics change; fmt clean,
clippy 0 warnings, `check --all-targets` clean, 155 tests debug and
release.

### 42.5 Fresh-evidence survey (2026-10-25, fifth pass)

No new SPHE15xx PNG_DEC sources or value-ladder constants. The
prominent PNG-decoder hits (PNGdec, Wuffs, Blend2D's codec) are
software decoders — irrelevant to the hardware engine's silicon
constants; the sunplus-plus1 (SP7021) and C3V (SP7350) ecosystems
remain the unrelated product lines; the 1506T/1506c receiver hits are
the same out-of-scope third-party firmware ecosystem. No `sp_gzip`
publication, no hardware-session reports from other owners. The
§30.4 blocker stays hardware-gated.
