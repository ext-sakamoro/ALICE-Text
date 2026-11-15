//! Fixtures of the bincode 1 bytes this crate stores, captured from bincode 1.3.3
//!
//! `tests/fixtures/bincode1/` holds, for the fixed inputs below, the payloads this crate
//! serializes (the v2 payload, each v3 column, a dialogue table, a localization table with
//! its maps in key order) and whole stored files of each format. They were written by
//! bincode 1.3.3 before this crate stopped depending on it, so the tests below pin that
//! the bytes written now are the bytes bincode wrote, and that files bincode wrote still
//! decode. `SHA256SUMS` names every file.
//!
//! `localization_ordered_file.bin` is the stored localization file written after the maps
//! became ordered (its payload is `localization_sorted.bin`); `localization_file.bin` was
//! written with the maps in hash order and is only read. A change of the zstd library may
//! change the whole-file fixtures (`*_file.bin`), which hold zstd streams; the payload
//! fixtures do not depend on it.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::columnar_encoder::{ColumnarEncoder, ColumnarPayload};
use crate::dialogue::{
    DialogueCompressionMode, DialogueCompressor, DialogueEntry, DialogueTable, LocaleId,
    LocalizationTable, RubyAnnotation,
};
use crate::format_v3::CompressionLevel;
use crate::query_engine::{compress_v3, decompress_v3};
use crate::tuned_compressor::{compress_tuned, decompress_tuned, CompressionMode};

/// A log that fills every v3 column: timestamps (and one that does not render back), IPv4
/// and IPv6, log levels, numbers, UUIDs, emails, URLs, paths, dates and times (parsed, and
/// impossible ones kept as text), hex values and other text.
pub const SAMPLE: &str = "\
2024-01-15 10:30:45 INFO request 17 from 192.0.2.1 accepted in 12.5 ms
2024-01-15 10:30:46 WARN retry 3 for 198.51.100.20 id 550e8400-e29b-41d4-a716-446655440000
2024-01-15 10:30:47 ERROR user ops@example.org at https://example.org/api/v1?x=1 failed
2024-01-15 10:30:48 DEBUG path /var/log/app/server.log size 4096 hash 0xdeadbeef
2024-01-15 10:30:49 INFO peer 2001:db8::1 on 2024-02-29 at 23:59:59 code 0x1F
2024-01-15 10:30:50 INFO request 18 from 192.0.2.2 accepted in 7.25 ms
2024-01-15 10:31:02 TRACE value -42 ratio 0.125 file C:\\data\\run.txt
2024-01-15T10:31:03.250+09:00 INFO peer fe80::1ff:fe23:4567:890a due 15/01/2024 at 10:30
2024-01-15 10:31:04 INFO host 2001:DB8::FF on 2024/03/01 at 7:05:09 ref ZX-99
2024-01-15 10:31:05 INFO peer 2001:db8:1:2:3:4:5:6 batch 2024-02-30 window 25:61:07
";

pub fn payload() -> ColumnarPayload {
    ColumnarEncoder::new().encode(SAMPLE)
}

fn entry(id: u32, speaker: u16, text: &str, ruby: Option<Vec<RubyAnnotation>>) -> DialogueEntry {
    DialogueEntry {
        id,
        speaker,
        text: text.to_string(),
        ruby,
    }
}

pub fn dialogue() -> DialogueTable {
    let mut t = DialogueTable::new();
    let narrator = t.speakers.insert("narrator");
    let alice = t.speakers.insert("アリス");
    t.add(entry(0, narrator, "The door opens.", None));
    t.add(entry(1, alice, "こんにちは", None));
    t.add(entry(2, alice, "Again?", None));
    t.add(entry(5, narrator, "", None));
    t
}

/// A localization table with three locales and several overrides each, so a map written
/// in hash order differs between runs.
pub fn localization() -> LocalizationTable {
    let mut l = LocalizationTable::new(LocaleId(0));
    l.base_table = dialogue();
    for (locale, words) in [(1u16, "Hallo"), (2, "Bonjour"), (7, "Hola")] {
        for id in [0u32, 1, 2, 5, 9, 11] {
            l.add_delta(
                LocaleId(locale),
                entry(id, 0, &format!("{words} {id}"), None),
            );
        }
    }
    l
}

/// The bytes the writer stores for each v3 column, in the writer's order.
pub fn v3_columns(p: &ColumnarPayload) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("skeleton", enc(&p.skeleton_tokens)),
        ("placeholders", enc(&p.placeholder_map)),
        ("timestamps", enc(&p.timestamps)),
        ("ipv4", enc(&p.ipv4_addrs)),
        ("ipv6", enc(&p.ipv6_addrs)),
        ("log_levels", enc(&p.log_levels)),
        ("numbers", enc(&p.numbers)),
        ("uuids", enc(&p.uuids)),
        ("emails", enc(&p.emails)),
        ("urls", enc(&p.urls)),
        ("paths", enc(&p.paths)),
        ("date_days", enc(&p.date_days)),
        ("dates", enc(&p.dates)),
        ("time_ms", enc(&p.time_ms)),
        ("times", enc(&p.times)),
        ("hex", enc(&p.hex_values)),
        ("others", enc(&p.others)),
        ("timestamps_raw", enc(&p.timestamps.raw)),
    ]
}

/// The columns as one fixture: per column a `u32` LE length, then its bytes.
fn v3_columns_blob(p: &ColumnarPayload) -> Vec<u8> {
    let mut out = Vec::new();
    for (_, bytes) in v3_columns(p) {
        out.extend_from_slice(&u32::try_from(bytes.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&bytes);
    }
    out
}

/// The encoding the crate stores with (`crate::wire`, the bincode 1 layout).
fn enc<T: Serialize + ?Sized>(v: &T) -> Vec<u8> {
    crate::wire::encode(v).expect("encode")
}

/// The localization table's bytes: its maps are ordered, so it writes them in key order,
/// the bytes bincode wrote for the same table with its maps sorted.
fn localization_sorted_bytes(l: &LocalizationTable) -> Vec<u8> {
    enc(l)
}

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bincode1")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn compressor() -> DialogueCompressor {
    DialogueCompressor::new(DialogueCompressionMode::Balanced)
}

/// Every fixture: (file name, the bytes this build produces for it, or `None` for a file
/// that is only read: the stored localization file, whose maps were written in hash order).
fn fixtures() -> Vec<(&'static str, Option<Vec<u8>>)> {
    let p = payload();
    vec![
        ("v2_payload.bin", Some(enc(&p))),
        ("v3_columns.bin", Some(v3_columns_blob(&p))),
        ("dialogue_table.bin", Some(enc(&dialogue()))),
        (
            "localization_sorted.bin",
            Some(localization_sorted_bytes(&localization())),
        ),
        (
            "v2_file.bin",
            Some(compress_tuned(SAMPLE, CompressionMode::Balanced).unwrap()),
        ),
        (
            "v3_file.bin",
            Some(compress_v3(SAMPLE, CompressionLevel::Balanced).unwrap()),
        ),
        (
            "dialogue_file.bin",
            Some(compressor().compress_table(&dialogue()).unwrap()),
        ),
        ("localization_file.bin", None),
        (
            "localization_ordered_file.bin",
            Some(compressor().compress_localization(&localization()).unwrap()),
        ),
    ]
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    Sha256::digest(b).iter().fold(String::new(), |mut s, x| {
        let _ = write!(s, "{x:02x}");
        s
    })
}

#[test]
fn every_fixture_matches_its_recorded_sha256() {
    let sums = std::fs::read_to_string(dir().join("SHA256SUMS")).unwrap();
    let mut checked = 0;
    for line in sums.lines() {
        let (hash, name) = line.split_once("  ").expect("`<sha256>  <name>`");
        assert_eq!(sha256_hex(&read(name)), hash, "{name}");
        checked += 1;
    }
    assert_eq!(checked, fixtures().len(), "SHA256SUMS lists every fixture");
}

#[test]
fn the_bytes_written_now_are_the_bytes_bincode_wrote() {
    let mut compared = 0;
    for (name, bytes) in fixtures() {
        if let Some(bytes) = bytes {
            assert!(bytes == read(name), "{name} differs from its fixture");
            compared += 1;
        }
    }
    assert_eq!(compared, 8);
}

#[test]
fn files_bincode_wrote_still_decode() {
    assert_eq!(decompress_tuned(&read("v2_file.bin")).unwrap(), SAMPLE);
    assert_eq!(decompress_v3(&read("v3_file.bin")).unwrap(), SAMPLE);
    let d = compressor()
        .decompress_table(&read("dialogue_file.bin"))
        .unwrap();
    assert!(enc(&d) == read("dialogue_table.bin"));
    // written in hash order: compared by content, through the key-ordered bytes
    let l = compressor()
        .decompress_localization(&read("localization_file.bin"))
        .unwrap();
    assert!(localization_sorted_bytes(&l) == read("localization_sorted.bin"));
    assert_eq!(l.locale_deltas.len(), 3);
    // the file written now (maps in key order) holds the same table
    let o = compressor()
        .decompress_localization(&read("localization_ordered_file.bin"))
        .unwrap();
    assert!(enc(&o) == read("localization_sorted.bin"));
}

#[test]
fn the_sample_fills_the_columns_it_is_meant_to() {
    let empty: Vec<&str> = v3_columns(&payload())
        .into_iter()
        .filter(|(_, b)| b.len() <= 8)
        .map(|(n, _)| n)
        .collect();
    // an empty column is only its 8-byte count: its layout would not be pinned
    assert!(
        empty.is_empty(),
        "v3 columns the sample leaves empty: {empty:?}"
    );
}

/// Every format's stored file for the fixed inputs, written now.
fn stored_files_now() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            "v2",
            compress_tuned(SAMPLE, CompressionMode::Balanced).unwrap(),
        ),
        (
            "v3",
            compress_v3(SAMPLE, CompressionLevel::Balanced).unwrap(),
        ),
        (
            "dialogue",
            compressor().compress_table(&dialogue()).unwrap(),
        ),
        (
            "localization",
            compressor().compress_localization(&localization()).unwrap(),
        ),
    ]
}

#[test]
fn compressing_the_same_input_again_gives_the_same_bytes() {
    // each round builds its tables afresh, so a map in hash order would come out in a
    // different order in some round
    let first = stored_files_now();
    for round in 0..8 {
        for ((name, a), (_, b)) in first.iter().zip(stored_files_now()) {
            assert!(*a == b, "{name}: round {round} wrote different bytes");
        }
    }
}

const CHILD_OUT: &str = "ALICE_TEXT_DETERMINISM_OUT";

#[test]
fn another_process_writes_the_same_bytes() {
    let files = stored_files_now();
    if let Some(out) = std::env::var_os(CHILD_OUT) {
        // the child: write what this process produced and stop
        let mut all = Vec::new();
        for (_, b) in &files {
            all.extend_from_slice(&u32::try_from(b.len()).unwrap().to_le_bytes());
            all.extend_from_slice(b);
        }
        std::fs::write(out, all).unwrap();
        return;
    }
    let out =
        std::env::temp_dir().join(format!("alice-text-determinism-{}.bin", std::process::id()));
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "bincode1_fixtures::another_process_writes_the_same_bytes",
            "--exact",
            "--test-threads=1",
        ])
        .env(CHILD_OUT, &out)
        .status()
        .unwrap();
    assert!(status.success(), "child process failed");
    let theirs = std::fs::read(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    let mut at = 0;
    for (name, mine) in &files {
        let len = u32::from_le_bytes(theirs[at..at + 4].try_into().unwrap()) as usize;
        at += 4;
        assert!(
            theirs[at..at + len] == mine[..],
            "{name}: the other process wrote different bytes"
        );
        at += len;
    }
    assert_eq!(at, theirs.len());
}

#[test]
fn a_stored_payload_with_bytes_after_it_is_refused() {
    // each stream holds exactly one payload: bincode 1 ignored trailing bytes, this does not
    let mut bytes = read("v2_payload.bin");
    assert!(crate::wire::decode::<ColumnarPayload>(&bytes).is_ok());
    bytes.push(0);
    assert!(matches!(
        crate::wire::decode::<ColumnarPayload>(&bytes),
        Err(alice_serial::bincode1::Bincode1Error::TrailingBytes { .. })
    ));
}
