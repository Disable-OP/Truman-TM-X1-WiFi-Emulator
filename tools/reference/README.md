# Reference sources (external, attributed)

Third-party GPL sources used as documentation for the SPHE15xx platform's
PNG_DEC hardware block. They are NOT part of this project's build and are
committed only as evidence for `docs/firmware-analysis.md` §30.

| file | origin | license |
|---|---|---|
| `sphe15xx-regfile.h` | Team-Proton/linux-sunplus-15xx, `linux-4.4.7/arch/mips/include/asm/mach-sphe15xx/regfile.h` | GPL-2.0 (Linux kernel) |
| `sphe-sp_imgdec.c` | vicyap/sony-xav-ax100, `sony-xav-ax100/uboot/arch/arm/cpu/sphe-common/sp_imgdec.c` | GPL-2.0+ (Sunplus U-Boot, (C) Sunplus Technology 2016) |
| `sphe-sp_imgdec.h` | vicyap/sony-xav-ax100, `sony-xav-ax100/uboot/arch/arm/cpu/sphe-common/sp_imgdec.h` | GPL-2.0+ |
| `sphe-cmd_sp_png_dec.c` | vicyap/sony-xav-ax100, `sony-xav-ax100/uboot/arch/arm/cpu/sphe-common/cmd_sp_png_dec.c` | GPL-2.0+ |

Why they matter: `sp_imgdec.c` is the vendor's own PNG_DEC programming
sequence and names every register constant the Truman loader writes
(`docs/firmware-analysis.md` §30), including the "ZLIB/Scrambler" container
format our "SP" magic comes from, `zlib_mode` (raw DEFLATE, no PNG
filters), `IMG_TYPE = (color_type<<5)|bit_depth` (0x68 = palette/8bpp
passthrough), and the scramble-key slot. `cmd_sp_png_dec.c` embeds the
SDK's own test container (known scramble key 0xa6fc) used as a fitting
target in §30.
