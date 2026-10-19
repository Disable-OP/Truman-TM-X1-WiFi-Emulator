//! Firmware image identification, validation and loading.
//!
//! The target artifact is a raw 4 MiB SPI NOR flash dump (see
//! docs/firmware-analysis.md §1/§4). Identification is structural: size,
//! prologue words, and the loader banner strings at their verified offsets.
//! The original image is never modified — it is loaded read-only.

use std::path::{Path, PathBuf};

/// Verified layout constants of the Truman flash image
/// (docs/firmware-analysis.md §4/§5).
pub mod layout {
    /// Total image size: 4 MiB SPI NOR dump.
    pub const IMAGE_SIZE: u32 = 0x0040_0000;
    /// Stage-1 loader ("zeroBoot") offset.
    pub const STAGE1_OFFSET: u32 = 0x0000_0000;
    /// Stage-2 loader ("Boot form SPI") offset.
    pub const STAGE2_OFFSET: u32 = 0x0000_8000;
    /// Offset of the "zeroBoot" banner string.
    pub const STAGE1_BANNER_OFFSET: u32 = 0x0000_0446;
    /// Offset of the "Boot form SPI" banner string.
    pub const STAGE2_BANNER_OFFSET: u32 = 0x0000_9016;
    /// Offset of the `SPHE15` SoC descriptor string.
    pub const SPHE_DESC_OFFSET: u32 = 0x0038_B000;
    /// First prologue word at offset 0 (verified: `addiu $zero, $zero, 0x6fb5`
    /// — used as a structural marker by the BootROM-adjacent header).
    pub const PROLOGUE_WORD0: u32 = 0x2400_6FB5;
    /// Second prologue word (verified: `lui $k0, 0x0819`).
    pub const PROLOGUE_WORD1: u32 = 0x3C1A_0819;
}

/// A loaded firmware image (owned bytes; the file on disk is untouched).
#[derive(Debug, Clone)]
pub struct Image {
    /// Original file path.
    pub path: PathBuf,
    /// Raw image bytes.
    pub bytes: Vec<u8>,
    /// SHA-256 of the bytes (hex).
    pub sha256: String,
}

/// Errors produced during firmware loading/validation.
#[derive(Debug)]
pub enum ImageError {
    /// The file could not be read.
    Io(std::io::Error),
    /// The file is structurally not a supported image.
    NotSupported { path: PathBuf, reason: String },
    /// The file is recognizable but fails validation.
    Invalid { path: PathBuf, reason: String },
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::Io(e) => write!(f, "cannot read firmware: {e}"),
            ImageError::NotSupported { path, reason } => {
                write!(f, "unsupported firmware {}: {}", path.display(), reason)
            }
            ImageError::Invalid { path, reason } => {
                write!(f, "invalid firmware {}: {}", path.display(), reason)
            }
        }
    }
}

impl std::error::Error for ImageError {}

fn sha256_hex(data: &[u8]) -> String {
    // Minimal SHA-256 implementation (FIPS 180-4). Zero-dependency by design;
    // validated against the known digest of the committed image in tests.
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bitlen = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());

    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// Read a firmware image from disk (read-only; file contents are copied).
pub fn load(path: &Path) -> Result<Image, ImageError> {
    let bytes = std::fs::read(path).map_err(ImageError::Io)?;
    let sha256 = sha256_hex(&bytes);
    Ok(Image {
        path: path.to_path_buf(),
        bytes,
        sha256,
    })
}

/// Structural identification result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identification {
    /// Detected image kind.
    pub kind: &'static str,
    /// Detected SoC family.
    pub soc: &'static str,
    /// Human-readable notes.
    pub notes: Vec<String>,
}

/// Identify a raw image using structural evidence.
///
/// Checks (docs/firmware-analysis.md):
/// 1. exact 4 MiB size,
/// 2. verified prologue words at offset 0,
/// 3. loader banner strings at their verified offsets,
/// 4. `SPHE15` descriptor string.
pub fn identify(image: &Image) -> Result<Identification, ImageError> {
    let b = &image.bytes;
    if b.len() != layout::IMAGE_SIZE as usize {
        return Err(ImageError::NotSupported {
            path: image.path.clone(),
            reason: format!(
                "expected {} bytes (4 MiB SPI dump), got {}",
                layout::IMAGE_SIZE,
                b.len()
            ),
        });
    }
    let rd32 = |off: u32| -> u32 {
        u32::from_le_bytes([
            b[off as usize],
            b[off as usize + 1],
            b[off as usize + 2],
            b[off as usize + 3],
        ])
    };
    if rd32(0) != layout::PROLOGUE_WORD0 || rd32(4) != layout::PROLOGUE_WORD1 {
        return Err(ImageError::NotSupported {
            path: image.path.clone(),
            reason: format!(
                "prologue words 0x{:08x}/0x{:08x} do not match the verified boot prologue",
                rd32(0),
                rd32(4)
            ),
        });
    }

    let mut notes = Vec::new();
    let stage1 = b"zeroBoot v1.0 for 1506F";
    let stage2 = b"Boot form SPI v1.13";
    let sphe = b"SPHE15";

    let has1 = starts_at(b, layout::STAGE1_BANNER_OFFSET, stage1);
    let has2 = starts_at(b, layout::STAGE2_BANNER_OFFSET, stage2);
    let has3 = starts_at(b, layout::SPHE_DESC_OFFSET, sphe);

    if !has1 || !has2 || !has3 {
        return Err(ImageError::Invalid {
            path: image.path.clone(),
            reason: format!(
                "loader banners missing (zeroBoot@0x446={}, boot-spi@0x9016={}, SPHE15@0x38b000={})",
                has1, has2, has3
            ),
        });
    }
    notes.push("stage-1 loader: zeroBoot v1.0 for 1506F (flash 0x0000)".to_string());
    notes.push("stage-2 loader: Boot form SPI v1.13 (flash 0x8000)".to_string());
    notes.push("SoC descriptor: SPHE15 (flash 0x38b000)".to_string());

    Ok(Identification {
        kind: "raw SPI NOR flash dump (4 MiB, full image)",
        soc: "Sunplus SPHE15xx (probable SPHE1506HV)",
        notes,
    })
}

fn starts_at(hay: &[u8], off: u32, needle: &[u8]) -> bool {
    let off = off as usize;
    hay.len() >= off + needle.len() && &hay[off..off + needle.len()] == needle
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_image() -> Vec<u8> {
        let mut v = vec![0xFF_u8; layout::IMAGE_SIZE as usize];
        // prologue words
        v[0..4].copy_from_slice(&layout::PROLOGUE_WORD0.to_le_bytes());
        v[4..8].copy_from_slice(&layout::PROLOGUE_WORD1.to_le_bytes());
        // banners
        let off1 = layout::STAGE1_BANNER_OFFSET as usize;
        v[off1..off1 + 23].copy_from_slice(b"zeroBoot v1.0 for 1506F");
        let off2 = layout::STAGE2_BANNER_OFFSET as usize;
        v[off2..off2 + 19].copy_from_slice(b"Boot form SPI v1.13");
        let off3 = layout::SPHE_DESC_OFFSET as usize;
        v[off3..off3 + 6].copy_from_slice(b"SPHE15");
        v
    }

    #[test]
    fn sha256_known_answer() {
        // FIPS 180-4 test vectors
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn identify_valid_synthetic_image() {
        let img = Image {
            path: PathBuf::from("synthetic.bin"),
            bytes: make_test_image(),
            sha256: "0".repeat(64),
        };
        let id = identify(&img).unwrap();
        assert_eq!(id.kind, "raw SPI NOR flash dump (4 MiB, full image)");
        assert!(id.notes.len() >= 3);
    }

    #[test]
    fn reject_wrong_size() {
        let img = Image {
            path: PathBuf::from("small.bin"),
            bytes: vec![0u8; 1024],
            sha256: "0".repeat(64),
        };
        assert!(matches!(
            identify(&img),
            Err(ImageError::NotSupported { .. })
        ));
    }

    #[test]
    fn reject_wrong_prologue() {
        let mut v = vec![0xFF_u8; layout::IMAGE_SIZE as usize];
        v[0..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        let img = Image {
            path: PathBuf::from("bad.bin"),
            bytes: v,
            sha256: "0".repeat(64),
        };
        assert!(matches!(
            identify(&img),
            Err(ImageError::NotSupported { .. })
        ));
    }

    #[test]
    fn reject_missing_banners() {
        let mut v = vec![0xFF_u8; layout::IMAGE_SIZE as usize];
        v[0..4].copy_from_slice(&layout::PROLOGUE_WORD0.to_le_bytes());
        v[4..8].copy_from_slice(&layout::PROLOGUE_WORD1.to_le_bytes());
        let img = Image {
            path: PathBuf::from("nobanner.bin"),
            bytes: v,
            sha256: "0".repeat(64),
        };
        assert!(matches!(identify(&img), Err(ImageError::Invalid { .. })));
    }
}
