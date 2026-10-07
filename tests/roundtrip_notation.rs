//! Every input must come back byte for byte. The typed columns (numbers,
//! timestamps, dates, times, IP addresses, UUIDs, log levels) hold parsed
//! values, so a token whose spelling is not the canonical form of its value
//! (`007`, `1.050`, `WARNING`, an upper-case UUID …) must not be reduced to
//! that value.
//!
//! Each case is the exact text expected back; nothing is computed by the
//! library under test.

use alice_text::{
    compress_tuned, compress_v3, decompress_tuned, decompress_v3, ALICEText, ColumnarEncoder,
    CompressionLevel, CompressionMode, EncodingMode,
};

/// Spellings that a value-only column loses, grouped by the pattern that
/// matches them
const CASES: &[&str] = &[
    // numbers: leading zeros, trailing zeros, beyond f64 precision
    ":03",
    "007",
    "0",
    "00",
    "-05",
    "1.050",
    "1.0",
    "10.0",
    "0.5",
    "000.25",
    "12345678901234567890",
    "id=0042 retries=00",
    // timestamps: fractional seconds, offsets, mixed forms after the base
    "2024-01-05 03:04:05",
    "2024-01-15 10:30:45.120",
    "2024-01-15 10:30:45.000",
    "2024-01-15T10:30:45.5Z",
    "2024-01-15T10:30:45+09:00 then 2024-01-15 10:30:46",
    "2024-01-15 10:30:45 then 2024-01-15T10:30:46Z",
    "2024-01-15T10:30:45Z then 2024-01-15T10:30:46+09:00",
    // times
    "at 03:04:05.1",
    "at 03:04:05.000",
    "at 10:30:45.120",
    // dates
    "on 2024-01-05",
    "on 1969-12-31",
    // IP addresses
    "from 10.0.0.1",
    "from 010.000.000.001",
    "from 0:0:0:0:0:0:0:1",
    "from FE80:0000:0000:0000:0202:B3FF:FE1E:8329",
    // UUIDs
    "id 550E8400-E29B-41D4-A716-446655440000",
    "id 550e8400-e29b-41d4-a716-446655440000",
    // log levels
    "WARNING disk",
    "WARN disk",
];

fn lines() -> impl Iterator<Item = String> {
    // each case alone, inside a line, and repeated (later occurrences take the
    // delta / column paths that the first one does not)
    CASES.iter().flat_map(|c| {
        [
            (*c).to_string(),
            format!("x {c} y\n"),
            format!("{c}\n{c}\n"),
        ]
    })
}

#[test]
fn columnar_payload_restores_every_spelling() {
    let enc = ColumnarEncoder::new();
    for text in lines() {
        assert_eq!(enc.decode(&enc.encode(&text)), text, "input {text:?}");
    }
}

#[test]
fn v2_restores_every_spelling() {
    for text in lines() {
        let mut alice = ALICEText::new(EncodingMode::Pattern);
        let c = alice.compress(&text).unwrap();
        assert_eq!(alice.decompress(&c).unwrap(), text, "input {text:?}");
        let c = compress_tuned(&text, CompressionMode::Fast).unwrap();
        assert_eq!(decompress_tuned(&c).unwrap(), text, "input {text:?}");
    }
}

#[test]
fn v3_restores_every_spelling() {
    for text in lines() {
        let c = compress_v3(&text, CompressionLevel::Fast).unwrap();
        assert_eq!(decompress_v3(&c).unwrap(), text, "input {text:?}");
    }
}

#[test]
fn all_cases_in_one_text() {
    let text: String = CASES.iter().flat_map(|c| [c, "\n"]).collect();
    let enc = ColumnarEncoder::new();
    assert_eq!(enc.decode(&enc.encode(&text)), text);
    let c = compress_v3(&text, CompressionLevel::Fast).unwrap();
    assert_eq!(decompress_v3(&c).unwrap(), text);
}
