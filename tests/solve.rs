//! Divergence→symbol attribution tests (Milestone AI, §36): the traced
//! decode replay, the byte-exactness of the replay against the boot's own
//! decode, and the mechanical P1 DIVERGENT attribution — pure units on
//! hand-built event lists plus real-firmware gates.

use truman_emulator::firmware;
use truman_emulator::inflate::{
    inflate_engine_with_traced, DecodeEvent, DecodeEventKind, ValueTables,
};
use truman_emulator::machine::{
    attribute_divergence, compare_dumps, Machine, MachineConfig, ENGINE_MAX_OUT,
    ENGINE_PRESET_BYTES,
};
use truman_emulator::memory::AccessSize;

const PIXEL_BUF: u32 = 0x0160_0000;
const ENGINE_IMG_WIDTH_REG: u32 = 0x1FFE_3828;
const ENGINE_IMG_HEIGHT_REG: u32 = 0x1FFE_382C;

// ---------------------------------------------------------------------------
// Pure attribution units (hand-built event lists)
// ---------------------------------------------------------------------------

/// literal@0 (1B) | end_of_block (0B) | match spanning 1..5 | literal@5.
fn hand_events() -> Vec<DecodeEvent> {
    vec![
        DecodeEvent {
            index: 0,
            out_start: 0,
            out_len: 1,
            kind: DecodeEventKind::Literal { value: 0x60 },
        },
        DecodeEvent {
            index: 1,
            out_start: 1,
            out_len: 0,
            kind: DecodeEventKind::EndOfBlock,
        },
        DecodeEvent {
            index: 2,
            out_start: 1,
            out_len: 4,
            kind: DecodeEventKind::Match {
                len_code: 257,
                len_extra_w: 0,
                len_extra_v: 0,
                length: 4,
                dist_code: 0,
                dist_extra_w: 0,
                dist_extra_v: 0,
                dist: 1,
            },
        },
        DecodeEvent {
            index: 3,
            out_start: 5,
            out_len: 1,
            kind: DecodeEventKind::Literal { value: 0x81 },
        },
    ]
}

#[test]
fn divergence_inside_a_match_names_the_match_and_offset() {
    let events = hand_events();
    // decoded output = 6 bytes: [0x60, m0, m1, m2, m3, 0x81]; window has
    // 2 band-fill bytes beyond.
    let reference = vec![0x60, 0x11, 0x11, 0x11, 0x11, 0x81, 0x00, 0x00];
    let mut captured = reference.clone();
    captured[3] ^= 0xFF; // the match's 3rd output byte
    let attr = attribute_divergence(&events, 6, &reference, &captured, 3);
    let ev = attr.event.as_ref().expect("byte 3 is decode output");
    assert_eq!(ev.event_index, 2);
    assert_eq!(ev.out_start, 1);
    assert_eq!(ev.out_len, 4);
    assert_eq!(ev.offset_in_event, 2);
    match ev.kind {
        DecodeEventKind::Match {
            len_code,
            length,
            dist,
            ..
        } => {
            assert_eq!(len_code, 257);
            assert_eq!(length, 4);
            assert_eq!(dist, 1);
        }
        ref k => panic!("expected a match, got {k:?}"),
    }
    assert_eq!(attr.reference_byte, 0x11);
    assert_eq!(attr.captured_byte, 0x11 ^ 0xFF);
}

#[test]
fn divergence_on_a_literal_is_reported_as_a_literal() {
    let events = hand_events();
    let reference = vec![0x60, 0x11, 0x11, 0x11, 0x11, 0x81, 0x00];
    let mut captured = reference.clone();
    captured[0] ^= 0xFF;
    let attr = attribute_divergence(&events, 6, &reference, &captured, 0);
    let ev = attr.event.as_ref().expect("byte 0 is decode output");
    assert_eq!(ev.event_index, 0);
    assert_eq!(ev.offset_in_event, 0);
    assert_eq!(
        ev.kind,
        DecodeEventKind::Literal { value: 0x60 },
        "a literal divergence is value-table-unreachable (§19.3)"
    );
}

#[test]
fn divergence_in_the_band_region_has_no_event() {
    let events = hand_events();
    let reference = vec![0x60, 0x11, 0x11, 0x11, 0x11, 0x81, 0x00, 0x00];
    let mut captured = reference.clone();
    captured[6] = 0xAB; // decoded_len = 6 → band fill
    let attr = attribute_divergence(&events, 6, &reference, &captured, 6);
    assert!(
        attr.event.is_none(),
        "band-fill bytes are not decode output (§17)"
    );
    assert_eq!(attr.decoded_len, 6);
}

#[test]
fn zero_span_events_never_cover_a_byte() {
    // The end-of-block marker (out_len 0) sits between the literal and the
    // match, both starting at output byte 1; the attribution must skip it
    // and land on the match.
    let events = hand_events();
    let reference = vec![0x60, 0x11, 0x11, 0x11, 0x11, 0x81];
    let mut captured = reference.clone();
    captured[1] ^= 0x55;
    let attr = attribute_divergence(&events, 6, &reference, &captured, 1);
    assert_eq!(attr.event.as_ref().unwrap().event_index, 2);
    assert_eq!(attr.event.as_ref().unwrap().offset_in_event, 0);
}

#[test]
#[should_panic(expected = "byte_index beyond the compared prefix")]
fn attribution_rejects_a_byte_beyond_the_compared_prefix() {
    let events = hand_events();
    let reference = vec![0x60; 4];
    let captured = vec![0x60; 4];
    let _ = attribute_divergence(&events, 6, &reference, &captured, 4);
}

// ---------------------------------------------------------------------------
// Real-firmware gates (the deterministic boot chain)
// ---------------------------------------------------------------------------

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

/// A candidate ladder in the m4 spirit (§17.5/§30.3), same shape as the
/// valtables tests: all extra-bit widths zeroed, length bases shifted.
fn m4_style_ladder() -> ValueTables {
    let mut t = ValueTables::rfc1951();
    for slot in t.lextra.iter_mut() {
        *slot = 0;
    }
    for slot in t.dextra.iter_mut() {
        *slot = 0;
    }
    for (i, slot) in t.lbase.iter_mut().enumerate() {
        *slot = (i as u16) + 1;
    }
    t
}

fn boot_window(config: &MachineConfig, img: &firmware::Image) -> (Vec<u8>, usize, usize) {
    let mut m = Machine::new(config.clone(), img);
    m.reset();
    let _ = m.run(2_000_000);
    let w = m
        .bus
        .peek_phys(ENGINE_IMG_WIDTH_REG, AccessSize::Word)
        .unwrap_or(0x400);
    let h = m
        .bus
        .peek_phys(ENGINE_IMG_HEIGHT_REG, AccessSize::Word)
        .unwrap_or(78);
    let total = (w as usize) * (h as usize);
    let mut window = vec![0u8; total];
    assert_eq!(m.bus.copy_out(PIXEL_BUF, &mut window), total);
    (window, w as usize, h as usize)
}

/// The traced replay of the boot's own decode is byte-exact, the event
/// spans tile the decode output exactly, and the §19.1 executable head is
/// reproduced by named events.
#[test]
fn traced_replay_is_byte_exact_and_events_tile_the_output() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let img = firmware::load(&path).expect("load real firmware");
    let config = MachineConfig::default();
    let mut m = Machine::new(config.clone(), &img);
    m.reset();
    let _ = m.run(2_000_000);

    let inputs = m
        .engine_decode_inputs()
        .expect("the real firmware's boot services the engine");
    assert!(!inputs.stream.is_empty());
    assert!(inputs.start_bit < inputs.stream.len() * 8);
    assert!(inputs.decoded_len > 0, "the boot decode produced output");

    // Window from the SAME machine (no second boot): the window's first
    // decoded_len bytes are the decode's output (§17 fill appends/truncates).
    let w = m
        .bus
        .peek_phys(ENGINE_IMG_WIDTH_REG, AccessSize::Word)
        .unwrap_or(0x400);
    let h = m
        .bus
        .peek_phys(ENGINE_IMG_HEIGHT_REG, AccessSize::Word)
        .unwrap_or(78);
    let total = (w as usize) * (h as usize);
    let mut window = vec![0u8; total];
    assert_eq!(m.bus.copy_out(PIXEL_BUF, &mut window), total);

    let mut replay = Vec::new();
    let mut events: Vec<DecodeEvent> = Vec::new();
    let n = inflate_engine_with_traced(
        &inputs.stream,
        inputs.start_bit,
        ENGINE_PRESET_BYTES,
        &mut replay,
        ENGINE_MAX_OUT,
        &config.value_tables,
        Some(&mut events),
    )
    .expect("replay succeeds");
    assert_eq!(n, inputs.decoded_len, "replay length == boot decode length");
    assert_eq!(
        &replay[..],
        &window[..inputs.decoded_len],
        "replay is byte-identical to the boot's own decode output"
    );
    assert!(inputs.decoded_len <= total, "decode output fits the window");

    // The events tile the decode output exactly: contiguous, ascending,
    // no gaps, no overlaps, total == decoded_len.
    let mut produced = 0usize;
    let mut literals = 0usize;
    let mut matches = 0usize;
    let mut eob = 0usize;
    for e in &events {
        if e.out_len > 0 {
            assert_eq!(e.out_start, produced, "spans are contiguous");
            produced += e.out_len;
        }
        match e.kind {
            DecodeEventKind::Literal { .. } => literals += 1,
            DecodeEventKind::EndOfBlock => eob += 1,
            DecodeEventKind::Match { .. } => matches += 1,
        }
    }
    assert_eq!(produced, inputs.decoded_len, "events tile the output");
    // Pinned (deterministic decode ⇒ deterministic trace): the RFC
    // reference decode's event census and lengths.
    assert_eq!(events.len(), 25_031);
    assert_eq!(literals, 15_762);
    assert_eq!(matches, 9_267);
    assert_eq!(eob, 2);
    assert_eq!(inputs.decoded_len, 75_408, "the decode's own output length");
    assert_eq!(
        total - inputs.decoded_len,
        4_464,
        "the zero-filled band region (§17) — the length-Δ family's shape"
    );

    // The §19.1 executable head, in event form (pinned): two literals
    // (60 81) then a match lc=259/len=5/dc=15/d=223 reaching into the
    // 32 KiB zero preset — the `00 00 00 00 00` fill whose first word is
    // the `jr v0` target. Deterministic decode ⇒ deterministic events.
    let head: Vec<u8> = window[..4].to_vec();
    assert_eq!(head, vec![0x60, 0x81, 0x00, 0x00], "the §19.1 head bytes");
    let covering: Vec<&DecodeEvent> = events
        .iter()
        .filter(|e| e.out_len > 0 && e.out_start < 4)
        .collect();
    assert_eq!(covering.len(), 3, "two literals + the preset match");
    assert_eq!(
        covering[0].kind,
        DecodeEventKind::Literal { value: 0x60 },
        "head byte 0: stream literal (ladder-independent, §19.3)"
    );
    assert_eq!(covering[0].out_start, 0);
    assert_eq!(
        covering[1].kind,
        DecodeEventKind::Literal { value: 0x81 },
        "head byte 1: stream literal"
    );
    assert_eq!(covering[1].out_start, 1);
    assert_eq!(
        covering[2].kind,
        DecodeEventKind::Match {
            len_code: 259,
            len_extra_w: 0,
            len_extra_v: 0,
            length: 5,
            dist_code: 15,
            dist_extra_w: 6,
            dist_extra_v: 30,
            dist: 223,
        },
        "head bytes 2..7: one match reaching the zero preset (193+30 = d=223 < 0x8000)"
    );
    assert_eq!(covering[2].out_start, 2);
    assert_eq!(covering[2].out_len, 5);
}

/// The P1 DIVERGENT mechanical half, end-to-end on the real firmware: a
/// candidate-ladder decode as the synthetic capture; the attribution must
/// name the event whose span covers the first divergent byte under the
/// RFC trace, and every byte before it is capture-identical by
/// construction.
#[test]
fn candidate_ladder_capture_attributes_to_the_covering_event() {
    let Some(path) = real_firmware_path() else {
        eprintln!("skipping: real firmware not present in firmware/");
        return;
    };
    let img = firmware::load(&path).expect("load real firmware");

    // Reference boot (RFC ladder) + traced replay of its own decode.
    let config = MachineConfig::default();
    let mut m = Machine::new(config.clone(), &img);
    m.reset();
    let _ = m.run(2_000_000);
    let inputs = m.engine_decode_inputs().expect("engine ran");
    let w = m
        .bus
        .peek_phys(ENGINE_IMG_WIDTH_REG, AccessSize::Word)
        .unwrap_or(0x400);
    let h = m
        .bus
        .peek_phys(ENGINE_IMG_HEIGHT_REG, AccessSize::Word)
        .unwrap_or(78);
    let total = (w as usize) * (h as usize);
    let mut reference = vec![0u8; total];
    assert_eq!(m.bus.copy_out(PIXEL_BUF, &mut reference), total);
    let mut replay = Vec::new();
    let mut events = Vec::new();
    let n = inflate_engine_with_traced(
        &inputs.stream,
        inputs.start_bit,
        ENGINE_PRESET_BYTES,
        &mut replay,
        ENGINE_MAX_OUT,
        &config.value_tables,
        Some(&mut events),
    )
    .expect("replay succeeds");
    assert_eq!(n, inputs.decoded_len);
    assert_eq!(&replay[..], &reference[..inputs.decoded_len]);

    // Candidate-ladder boot = the synthetic "device capture".
    let cand_config = MachineConfig {
        value_tables: m4_style_ladder(),
        ..MachineConfig::default()
    };
    let (captured, _, _) = boot_window(&cand_config, &img);
    assert_ne!(captured, reference, "the candidate must change the decode");

    let cmp = compare_dumps(&reference, &captured);
    let k = cmp.first_divergence.expect("the decodes differ");
    let attr = attribute_divergence(&events, inputs.decoded_len, &reference, &captured, k);
    // Pinned (deterministic): the m4-style candidate first diverges at
    // byte 5 — INSIDE the §19.1 head match (event #2, span 2..7), at its
    // 4th output byte — exactly the shape the P1 DIVERGENT row exists to
    // name: "the first match whose value semantics differ".
    assert_eq!(k, 5);
    let ev = attr.event.as_ref().expect("byte 5 is decode output");
    assert_eq!(ev.event_index, 2);
    assert_eq!(ev.out_start, 2);
    assert_eq!(ev.out_len, 5);
    assert_eq!(ev.offset_in_event, 3);
    assert!(
        matches!(ev.kind, DecodeEventKind::Match { len_code: 259, .. }),
        "the attribution names the head match"
    );
    // The first-divergence definition, re-checked: every byte before k
    // is capture-identical.
    assert_eq!(&reference[..k], &captured[..k]);
}
