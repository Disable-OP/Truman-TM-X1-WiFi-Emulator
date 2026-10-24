//! Milestone T (docs/firmware-analysis.md §22): the Q8 runtime verdict —
//! the loader's checksum code path, observed live with watchpoints.
//!
//! Q8 (§15.2, §16.2) asked for the checksum algorithm behind two
//! descriptor anchors. §21.6 named "the loader's own checksum code path
//! (if any)" as the decisive escalation. These tests pin what the real
//! boot actually does, observed by booting the real firmware with
//! physical-range watchpoints over the descriptors, the payload, the
//! container and the DRAM destination:
//!
//! * the stage-1 loader reads exactly three descriptor words (source
//!   offset 0x121C, destination 0x1214, length 0x1220) — the checksum
//!   word 0x1224 and flags word 0x1228 are NEVER read;
//! * the driver payload is read exactly once, by the `lwl/lwr` copy loop
//!   at 0x9800ab24/0x9800ab28 — one pass, no digest interleaved;
//! * the copied payload is re-read 19 times in total (an 18-entry config
//!   table in the driver's own image) — never a bulk pass;
//! * the container + eCos descriptor bytes are streamed byte-wise by the
//!   driver loop at 0x82001c08 (`lbu; sb; bnez`, no accumulator) — the
//!   desc[6] checksum bytes are read as part of the 28-byte parse, and
//!   nothing ever computes against them;
//! * after the eCos entry no data-read pass over the pixel buffer occurs
//!   (the model head runs to its known +0x50 fault with zero such reads).
//!
//! Together with the instruction-level disassembly (§22.3) this resolves
//! Q8's runtime half: no code in the observed boot chain validates either
//! checksum anchor; the words are packaging metadata.

use truman_emulator::boot;
use truman_emulator::firmware;
use truman_emulator::machine::{Machine, MachineConfig, StopReason};
use truman_emulator::memory::{WatchHit, WatchKind, WatchKinds};

/// The eCos entry: the loader's `jr v0` target (§13.3, §19.1).
const ENTRY: u32 = 0xA160_0000;
/// Stage-1 data-area descriptor words (file offsets; both flash windows).
const DESC_OFF: u32 = 0x1210;
const DESC_LEN: u32 = 0x70; // 0x1210..0x1280: entry + later table words
/// Driver payload (file 0x2F6C..0x6948, 0x3ADC bytes).
const PAYLOAD_OFF: u32 = 0x2F6C;
const PAYLOAD_LEN: u32 = 0x3ADC;
/// eCos package descriptor entry (28 bytes) + container stream.
const ECOS_DESC_OFF: u32 = 0x10010;
const ECOS_DESC_LEN: u32 = 0x1C;
const CONTAINER_OFF: u32 = 0x1002C;
const CONTAINER_LEN: u32 = 0x868C;
/// Driver payload DRAM destination (desc word 0x1214 = 0xA2000000).
const DRAM_DEST: u32 = 0x0200_0000;
/// Pixel buffer (engine output, eCos entry image).
const PIXEL_BUF: u32 = 0x0160_0000;

const MIRROR: u32 = 0x1800_0000; // MachineConfig::win_180_base
const XIP: u32 = 0x1FC0_0000; // MachineConfig::flash_base

fn real_firmware_path() -> Option<std::path::PathBuf> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let fw_dir = std::path::Path::new(manifest).join("firmware");
    std::fs::read_dir(fw_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.extension().map(|x| x == "bin").unwrap_or(false)
                && p.file_name()
                    .map(|n| n.to_string_lossy().contains("1506HV"))
                    .unwrap_or(false)
        })
}

/// Boot the real firmware to the eCos entry with READ watches over
/// `ranges` (given as (file_offset, len) pairs installed on BOTH flash
/// windows) plus a READ_WRITE watch on the driver payload's DRAM
/// destination. Returns (machine, hits drained after the boot).
fn boot_with_watches(ranges: &[(&str, u32, u32)]) -> (Machine, Vec<WatchHit>) {
    let path = real_firmware_path().expect("real firmware must be present");
    let image = firmware::load(&path).expect("load real firmware");
    let cfg = MachineConfig::default();
    let mut m = Machine::new(cfg, &image);
    for (_, off, len) in ranges {
        m.add_watch(MIRROR + off, *len, WatchKinds::READ);
        m.add_watch(XIP + off, *len, WatchKinds::READ);
    }
    m.add_watch(DRAM_DEST, PAYLOAD_LEN, WatchKinds::READ_WRITE);
    boot::reset_with_diagnostics(&mut m, false);
    let res = m.run_until_pc(4_000_000, ENTRY);
    if !matches!(res.stop, StopReason::EntryReached { .. }) {
        panic!("boot did not reach the eCos entry: {:?}", res.stop);
    }
    let hits = m.take_watch_hits();
    (m, hits)
}

#[test]
fn stage1_reads_exactly_three_descriptor_words_and_never_the_checksum() {
    let (_, hits) = boot_with_watches(&[("stage1-desc", DESC_OFF, DESC_LEN)]);
    // Only the mirror window is read (Q2: stage-1 executes/reads through
    // 0x98000000); the XIP window sees nothing here.
    let desc_hits_all_windows: Vec<&WatchHit> = hits
        .iter()
        .filter(|h| {
            let phys = h.phys & 0x1FFF_FFFF;
            (MIRROR + DESC_OFF..MIRROR + DESC_OFF + DESC_LEN).contains(&phys)
                || (XIP + DESC_OFF..XIP + DESC_OFF + DESC_LEN).contains(&phys)
        })
        .collect();
    assert!(
        desc_hits_all_windows
            .iter()
            .all(|h| h.phys & 0x1FFF_FFFF >= MIRROR),
        "all stage-1 descriptor reads must come from the mirror window"
    );
    // Exactly three words are read, each exactly once, with the documented
    // values (source offset, destination, length).
    let expected = [
        (0xB800_121C, 0x135C, 0x9800_AA7C), // source offset (0x1C10 base)
        (0xB800_1214, 0xA200_0000, 0x9800_AA94), // destination
        (0xB800_1220, 0x3ADC, 0x9800_AAA0), // length
    ];
    let mut desc_hits: Vec<&WatchHit> = hits
        .iter()
        .filter(|h| h.phys & 0x1FFF_FFFF >= MIRROR + DESC_OFF)
        .filter(|h| h.phys & 0x1FFF_FFFF < MIRROR + DESC_OFF + DESC_LEN)
        .collect();
    desc_hits.sort_by_key(|h| h.step);
    assert_eq!(desc_hits.len(), 3, "exactly three descriptor words read");
    for (i, (vaddr, value, pc)) in expected.iter().enumerate() {
        assert_eq!(desc_hits[i].vaddr, *vaddr, "word {i} vaddr");
        assert_eq!(desc_hits[i].value, *value, "word {i} value");
        assert_eq!(desc_hits[i].pc, *pc, "word {i} issuing PC");
    }
    // The checksum word (0x1224) and every other table word: never read.
    for h in &hits {
        let phys = h.phys & 0x1FFF_FFFF;
        assert_ne!(
            phys,
            MIRROR + 0x1224,
            "checksum word 0x1224 must never be read"
        );
        assert_ne!(
            phys,
            MIRROR + 0x1218,
            "source-field word 0x1218 must never be read"
        );
        assert_ne!(
            phys,
            MIRROR + 0x1228,
            "flags word 0x1228 must never be read"
        );
    }
}

#[test]
fn driver_payload_is_read_exactly_once_by_the_copy_loop() {
    let (_, hits) = boot_with_watches(&[("driver-payload", PAYLOAD_OFF, PAYLOAD_LEN)]);
    let mut payload_hits: Vec<&WatchHit> = hits
        .iter()
        .filter(|h| h.kind == WatchKind::Read)
        .filter(|h| {
            let phys = h.phys & 0x1FFF_FFFF;
            (MIRROR + PAYLOAD_OFF..MIRROR + PAYLOAD_OFF + PAYLOAD_LEN).contains(&phys)
                || (XIP + PAYLOAD_OFF..XIP + PAYLOAD_OFF + PAYLOAD_LEN).contains(&phys)
        })
        .collect();
    assert_eq!(
        payload_hits.len(),
        2 * 15_068,
        "exactly one pass: 15068 bytes x (lwl + lwr)"
    );
    // All reads come from the two unaligned-load instructions of the
    // stage-1 copy loop, through the mirror window.
    for h in &payload_hits {
        assert!(
            h.pc == 0x9800_AB24 || h.pc == 0x9800_AB28,
            "unexpected reader pc 0x{:08x}",
            h.pc
        );
        assert_eq!(h.size, truman_emulator::memory::AccessSize::Byte);
    }
    payload_hits.sort_by_key(|h| h.step);
    assert_eq!(payload_hits[0].vaddr, 0xB800_2F6C, "first byte read");
    assert_eq!(
        payload_hits.last().unwrap().vaddr,
        0xB800_6A47,
        "last byte read"
    );
    // No read anywhere in the XIP window (the loader never uses it here).
    assert!(
        payload_hits.iter().all(|h| h.phys & 0x1FFF_FFFF >= MIRROR),
        "payload reads must come from the mirror window"
    );
}

#[test]
fn copied_payload_is_never_digest_read_after_the_copy() {
    let (_, hits) = boot_with_watches(&[("driver-payload", PAYLOAD_OFF, PAYLOAD_LEN)]);
    let writes: Vec<&WatchHit> = hits
        .iter()
        .filter(|h| {
            h.kind == WatchKind::Write && ((DRAM_DEST..DRAM_DEST + PAYLOAD_LEN).contains(&h.phys))
        })
        .collect();
    assert_eq!(
        writes.len(),
        3_767,
        "the copy performs 0x3ADC/4 aligned word stores"
    );
    assert!(
        writes.iter().all(|h| h.pc == 0x9800_AB2C),
        "all destination stores come from the copy loop's sw"
    );
    // Post-copy reads of the destination: exactly 19 words from the
    // driver's own config-table code — NOT a digest pass (a byte or word
    // checksum over 0x3ADC bytes would need >= 0x3ADC accesses).
    let reads: Vec<&WatchHit> = hits
        .iter()
        .filter(|h| {
            h.kind == WatchKind::Read && (DRAM_DEST..DRAM_DEST + PAYLOAD_LEN).contains(&h.phys)
        })
        .collect();
    assert_eq!(reads.len(), 19, "config-table reads only");
    assert!(
        reads
            .iter()
            .all(|h| h.pc == 0xA200_27EC || h.pc == 0xA200_283C || h.pc == 0xA200_2888),
        "post-copy readers are the driver's config-table code"
    );
}

#[test]
fn container_is_streamed_byte_wise_and_checksum_bytes_are_never_used() {
    let (_, hits) = boot_with_watches(&[
        ("ecos-desc", ECOS_DESC_OFF, ECOS_DESC_LEN),
        ("ecos-container", CONTAINER_OFF, CONTAINER_LEN),
    ]);
    // The driver's byte loop reads all 28 descriptor bytes and all 34444
    // container bytes from the mirror, byte by byte, one pass.
    let reads: Vec<&WatchHit> = hits
        .iter()
        .filter(|h| h.kind == WatchKind::Read)
        .filter(|h| h.phys & 0x1FFF_FFFF >= MIRROR)
        .collect();
    assert_eq!(reads.len(), ECOS_DESC_LEN as usize + CONTAINER_LEN as usize);
    assert!(
        reads.iter().all(|h| h.pc == 0x8200_1C08),
        "all container/desc reads come from the driver's streaming loop"
    );
    assert!(
        reads
            .iter()
            .all(|h| h.size == truman_emulator::memory::AccessSize::Byte),
        "the streaming loop is byte-wise"
    );
    // The desc[6] checksum bytes ARE read (part of the 28-byte parse) —
    // with the exact anchor values, proving the parse touches them; the
    // disassembly (§22.3) shows the loop has no accumulator.
    let mut by_addr: Vec<&&WatchHit> = reads
        .iter()
        .filter(|h| {
            let phys = h.phys & 0x1FFF_FFFF;
            (MIRROR + 0x10028..MIRROR + 0x1002C).contains(&phys)
        })
        .collect();
    by_addr.sort_by_key(|h| h.step);
    assert_eq!(by_addr.len(), 4);
    let values: Vec<u32> = by_addr.iter().map(|h| h.value).collect();
    assert_eq!(
        values,
        vec![0xFF, 0xF9, 0xEB, 0xB5],
        "0xB5EBF9FF little-endian"
    );
}

#[test]
fn no_data_read_pass_after_the_ecos_entry() {
    let path = real_firmware_path().expect("real firmware must be present");
    let image = firmware::load(&path).expect("load real firmware");
    let mut m = Machine::new(MachineConfig::default(), &image);
    // Watch everything a post-entry validator could digest: its own image
    // (pixel buffer, data reads only — execution is fetch), the copied
    // payload and the container.
    m.add_watch(PIXEL_BUF, 79_872, WatchKinds::READ);
    m.add_watch(DRAM_DEST, PAYLOAD_LEN, WatchKinds::READ_WRITE);
    m.add_watch(MIRROR + CONTAINER_OFF, CONTAINER_LEN, WatchKinds::READ);
    boot::reset_with_diagnostics(&mut m, false);
    let res = m.run_until_pc(4_000_000, ENTRY);
    assert!(matches!(res.stop, StopReason::EntryReached { .. }));
    m.take_watch_hits();
    // Continue the bounded post-entry run (the model head runs to its
    // known +0x50 fault, §19.2).
    let _ = m.run(100);
    let hits = m.take_watch_hits();
    let data_reads: Vec<&WatchHit> = hits.iter().filter(|h| h.kind == WatchKind::Read).collect();
    assert!(
        data_reads.is_empty(),
        "no bulk data-read (checksum) pass exists after the entry; got {:?}",
        data_reads.iter().take(8).collect::<Vec<_>>()
    );
}
