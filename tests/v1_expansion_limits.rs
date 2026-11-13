//! The legacy v1 (LZMA) payload is decompressed only up to the length its header declares.
//!
//! * direct mode: the payload is the text, so the header's `original_length` is the exact length; a payload that
//!   expands past it, or to fewer bytes, is rejected
//! * pattern mode: the payload is the pattern table and the processed text, at most
//!   `8 + 2 * original_length + 24 * exception_count` bytes (each match: 11 bytes of fields plus its text, which is
//!   part of the original; each placeholder `{P<i>}` at most 13 bytes)
//!
//! A "bomb" below is an LZMA stream of a few kilobytes that expands to far more than its header declares.

use alice_text::{
    EncodingMode, ExceptionDecoder, ExceptionHeader, ALICE_TEXT_MAGIC, ALICE_TEXT_VERSION,
};

/// An LZMA stream (the format the v1 writer uses) of `n` zero bytes
fn lzma_zeros(n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    lzma_rs::lzma_compress(&mut std::io::Cursor::new(vec![0u8; n]), &mut out).expect("compresses");
    out
}

fn lzma_of(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    lzma_rs::lzma_compress(&mut std::io::Cursor::new(bytes), &mut out).expect("compresses");
    out
}

/// A v1 file: magic, version, the header, then the payload
fn v1_file(header: &ExceptionHeader, payload: &[u8]) -> Vec<u8> {
    let mut f = ALICE_TEXT_MAGIC.to_vec();
    f.push(ALICE_TEXT_VERSION.0);
    f.push(ALICE_TEXT_VERSION.1);
    let mut h = header.clone();
    h.compressed_length = u32::try_from(payload.len()).expect("small");
    f.extend_from_slice(&h.to_bytes());
    f.extend_from_slice(payload);
    f
}

const fn direct_header(original_length: u32) -> ExceptionHeader {
    let mut h = ExceptionHeader::new(EncodingMode::Pattern);
    h.original_length = original_length;
    h.pattern_db_length = 0; // direct
    h
}

/// Decoding stopped at the declared length (the stream would have produced more)
fn stopped_at_the_declared_length(r: &alice_text::Result<String>) -> bool {
    matches!(r, Err(alice_text::ALICETextError::DecompressionError(m)) if m.contains("expands past its declared length"))
}

/// The whole payload was decoded and is not the declared length
fn not_the_declared_length(r: &alice_text::Result<String>) -> bool {
    matches!(r, Err(alice_text::ALICETextError::DecompressionError(m)) if m.starts_with("the payload is"))
}

#[test]
fn a_direct_payload_reads_back_at_exactly_its_declared_length() {
    let text = b"exactly sixteen!";
    let file = v1_file(&direct_header(16), &lzma_of(text));
    let got = ExceptionDecoder::new()
        .decode_from_bytes(&file)
        .expect("reads");
    assert_eq!(got.as_bytes(), text);
}

#[test]
fn a_direct_payload_that_expands_past_its_declared_length_is_refused() {
    // 4 MiB of zeros behind a header that declares 16 bytes
    let file = v1_file(&direct_header(16), &lzma_zeros(4 << 20));
    assert!(
        // the v1 writer's LZMA encoder (lzma-rs) is not tight: about 104 KB for 4 MiB, still far smaller than it expands
        file.len() < (4 << 20) / 16,
        "the bomb is small: {} bytes",
        file.len()
    );
    let r = ExceptionDecoder::new().decode_from_bytes(&file);
    assert!(stopped_at_the_declared_length(&r), "{r:?}");
}

#[test]
fn a_direct_payload_shorter_than_declared_is_refused() {
    let file = v1_file(&direct_header(100), &lzma_of(b"ten bytes!"));
    let r = ExceptionDecoder::new().decode_from_bytes(&file);
    assert!(not_the_declared_length(&r), "{r:?}");
}

#[test]
fn a_pattern_payload_past_its_bound_is_refused() {
    // pattern mode, 16 bytes of text and 1 match: the payload is at most 8 + 32 + 24 = 64 bytes
    let mut h = ExceptionHeader::new(EncodingMode::Pattern);
    h.original_length = 16;
    h.exception_count = 1;
    h.pattern_db_length = 1; // pattern
    let file = v1_file(&h, &lzma_zeros(1 << 20));
    let r = ExceptionDecoder::new().decode_from_bytes(&file);
    assert!(stopped_at_the_declared_length(&r), "{r:?}");
}

#[test]
fn the_v1_writer_output_still_reads_back_in_both_modes() {
    // the bounds hold for what the writer produces: text with patterns (pattern mode when smaller) and without
    let samples = [
        "2026-11-13 10:00:01 INFO 192.168.0.1 GET /a\n2026-11-13 10:00:02 INFO 192.168.0.2 GET /b\n".repeat(40),
        "plain words without any pattern at all, repeated. ".repeat(20),
    ];
    let mut compared = 0;
    for mode in [EncodingMode::Pattern, EncodingMode::NGram] {
        for text in &samples {
            let enc = alice_text::ExceptionEncoder::new(mode);
            let encoded = enc.encode(text).expect("encodes");
            let bytes = enc.to_bytes(&encoded).expect("writes");
            let back = ExceptionDecoder::new()
                .decode_from_bytes(&bytes)
                .expect("reads back");
            assert_eq!(&back, text);
            compared += 1;
        }
    }
    assert_eq!(compared, 4);
}
