//! The vendor vector gate (§40.4, Milestone AN): the Rust reference
//! implementation, dual-pinned against the Python reference
//! (`tools/fw_vector_probe.py`) and the §40 documentation.
//!
//! Pins enforced here:
//! * the container's own accounting (3,744-byte array, SP magic, 9
//!   chunks, stream_len 0xe98 = 3,736);
//! * the RFC ladder's documented behavior on the vector — completes at
//!   every candidate start bit with zero invalid distances, clean
//!   alphabet, pinned ≥990‰ hex backbone, over-runs +570/+377/+358, and
//!   therefore FAILS the accounting condition (the gate discriminates);
//! * the ladder-independence of the literal census (§19.3) and the
//!   ladder-sensitivity of the accounting — a degenerate ladder moves
//!   the decode length and fails the gate;
//! * structured extraction errors (no marker, truncated body).

use truman_emulator::inflate::ValueTables;
use truman_emulator::vendor_vector::{
    extract_vector_container, reference_container, vector_gate, vector_gate_report,
    VectorGateError, VECTOR_CONTAINER_BYTES,
};

/// The archived vendor container parses and pins its own accounting
/// (§40.1): SP magic, 9 chunks, declared stream length, 3,744 bytes.
#[test]
fn container_from_reference_pins_its_accounting() {
    let c = reference_container().expect("the committed vendor reference must parse");
    assert_eq!(c.chunks, 9);
    assert_eq!(c.stream_len, 0xe98);
    assert_eq!(c.stream.len(), 0xe98_usize);
    assert_eq!(c.stream.len() + 8, VECTOR_CONTAINER_BYTES);
}

/// The RFC ladder's vector behavior is the documented §40 baseline at
/// every candidate start bit: completes (zero invalid distances), clean
/// alphabet, pinned backbone, exact over-run — and the gate FAILS
/// overall, because the accounting never hits the 9,216 anchor. The
/// gate's discriminating edge is real.
#[test]
fn rfc_ladder_pins_documented_overruns_and_fails_the_gate() {
    let c = reference_container().expect("the committed vendor reference must parse");
    let rfc = ValueTables::rfc1951();
    for (start, overrun) in [(320, 570), (1024, 377), (1088, 358)] {
        let r = vector_gate_report(&c.stream, start, &rfc);
        assert!(r.completed, "RFC decode must complete at start {start}");
        assert!(r.error.is_none());
        assert!(
            r.alphabet_clean(),
            "alphabet must be clean at start {start}"
        );
        assert!(r.backbone_ok(), "backbone must hold at start {start}");
        assert_eq!(
            r.anchor_delta, overrun,
            "documented over-run at start {start}"
        );
        assert!(
            !r.accounting_ok(),
            "RFC must fail the accounting at start {start}"
        );
        assert!(!r.pass(), "RFC must fail the gate at start {start}");
    }
    let (reports, overall) = vector_gate(&c.stream, &rfc);
    assert_eq!(reports.len(), 3);
    assert!(!overall, "the RFC ladder must not pass the vector gate");
}

/// A value-family ladder (RFC base values shifted +1, extra-bit widths
/// untouched) must move the accounting off the RFC pin and fail the gate
/// — while the literal census stays EXACTLY the RFC one (§19.3: literal
/// values are stream-determined at a fixed frame). The census is NOT
/// invariant to width changes — widths are stream-position data; a
/// degenerate all-zero-width ladder re-aligns the stream and shifts the
/// census (pinned here too). The gate's conditions respond to both
/// families; the accounting alone discriminates within the value family.
#[test]
fn value_family_ladder_moves_accounting_and_fails_the_gate() {
    let c = reference_container().expect("the committed vendor reference must parse");
    let rfc = ValueTables::rfc1951();
    let mut shifted = rfc.clone();
    for b in shifted.lbase.iter_mut() {
        *b += 1;
    }
    for b in shifted.dbase.iter_mut() {
        *b += 1;
    }
    let r_rfc = vector_gate_report(&c.stream, 1024, &rfc);
    let r_shift = vector_gate_report(&c.stream, 1024, &shifted);
    assert_eq!(
        r_shift.lit_total, r_rfc.lit_total,
        "at fixed widths the literal census is value-ladder-independent (§19.3)"
    );
    assert_eq!(
        r_shift.hex_literals, r_rfc.hex_literals,
        "at fixed widths the backbone is value-ladder-independent (§19.3)"
    );
    assert_ne!(
        r_shift.decode_len, r_rfc.decode_len,
        "the accounting must move under different base values"
    );
    assert!(!r_shift.pass());
}

/// A width-family ladder (all extra-bit widths zeroed) re-aligns the
/// stream — the literal census MOVES (2,350 vs the RFC 1,538 at start
/// 1024) and the gate fails. Widths are stream-position data, not just
/// value semantics.
#[test]
fn width_family_ladder_realigns_the_stream_and_fails_the_gate() {
    let c = reference_container().expect("the committed vendor reference must parse");
    let rfc = ValueTables::rfc1951();
    let zerowidth = ValueTables {
        lbase: [3; 29],
        lextra: [0; 29],
        dbase: [1; 30],
        dextra: [0; 30],
    };
    let r_rfc = vector_gate_report(&c.stream, 1024, &rfc);
    let r_zero = vector_gate_report(&c.stream, 1024, &zerowidth);
    assert_ne!(
        r_zero.lit_total, r_rfc.lit_total,
        "zero widths re-align the stream: the census must move"
    );
    assert!(!r_zero.pass());
}

/// Extraction failures are structured, not panics: a source without the
/// marker, and a truncated array body, each map to their error.
#[test]
fn extraction_errors_are_structured() {
    assert_eq!(
        extract_vector_container("no marker in this source"),
        Err(VectorGateError::ContainerMarkerMissing)
    );
    assert_eq!(
        extract_vector_container("ZlibBuf[16*234] = { 0x00 0x11"),
        Err(VectorGateError::ContainerTruncated)
    );
    assert_eq!(
        extract_vector_container("ZlibBuf[16*234] = {};"),
        Err(VectorGateError::ContainerBytes {
            expected: VECTOR_CONTAINER_BYTES,
            found: 0,
        })
    );
}

/// A corrupted container (bad magic, inconsistent accounting) is
/// rejected before any gate verdict could be computed from it: synthetic
/// C sources carrying the marker with 3,744 real bytes but a broken
/// header.
#[test]
fn container_validation_rejects_corruption() {
    let fake_source = |header: [u8; 8]| {
        let mut body = String::from("ZlibBuf[16*234] = {");
        for b in header
            .iter()
            .chain(std::iter::repeat_n(&0u8, VECTOR_CONTAINER_BYTES - 8))
        {
            body.push_str(&format!(" 0x{b:02x}"));
        }
        body.push_str(" };");
        body
    };
    // Correct header (SP, 9 chunks, stream_len 3736) must parse.
    let good = fake_source([0x53, 0x50, 0x00, 0x09, 0x00, 0x00, 0x0e, 0x98]);
    assert!(extract_vector_container(&good).is_ok());
    // Wrong magic must be named.
    let bad_magic = fake_source([0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x0e, 0x98]);
    assert_eq!(
        extract_vector_container(&bad_magic),
        Err(VectorGateError::ContainerMagic {
            found: [0x00, 0x00]
        })
    );
    // Right magic, wrong accounting (chunks = 1) must be named.
    let bad_acct = fake_source([0x53, 0x50, 0x00, 0x01, 0x00, 0x00, 0x0e, 0x98]);
    assert_eq!(
        extract_vector_container(&bad_acct),
        Err(VectorGateError::ContainerAccounting {
            chunks: 1,
            stream_len: 0xe98,
            bytes: VECTOR_CONTAINER_BYTES,
        })
    );
}
