//! Unicode 16.0.0 word boundaries (UAX #29) against the Consortium's WordBreakTest.txt (checked in under
//! data/ucd/16.0.0/word-break/, hashes in its SHA256SUMS): every line's boundaries, and a semantics id that names
//! the Unicode version and the word-break data.

use alice_text::word_break::{boundaries, WORD_BREAK_SEMANTICS_ID};

const WORD_BREAK_TEST: &str = include_str!("../data/ucd/16.0.0/word-break/WordBreakTest.txt");

#[test]
fn the_data_is_unicode_16_0_0() {
    assert!(WORD_BREAK_TEST.starts_with("# WordBreakTest-16.0.0.txt"));
}

#[test]
fn every_line_of_word_break_test_holds() {
    let mut lines = 0usize;
    let mut failures = Vec::new();
    for line in WORD_BREAK_TEST.lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        // ÷ = a boundary, × = none, between the code points (and at both ends)
        let mut text = String::new();
        let mut want = Vec::new();
        for tok in line.split_whitespace() {
            match tok {
                "÷" => want.push(text.len()),
                "×" => {}
                hex => text.push(char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap()),
            }
        }
        lines += 1;
        let got = boundaries(&text);
        if got != want && failures.len() < 20 {
            let cps: Vec<String> = text.chars().map(|c| format!("{:04X}", c as u32)).collect();
            failures.push(format!("{}: got {got:?}, want {want:?}", cps.join(" ")));
        }
    }
    assert!(lines > 1_800, "{lines}");
    assert!(
        failures.is_empty(),
        "{} failing:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn an_empty_text_has_no_boundary_and_any_other_has_both_ends() {
    assert!(boundaries("").is_empty());
    let b = boundaries("ab cd");
    assert_eq!((b.first(), b.last()), (Some(&0), Some(&5)));
}

#[test]
fn the_semantics_id_names_the_unicode_version_and_the_word_break_data() {
    // hashlib.sha256 of "alice-text word break v1\nunicode 16.0.0\n" + data/ucd/16.0.0/word-break/SHA256SUMS
    const ID_HEX: &str = "bdfa53e73d11d2ffb340966b53f830b43050e04403bac9ba788623d4f8ec22bf";
    let sums = include_str!("../data/ucd/16.0.0/word-break/SHA256SUMS");
    assert_eq!(sums.lines().count(), 3, "{sums}");
    let want: Vec<u8> = (0..64)
        .step_by(2)
        .map(|i| u8::from_str_radix(&ID_HEX[i..i + 2], 16).unwrap())
        .collect();
    assert_eq!(WORD_BREAK_SEMANTICS_ID.to_vec(), want);
}
