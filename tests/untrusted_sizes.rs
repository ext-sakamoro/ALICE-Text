//! Sizes and offsets declared inside untrusted input are checked against the
//! input before anything is allocated or read.
//!
//! Layout used below (from `format_v3`): magic (8) + version (2) + header (32),
//! then one 21-byte column entry per column: type (1), offset (8, LE),
//! compressed size (4, LE), uncompressed size (4), row count (4).

use alice_text::format_v3::{
    ColumnEntry, ColumnType, FormatV3Header, FormatV3Metadata, FormatV3Writer,
};
use alice_text::{compress_v3, decompress_v3, ALICETextError, CompressionLevel};
use alice_text::{DialogueCompressionMode, DialogueCompressor, DialogueEntry, DialogueTable};
use std::fmt::Write as _;
use std::io::Cursor;

const DIR_START: usize = 8 + 2 + FormatV3Header::SIZE;

/// Input found by `fuzz_decompress_v3`: one column entry whose offset
/// (0x6868…) and compressed size (0x68686868 ≈ 1.75 GB) lie far beyond the
/// 89 bytes of input; the decoder allocated the declared size before reading
const FUZZ_OOM_INPUT: [u8; 89] = [
    0x41, 0x4c, 0x49, 0x43, 0x45, 0x54, 0x58, 0x54, 0x03, 0x03, 0x03, 0x41, 0x4c, 0x49, 0x43, 0x45,
    0x54, 0x58, 0x54, 0x01, 0x00, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x01, 0x00, 0x00, 0xff, 0xff,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x68, 0x68, 0x68, 0x68,
    0x68, 0x68, 0x68, 0x68, 0x68, 0x68, 0x68, 0x68, 0x68, 0x68, 0x68, 0x68, 0x68, 0x68, 0x68, 0x68,
    0x00, 0x00, 0x00, 0x00, 0x15, 0x00, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x41, 0x4c, 0x00, 0x00, 0x49,
];

fn is_size_error(r: &Result<String, ALICETextError>) -> bool {
    matches!(r, Err(ALICETextError::DecompressionError(m)) if m.contains("exceeds the input"))
}

fn sample_v3() -> (String, Vec<u8>) {
    let text = (0..40).fold(String::new(), |mut t, i| {
        let _ = writeln!(
            t,
            "2024-01-15 10:30:{:02} INFO request {i} from 10.0.0.{i}",
            i % 60
        );
        t
    });
    let data = compress_v3(&text, CompressionLevel::Balanced).unwrap();
    (text, data)
}

/// (index, offset) of the column whose data ends the file
fn last_column(data: &[u8]) -> (usize, u64) {
    let meta = FormatV3Metadata::read_from(&mut Cursor::new(data)).unwrap();
    let (i, e) = meta
        .columns
        .iter()
        .enumerate()
        .max_by_key(|(_, e)| e.offset)
        .unwrap();
    assert_eq!(
        e.offset + u64::from(e.compressed_size),
        data.len() as u64,
        "the writer places the last column at the end of the file"
    );
    (i, e.offset)
}

fn set_size(data: &mut [u8], index: usize, size: u32) {
    let at = DIR_START + index * ColumnEntry::SIZE + 9;
    data[at..at + 4].copy_from_slice(&size.to_le_bytes());
}

fn set_offset(data: &mut [u8], index: usize, offset: u64) {
    let at = DIR_START + index * ColumnEntry::SIZE + 1;
    data[at..at + 8].copy_from_slice(&offset.to_le_bytes());
}

#[test]
fn fuzz_input_is_rejected_by_the_size_check() {
    assert!(is_size_error(&decompress_v3(&FUZZ_OOM_INPUT)));
}

#[test]
fn size_equal_to_the_remaining_input_decodes() {
    let (text, mut data) = sample_v3();
    let (i, offset) = last_column(&data);
    let remaining = u32::try_from(data.len() as u64 - offset).unwrap();
    set_size(&mut data, i, remaining); // unchanged value, written explicitly
    assert_eq!(decompress_v3(&data).unwrap(), text);
}

#[test]
fn size_one_past_the_remaining_input_is_rejected() {
    let (_, mut data) = sample_v3();
    let (i, offset) = last_column(&data);
    let remaining = u32::try_from(data.len() as u64 - offset).unwrap();
    set_size(&mut data, i, remaining + 1);
    assert!(is_size_error(&decompress_v3(&data)));
}

#[test]
fn largest_declared_size_is_rejected() {
    let (_, mut data) = sample_v3();
    set_size(&mut data, 0, u32::MAX);
    assert!(is_size_error(&decompress_v3(&data)));
}

#[test]
fn offset_past_the_end_is_rejected() {
    let (_, mut data) = sample_v3();
    let end = data.len() as u64;
    set_offset(&mut data, 0, end + 1);
    assert!(is_size_error(&decompress_v3(&data)));
    set_offset(&mut data, 0, u64::MAX);
    assert!(is_size_error(&decompress_v3(&data)));
}

#[test]
fn selective_read_checks_the_size_too() {
    let (_, mut data) = sample_v3();
    let meta = FormatV3Metadata::read_from(&mut Cursor::new(&data)).unwrap();
    let i = meta
        .columns
        .iter()
        .position(|e| e.col_type == ColumnType::LogLevels)
        .unwrap();
    // valid: the column reads
    let ok = FormatV3Writer::read_columns(&mut Cursor::new(&data), &meta, &[ColumnType::LogLevels]);
    assert!(ok.unwrap().log_levels.is_some());
    // declared size past the end of the input
    let offset = meta.columns[i].offset;
    let past = u32::try_from(data.len() as u64 - offset).unwrap() + 1;
    set_size(&mut data, i, past);
    let meta = FormatV3Metadata::read_from(&mut Cursor::new(&data)).unwrap();
    let r = FormatV3Writer::read_columns(&mut Cursor::new(&data), &meta, &[ColumnType::LogLevels]);
    assert!(
        matches!(r, Err(ALICETextError::DecompressionError(m)) if m.contains("exceeds the input"))
    );
}

// --- dialogue tables: 16-byte header, compressed length at bytes 12..16 ---

fn sample_dialogue() -> (DialogueCompressor, Vec<u8>) {
    let c = DialogueCompressor::new(DialogueCompressionMode::Balanced);
    let mut table = DialogueTable::new();
    let s = table.speakers.insert("narrator");
    table.add(DialogueEntry {
        id: 0,
        speaker: s,
        text: "hello".to_string(),
        ruby: None,
    });
    let data = c.compress_table(&table).unwrap();
    (c, data)
}

fn set_dialogue_len(data: &mut [u8], len: u32) {
    data[12..16].copy_from_slice(&len.to_le_bytes());
}

#[test]
fn dialogue_length_equal_to_the_remaining_input_decodes() {
    let (c, data) = sample_dialogue();
    let declared = u32::from_le_bytes(data[12..16].try_into().unwrap());
    assert_eq!(declared as usize, data.len() - 16);
    assert_eq!(c.decompress_table(&data).unwrap().len(), 1);
}

#[test]
fn dialogue_length_past_the_input_is_rejected() {
    let (c, mut data) = sample_dialogue();
    let past = u32::try_from(data.len() - 16 + 1).unwrap();
    set_dialogue_len(&mut data, past);
    assert!(c.decompress_table(&data).is_err());
    set_dialogue_len(&mut data, u32::MAX);
    assert!(c.decompress_table(&data).is_err());
}
