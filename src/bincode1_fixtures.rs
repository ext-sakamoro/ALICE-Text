//! Fixtures of the bincode 1 bytes this crate stores, captured from bincode 1.3.3
//!
//! `tests/fixtures/bincode1/` holds, for the fixed inputs below, the payloads this crate
//! serializes (the v2 payload, each v3 column, a dialogue table, a localization table with
//! its maps in key order) and whole stored files of each format. They were written by
//! bincode 1.3.3 before this crate stopped depending on it, so the tests below pin that
//! the bytes written now are the bytes bincode wrote, and that files bincode wrote still
//! decode. `SHA256SUMS` names every file.
//!
//! A change of the zstd library may change the whole-file fixtures (`*_file.bin`), which
//! hold zstd streams; the payload fixtures do not depend on it.

use std::collections::BTreeMap;
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
pub(crate) const SAMPLE: &str = "\
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

pub(crate) fn payload() -> ColumnarPayload {
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

pub(crate) fn dialogue() -> DialogueTable {
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
pub(crate) fn localization() -> LocalizationTable {
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
pub(crate) fn v3_columns(p: &ColumnarPayload) -> Vec<(&'static str, Vec<u8>)> {
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

/// bincode 1.3.3 `serialize`, the encoding these fixtures were captured with.
fn enc<T: Serialize + ?Sized>(v: &T) -> Vec<u8> {
    bincode::serialize(v).expect("bincode serialize")
}

/// The localization table with both maps written in key order (what a table whose maps
/// are ordered writes).
fn localization_sorted_bytes(l: &LocalizationTable) -> Vec<u8> {
    #[derive(Serialize)]
    struct Delta<'a> {
        locale: LocaleId,
        entries: BTreeMap<u32, &'a DialogueEntry>,
        content_hash: u64,
    }
    #[derive(Serialize)]
    struct Table<'a> {
        base_locale: LocaleId,
        base_table: &'a DialogueTable,
        locale_deltas: BTreeMap<u16, Delta<'a>>,
    }
    let t = Table {
        base_locale: l.base_locale,
        base_table: &l.base_table,
        locale_deltas: l
            .locale_deltas
            .iter()
            .map(|(k, d)| {
                (
                    k.0,
                    Delta {
                        locale: d.locale,
                        entries: d.entries.iter().map(|(i, e)| (*i, e)).collect(),
                        content_hash: d.content_hash,
                    },
                )
            })
            .collect(),
    };
    enc(&t)
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
    ]
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// Writes the fixtures (run once with bincode 1.3.3:
/// `ALICE_TEXT_WRITE_BINCODE1_FIXTURES=1 cargo test --lib write_bincode1_fixtures -- --ignored`).
#[test]
#[ignore = "writes tests/fixtures/bincode1; run only to capture"]
fn write_bincode1_fixtures() {
    if std::env::var_os("ALICE_TEXT_WRITE_BINCODE1_FIXTURES").is_none() {
        return;
    }
    std::fs::create_dir_all(dir()).unwrap();
    let mut sums = String::new();
    for (name, bytes) in fixtures() {
        let bytes = bytes.unwrap_or_else(|| {
            compressor()
                .compress_localization(&localization())
                .unwrap()
        });
        std::fs::write(dir().join(name), &bytes).unwrap();
        sums.push_str(&format!("{}  {name}\n", sha256_hex(&bytes)));
    }
    std::fs::write(dir().join("SHA256SUMS"), sums).unwrap();
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
    assert_eq!(compared, 7);
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
}

#[test]
fn the_sample_fills_the_columns_it_is_meant_to() {
    let empty: Vec<&str> = v3_columns(&payload())
        .into_iter()
        .filter(|(_, b)| b.len() <= 8)
        .map(|(n, _)| n)
        .collect();
    // an empty column is only its 8-byte count: its layout would not be pinned
    assert!(empty.is_empty(), "v3 columns the sample leaves empty: {empty:?}");
}
