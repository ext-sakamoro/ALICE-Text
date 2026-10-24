//! Unicode 16.0.0 conformance of the normalization forms and of case folding, against the Unicode Consortium's own
//! test data (checked in under data/ucd/16.0.0/, hashes in SHA256SUMS).
//!
//! - NormalizationTest.txt, every part: for each line c1;c2;c3;c4;c5 the invariants of UAX #15 (NFC(c1..c3) = c2,
//!   NFC(c4..c5) = c4, NFD(c1..c3) = c3, NFD(c4..c5) = c5, NFKC(c1..c5) = c4, NFKD(c1..c5) = c5), and for every code
//!   point not listed in part 1, X = NFC(X) = NFD(X) = NFKC(X) = NFKD(X).
//! - CaseFolding.txt: status C and F give the full folding, C and S the simple folding; every other code point folds
//!   to itself.

use alice_text::unicode_norm::{
    case_fold, case_fold_simple, to_nfc, to_nfd, to_nfkc, to_nfkd, NORMALIZATION_SEMANTICS_ID, UNICODE_VERSION,
};
use std::collections::{BTreeMap, BTreeSet};

const NORMALIZATION_TEST: &str = include_str!("../data/ucd/16.0.0/NormalizationTest.txt");
const CASE_FOLDING: &str = include_str!("../data/ucd/16.0.0/CaseFolding.txt");

fn chars(field: &str) -> String {
    field
        .split_whitespace()
        .map(|h| char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap())
        .collect()
}

fn show(s: &str) -> String {
    s.chars().map(|c| format!("{:04X}", c as u32)).collect::<Vec<_>>().join(" ")
}

/// Every unpaired-surrogate-free scalar value
fn scalars() -> impl Iterator<Item = char> {
    (0..=0x10_FFFFu32).filter_map(char::from_u32)
}

#[test]
fn the_data_is_unicode_16_0_0() {
    assert!(NORMALIZATION_TEST.starts_with("# NormalizationTest-16.0.0.txt"));
    assert!(CASE_FOLDING.starts_with("# CaseFolding-16.0.0.txt"));
    assert_eq!(UNICODE_VERSION, (16, 0, 0));
}

#[test]
fn every_line_of_normalization_test_holds() {
    let mut part = String::new();
    let mut lines_per_part: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    for line in NORMALIZATION_TEST.lines() {
        if let Some(p) = line.strip_prefix('@') {
            part = p.split_whitespace().next().unwrap().to_owned();
            continue;
        }
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let c: Vec<String> = line.split(';').take(5).map(chars).collect();
        *lines_per_part.entry(part.clone()).or_default() += 1;
        let checks = [
            ("NFC", 0, to_nfc(&c[0]), &c[1]),
            ("NFC", 1, to_nfc(&c[1]), &c[1]),
            ("NFC", 2, to_nfc(&c[2]), &c[1]),
            ("NFC", 3, to_nfc(&c[3]), &c[3]),
            ("NFC", 4, to_nfc(&c[4]), &c[3]),
            ("NFD", 0, to_nfd(&c[0]), &c[2]),
            ("NFD", 1, to_nfd(&c[1]), &c[2]),
            ("NFD", 2, to_nfd(&c[2]), &c[2]),
            ("NFD", 3, to_nfd(&c[3]), &c[4]),
            ("NFD", 4, to_nfd(&c[4]), &c[4]),
            ("NFKC", 0, to_nfkc(&c[0]), &c[3]),
            ("NFKC", 1, to_nfkc(&c[1]), &c[3]),
            ("NFKC", 2, to_nfkc(&c[2]), &c[3]),
            ("NFKC", 3, to_nfkc(&c[3]), &c[3]),
            ("NFKC", 4, to_nfkc(&c[4]), &c[3]),
            ("NFKD", 0, to_nfkd(&c[0]), &c[4]),
            ("NFKD", 1, to_nfkd(&c[1]), &c[4]),
            ("NFKD", 2, to_nfkd(&c[2]), &c[4]),
            ("NFKD", 3, to_nfkd(&c[3]), &c[4]),
            ("NFKD", 4, to_nfkd(&c[4]), &c[4]),
        ];
        for (form, col, got, want) in checks {
            if &got != want && failures.len() < 20 {
                failures.push(format!("{part} {form}(c{}) of {}: {} != {}", col + 1, show(&c[0]), show(&got), show(want)));
            }
        }
    }
    // the six parts of the 16.0.0 file (specific cases, character by character, canonical order, PRI #29,
    // canonical closures, chained primary composites), each non-empty (a parse that read nothing must not pass)
    assert_eq!(
        lines_per_part.keys().cloned().collect::<Vec<_>>(),
        ["Part0", "Part1", "Part2", "Part3", "Part4", "Part5"]
    );
    assert!(lines_per_part.values().all(|n| *n > 0), "{lines_per_part:?}");
    assert!(lines_per_part.values().sum::<usize>() > 19_000, "{lines_per_part:?}");
    assert!(failures.is_empty(), "{} failing:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn every_code_point_outside_part_1_is_invariant() {
    let mut in_part1 = BTreeSet::new();
    let mut part = "";
    for line in NORMALIZATION_TEST.lines() {
        if let Some(p) = line.strip_prefix('@') {
            part = if p.starts_with("Part1") { "Part1" } else { "" };
            continue;
        }
        if part == "Part1" {
            let f = line.split(';').next().unwrap().trim();
            if !f.is_empty() && !f.starts_with('#') {
                in_part1.insert(u32::from_str_radix(f, 16).unwrap());
            }
        }
    }
    assert!(in_part1.len() > 10_000, "{}", in_part1.len());
    let mut failures = Vec::new();
    let mut checked = 0usize;
    for ch in scalars().filter(|ch| !in_part1.contains(&(*ch as u32))) {
        let x = ch.to_string();
        checked += 1;
        for (form, got) in [("NFC", to_nfc(&x)), ("NFD", to_nfd(&x)), ("NFKC", to_nfkc(&x)), ("NFKD", to_nfkd(&x))] {
            if got != x && failures.len() < 20 {
                failures.push(format!("{form}({:04X}) = {}", ch as u32, show(&got)));
            }
        }
    }
    assert!(checked > 1_000_000, "{checked}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn case_folding_matches_every_line_and_leaves_the_rest_alone() {
    let mut full: BTreeMap<u32, String> = BTreeMap::new();
    let mut simple: BTreeMap<u32, String> = BTreeMap::new();
    for line in CASE_FOLDING.lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(';').map(str::trim).collect();
        let cp = u32::from_str_radix(f[0], 16).unwrap();
        let mapping = chars(f[2]);
        match f[1] {
            "C" => {
                full.insert(cp, mapping.clone());
                simple.insert(cp, mapping);
            }
            "F" => {
                full.insert(cp, mapping);
            }
            "S" => {
                simple.insert(cp, mapping);
            }
            "T" => {} // the Turkic mappings are not part of the default folding
            other => panic!("unknown status {other}"),
        }
    }
    assert!(full.len() > 1_400 && simple.len() > 1_400, "{} {}", full.len(), simple.len());
    let mut failures = Vec::new();
    for ch in scalars() {
        let x = ch.to_string();
        let want_full = full.get(&(ch as u32)).cloned().unwrap_or_else(|| x.clone());
        let want_simple = simple.get(&(ch as u32)).cloned().unwrap_or_else(|| x.clone());
        let (got_full, got_simple) = (case_fold(&x), case_fold_simple(&x));
        if got_full != want_full && failures.len() < 20 {
            failures.push(format!("full {:04X}: {} != {}", ch as u32, show(&got_full), show(&want_full)));
        }
        if got_simple != want_simple && failures.len() < 20 {
            failures.push(format!("simple {:04X}: {} != {}", ch as u32, show(&got_simple), show(&want_simple)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // folding is per character: a string folds to the concatenation of its characters' foldings
    assert_eq!(case_fold("Straße ǅ ΣΑΣ"), "strasse ǆ σασ");
}

#[test]
fn the_semantics_id_names_the_unicode_version_and_the_data() {
    // hashlib.sha256 of "alice-text unicode v1\nunicode 16.0.0\n" + data/ucd/16.0.0/SHA256SUMS (the 4 files' hashes)
    const ID_HEX: &str = "cac527945057ddc79ec9b270e3ee2c9c64350998d428a7c848e778f8ef4ca409";
    let sums = include_str!("../data/ucd/16.0.0/SHA256SUMS");
    assert_eq!(sums.lines().count(), 4, "{sums}");
    let want: Vec<u8> = (0..64).step_by(2).map(|i| u8::from_str_radix(&ID_HEX[i..i + 2], 16).unwrap()).collect();
    assert_eq!(NORMALIZATION_SEMANTICS_ID.to_vec(), want);
}
