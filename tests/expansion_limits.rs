//! Decompressed sizes are bounded before the bytes are produced.
//!
//! * format v3 records each column's decompressed size; a column that expands
//!   past it, or to a different size, is rejected
//! * files without a recorded size (format v2, dialogue tables, v3 columns
//!   with `uncompressed_size == 0` as written by earlier versions) accept at
//!   most 256 MiB per Zstd stream
//!
//! A "bomb" below is a Zstd frame of a few kilobytes that expands to more than
//! the limit; it is written in chunks so the test itself never holds the
//! expanded bytes.

// sizes here are a few hundred MiB at most and the tests run on 64-bit hosts
#![allow(clippy::cast_possible_truncation)]

use alice_text::format_v3::{ColumnEntry, FormatV3Header, FormatV3Metadata};
use alice_text::{
    compress_tuned, compress_v3, decompress_tuned, decompress_v3, decompress_v3_with_limit,
    ALICETextError, CompressionLevel, CompressionMode, DialogueCompressionMode, DialogueCompressor,
    DialogueEntry, DialogueTable,
};
use std::io::{Cursor, Write};

/// 256 MiB, the documented limit for streams without a recorded size
const UNRECORDED_LIMIT: u64 = 256 * 1024 * 1024;
const DIR_START: usize = 8 + 2 + FormatV3Header::SIZE;

/// A Zstd frame that expands to `n` zero bytes
fn zeros_frame(n: u64) -> Vec<u8> {
    let mut enc = zstd::stream::Encoder::new(Vec::new(), 1).unwrap();
    let chunk = vec![0u8; 1 << 20];
    let mut left = n;
    while left > 0 {
        let k = left.min(chunk.len() as u64) as usize;
        enc.write_all(&chunk[..k]).unwrap();
        left -= k as u64;
    }
    enc.finish().unwrap()
}

fn is_err_with(r: &Result<impl std::fmt::Debug, ALICETextError>, needle: &str) -> bool {
    matches!(r, Err(ALICETextError::DecompressionError(m)) if m.contains(needle))
}

fn sample_text() -> String {
    "2024-01-15 10:30:45 INFO request 17 from 10.0.0.1 accepted\n".repeat(30)
}

// --- format v3 ----------------------------------------------------------------

fn entry_field(data: &mut [u8], index: usize, at: usize, value: u32) {
    let p = DIR_START + index * ColumnEntry::SIZE + at;
    data[p..p + 4].copy_from_slice(&value.to_le_bytes());
}

const SIZE_AT: usize = 9;
const UNCOMPRESSED_AT: usize = 13;

/// (file, index of the column that ends the file, its offset, its recorded size)
fn v3_with_last_column() -> (Vec<u8>, usize, usize, u32) {
    let data = compress_v3(&sample_text(), CompressionLevel::Balanced).unwrap();
    let meta = FormatV3Metadata::read_from(&mut Cursor::new(&data)).unwrap();
    let (i, e) = meta
        .columns
        .iter()
        .enumerate()
        .max_by_key(|(_, e)| e.offset)
        .unwrap();
    (data, i, e.offset as usize, e.uncompressed_size)
}

#[test]
fn v3_writer_records_each_decompressed_size() {
    let data = compress_v3(&sample_text(), CompressionLevel::Balanced).unwrap();
    let meta = FormatV3Metadata::read_from(&mut Cursor::new(&data)).unwrap();
    for e in &meta.columns {
        let start = e.offset as usize;
        let block = &data[start..start + e.compressed_size as usize];
        let expanded = zstd::stream::decode_all(Cursor::new(block)).unwrap();
        assert_eq!(
            e.uncompressed_size as usize,
            expanded.len(),
            "{:?}",
            e.col_type
        );
        assert!(e.uncompressed_size > 0, "a bincode column is never empty");
    }
}

#[test]
fn v3_recorded_size_exact_decodes_and_off_by_one_is_rejected() {
    let (data, i, _, size) = v3_with_last_column();
    assert_eq!(decompress_v3(&data).unwrap(), sample_text());

    let mut short = data.clone();
    entry_field(&mut short, i, UNCOMPRESSED_AT, size - 1);
    assert!(is_err_with(&decompress_v3(&short), "expands past"));

    let mut long = data;
    entry_field(&mut long, i, UNCOMPRESSED_AT, size + 1);
    assert!(is_err_with(&decompress_v3(&long), "does not match"));
}

#[test]
fn v3_file_without_recorded_sizes_still_decodes() {
    // earlier versions wrote 0 in every `uncompressed_size`
    let data = compress_v3(&sample_text(), CompressionLevel::Balanced).unwrap();
    let n = FormatV3Metadata::read_from(&mut Cursor::new(&data))
        .unwrap()
        .columns
        .len();
    let mut old = data;
    for i in 0..n {
        entry_field(&mut old, i, UNCOMPRESSED_AT, 0);
    }
    assert_eq!(decompress_v3(&old).unwrap(), sample_text());
}

/// The last column replaced by a bomb, with `recorded` as its decompressed size
fn v3_bomb(expands_to: u64, recorded: u32) -> Vec<u8> {
    let (data, i, offset, _) = v3_with_last_column();
    let bomb = zeros_frame(expands_to);
    let mut out = data[..offset].to_vec();
    out.extend_from_slice(&bomb);
    entry_field(&mut out, i, SIZE_AT, bomb.len() as u32);
    entry_field(&mut out, i, UNCOMPRESSED_AT, recorded);
    out
}

#[test]
fn v3_bomb_past_the_recorded_size_is_rejected() {
    let data = v3_bomb(64 << 20, 1024);
    assert!(
        data.len() < 64 * 1024,
        "the bomb is small: {} bytes",
        data.len()
    );
    assert!(is_err_with(&decompress_v3(&data), "expands past 1024"));
}

#[test]
fn v3_bomb_without_recorded_size_stops_at_the_fixed_limit() {
    let data = v3_bomb(UNRECORDED_LIMIT + (1 << 20), 0);
    assert!(is_err_with(&decompress_v3(&data), "expands past 268435456"));
}

// --- format v2 ----------------------------------------------------------------

#[test]
fn v2_bomb_stops_at_the_fixed_limit() {
    let valid = compress_tuned(&sample_text(), CompressionMode::Fast).unwrap();
    let header_end = 10 + alice_text::TunedHeader::SIZE;
    let mut data = valid[..header_end].to_vec();
    data.extend_from_slice(&zeros_frame(UNRECORDED_LIMIT + (1 << 20)));
    assert!(data.len() < 64 * 1024);
    assert!(is_err_with(
        &decompress_tuned(&data),
        "expands past 268435456"
    ));
}

#[test]
fn v2_round_trip_is_unchanged() {
    let c = compress_tuned(&sample_text(), CompressionMode::Fast).unwrap();
    assert_eq!(decompress_tuned(&c).unwrap(), sample_text());
}

// --- dialogue tables (16-byte header, compressed length at 12..16) -------------

#[test]
fn dialogue_bomb_stops_at_the_fixed_limit() {
    let c = DialogueCompressor::new(DialogueCompressionMode::Fast);
    let mut table = DialogueTable::new();
    let s = table.speakers.insert("narrator");
    table.add(DialogueEntry {
        id: 0,
        speaker: s,
        text: "hello".to_string(),
        ruby: None,
    });
    let valid = c.compress_table(&table).unwrap();
    assert_eq!(c.decompress_table(&valid).unwrap().len(), 1);

    let bomb = zeros_frame(UNRECORDED_LIMIT + (1 << 20));
    let mut data = valid[..16].to_vec();
    data[12..16].copy_from_slice(&(bomb.len() as u32).to_le_bytes());
    data.extend_from_slice(&bomb);
    assert!(is_err_with(
        &c.decompress_table(&data),
        "expands past 268435456"
    ));
    data[10] = 0x02; // the same bytes as a localization table
    assert!(is_err_with(
        &c.decompress_localization(&data),
        "expands past 268435456"
    ));
}

/// The recorded size comes from the input: a size above the limit of one decode (256 MiB, the same as for streams
/// without a recorded size) is refused before the column is decoded, whatever the stream holds
#[test]
fn v3_recorded_size_above_the_limit_is_refused_before_decoding() {
    let mut compared = 0;
    for recorded in [UNRECORDED_LIMIT as u32 + 1, 300 << 20, u32::MAX] {
        let (mut data, i, _, _) = v3_with_last_column();
        entry_field(&mut data, i, UNCOMPRESSED_AT, recorded);
        let r = decompress_v3(&data);
        assert!(
            is_err_with(&r, "above the limit of one decode"),
            "{recorded}: {r:?}"
        );
        compared += 1;
    }
    // exactly the limit is not refused for its size (it fails later, as the stream is far shorter)
    let (mut data, i, _, _) = v3_with_last_column();
    entry_field(&mut data, i, UNCOMPRESSED_AT, UNRECORDED_LIMIT as u32);
    let r = decompress_v3(&data);
    assert!(!is_err_with(&r, "above the limit of one decode"), "{r:?}");
    assert_eq!(compared, 3);
}

/// Patches every column's recorded size to `size` (the streams are unchanged)
fn v3_with_every_recorded_size(size: u32) -> Vec<u8> {
    let mut data = compress_v3(&sample_text(), CompressionLevel::Balanced).unwrap();
    let n = FormatV3Metadata::read_from(&mut Cursor::new(&data))
        .unwrap()
        .columns
        .len();
    for i in 0..n {
        entry_field(&mut data, i, UNCOMPRESSED_AT, size);
    }
    data
}

/// The recorded sizes are each within the limit of one decode, but together they ask for more than the file's limit
/// (1 GiB by default): refused before any column is decoded
#[test]
fn v3_recorded_sizes_that_sum_past_the_file_limit_are_refused_before_decoding() {
    let n = FormatV3Metadata::read_from(&mut Cursor::new(
        &compress_v3(&sample_text(), CompressionLevel::Balanced).unwrap(),
    ))
    .unwrap()
    .columns
    .len() as u64;
    let per = UNRECORDED_LIMIT as u32; // each column at the per-column limit
    assert!(
        n * u64::from(per) > 1 << 30,
        "the sample has enough columns: {n}"
    );
    let r = decompress_v3(&v3_with_every_recorded_size(per));
    assert!(is_err_with(&r, "above the file's limit"), "{r:?}");
}

#[test]
fn v3_the_file_limit_is_the_callers() {
    // exactly the recorded total reads back; one byte less is refused before decoding (shown with a corrupted stream,
    // which a decode would report as a Zstd error)
    let data = compress_v3(&sample_text(), CompressionLevel::Balanced).unwrap();
    let meta = FormatV3Metadata::read_from(&mut Cursor::new(&data)).unwrap();
    let total: u64 = meta
        .columns
        .iter()
        .map(|e| u64::from(e.uncompressed_size))
        .sum();
    assert_eq!(
        decompress_v3_with_limit(&data, total).unwrap(),
        sample_text()
    );
    let mut corrupt = data;
    let first = meta.columns.iter().min_by_key(|e| e.offset).unwrap();
    corrupt[first.offset as usize] ^= 0xFF;
    let r = decompress_v3_with_limit(&corrupt, total - 1);
    assert!(is_err_with(&r, "above the file's limit"), "{r:?}");
}

#[test]
fn v3_columns_without_recorded_sizes_share_the_file_limit() {
    // earlier versions recorded no sizes (0): what is left of the file's limit bounds each such column as it decodes
    let data = v3_with_every_recorded_size(0);
    let text_len = sample_text().len() as u64;
    let r = decompress_v3_with_limit(&data, 16);
    assert!(is_err_with(&r, "what is left of the file's limit"), "{r:?}");
    assert_eq!(
        decompress_v3_with_limit(&data, 64 * text_len).unwrap(),
        sample_text()
    );
}
