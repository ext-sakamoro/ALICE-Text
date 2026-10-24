//! Unicode 正規化と case folding (Unicode 16.0.0、UAX #15 と Unicode §3.13)
//!
//! NFD / NFC / NFKD / NFKC と、既定の case folding (full と simple) を提供する 表は
//! `scripts/gen_unicode_tables.py` が `data/ucd/16.0.0/` の UCD から生成した `unicode_tables.rs` だけを
//! 読み、OS や標準 library の Unicode 表には依らない (全端末で同じ結果) 版と入力 data の hash は
//! [`UNICODE_VERSION`] と [`NORMALIZATION_SEMANTICS_ID`] に出る (版を上げれば id が変わる)
//!
//! 適合性は Unicode Consortium の NormalizationTest.txt (全 part) と CaseFolding.txt で試験する
//! (`tests/unicode_conformance.rs`)
//!
//! Author: Moroya Sakamoto

use crate::unicode_tables as t;
pub use crate::unicode_tables::{NORMALIZATION_SEMANTICS_ID, UNICODE_VERSION};

/// 正規化形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormForm {
    /// NFD: Canonical Decomposition (合成済み → 基底 + 結合文字)。
    Nfd,
    /// NFC: Canonical Composition (基底 + 結合文字 → 合成済み)。
    Nfc,
}

const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;

/// Canonical Combining Class (CCC) (表に無い文字は 0)
fn combining_class(c: char) -> u8 {
    let cp = c as u32;
    t::CCC
        .binary_search_by(|&(first, last, _)| {
            if last < cp {
                core::cmp::Ordering::Less
            } else if first > cp {
                core::cmp::Ordering::Greater
            } else {
                core::cmp::Ordering::Equal
            }
        })
        .map_or(0, |i| t::CCC[i].2)
}

/// 結合文字 (CCC > 0) かどうか。
#[inline]
#[must_use]
pub fn is_combining(c: char) -> bool {
    combining_class(c) > 0
}

/// `cp` の (offset, length) を `index` から引く
fn lookup(index: &[(u32, u16, u8)], cp: u32) -> Option<(usize, usize)> {
    index
        .binary_search_by_key(&cp, |&(c, _, _)| c)
        .ok()
        .map(|i| (usize::from(index[i].1), usize::from(index[i].2)))
}

/// 表の code point を足す (表は `UnicodeData` / `CaseFolding` の scalar value だけを持つ)
fn append_cp(out: &mut Vec<char>, cp: u32) {
    out.extend(char::from_u32(cp));
}

/// 1 文字の完全分解を `out` に足す (`compat` なら互換分解、Hangul 音節は算術で分解)
fn decompose_into(c: char, compat: bool, out: &mut Vec<char>) {
    let cp = c as u32;
    if (S_BASE..S_BASE + S_COUNT).contains(&cp) {
        let s = cp - S_BASE;
        append_cp(out, L_BASE + s / N_COUNT);
        append_cp(out, V_BASE + (s % N_COUNT) / T_COUNT);
        if !s.is_multiple_of(T_COUNT) {
            append_cp(out, T_BASE + s % T_COUNT);
        }
        return;
    }
    let found = if compat {
        lookup(t::COMPAT, cp)
            .map(|r| (t::COMPAT_DATA, r))
            .or_else(|| lookup(t::CANON, cp).map(|r| (t::CANON_DATA, r)))
    } else {
        lookup(t::CANON, cp).map(|r| (t::CANON_DATA, r))
    };
    match found {
        Some((data, (off, len))) => data[off..off + len].iter().for_each(|&x| append_cp(out, x)),
        None => out.extend([c]),
    }
}

/// 分解して、結合文字の並びを CCC の安定な順に並べる (Canonical Ordering)
fn decompose(input: &str, compat: bool) -> Vec<char> {
    let mut out = Vec::with_capacity(input.len());
    for c in input.chars() {
        decompose_into(c, compat, &mut out);
    }
    // 非 starter の連続区間ごとに CCC で安定 insertion sort
    let mut i = 1;
    while i < out.len() {
        let cc = combining_class(out[i]);
        if cc != 0 {
            let mut j = i;
            while j > 0 {
                let prev = combining_class(out[j - 1]);
                if prev == 0 || prev <= cc {
                    break;
                }
                out.swap(j - 1, j);
                j -= 1;
            }
        }
        i += 1;
    }
    out
}

/// 2 文字の primary composite (Hangul は算術)
fn compose_pair(a: char, b: char) -> Option<char> {
    let (a, b) = (a as u32, b as u32);
    if (L_BASE..L_BASE + L_COUNT).contains(&a) && (V_BASE..V_BASE + V_COUNT).contains(&b) {
        return char::from_u32(S_BASE + ((a - L_BASE) * V_COUNT + (b - V_BASE)) * T_COUNT);
    }
    if (S_BASE..S_BASE + S_COUNT).contains(&a)
        && (a - S_BASE).is_multiple_of(T_COUNT)
        && (T_BASE + 1..T_BASE + T_COUNT).contains(&b)
    {
        return char::from_u32(a + (b - T_BASE));
    }
    t::COMPOSE
        .binary_search_by_key(&(a, b), |&(x, y, _)| (x, y))
        .ok()
        .and_then(|i| char::from_u32(t::COMPOSE[i].2))
}

/// Canonical Composition (UAX #15 §3.11): 遮られていない文字を直前の starter と合成する
fn compose(chars: &[char]) -> String {
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut starter: Option<usize> = None;
    // starter の後に残した最後の文字の CCC (None = starter の直後)
    let mut last_class: Option<u8> = None;
    for &c in chars {
        let cc = combining_class(c);
        if let Some(si) = starter {
            let unblocked = last_class.is_none_or(|lc| lc != 0 && lc < cc);
            if unblocked {
                if let Some(composed) = compose_pair(out[si], c) {
                    out[si] = composed;
                    continue;
                }
            }
        }
        if cc == 0 {
            starter = Some(out.len());
            last_class = None;
        } else {
            last_class = Some(cc);
        }
        out.extend([c]);
    }
    out.into_iter().collect()
}

/// NFD (Canonical Decomposition)。
#[must_use]
pub fn to_nfd(input: &str) -> String {
    decompose(input, false).into_iter().collect()
}

/// NFC (Canonical Decomposition の後に Canonical Composition)。
#[must_use]
pub fn to_nfc(input: &str) -> String {
    compose(&decompose(input, false))
}

/// NFKD (Compatibility Decomposition)。全角・半角や合字は互換分解される (例: "Ａ" → "A")
#[must_use]
pub fn to_nfkd(input: &str) -> String {
    decompose(input, true).into_iter().collect()
}

/// NFKC (Compatibility Decomposition の後に Canonical Composition)。
#[must_use]
pub fn to_nfkc(input: &str) -> String {
    compose(&decompose(input, true))
}

/// 既定の full case folding (CaseFolding.txt の status C と F、Turkic の T は含めない) 例: "Straße" → "strasse"
#[must_use]
pub fn case_fold(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    for c in input.chars() {
        match lookup(t::FOLD_FULL, c as u32) {
            Some((off, len)) => t::FOLD_FULL_DATA[off..off + len]
                .iter()
                .for_each(|&x| append_cp(&mut out, x)),
            None => out.extend([c]),
        }
    }
    out.into_iter().collect()
}

/// 既定の simple case folding (status C と S、1 文字は 1 文字に)。
#[must_use]
pub fn case_fold_simple(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            t::FOLD_SIMPLE
                .binary_search_by_key(&(c as u32), |&(x, _)| x)
                .ok()
                .and_then(|i| char::from_u32(t::FOLD_SIMPLE[i].1))
                .unwrap_or(c)
        })
        .collect()
}

/// テキストが指定の正規化形式かどうかを判定。
#[must_use]
pub fn is_normalized(input: &str, form: NormForm) -> bool {
    match form {
        NormForm::Nfd => input == to_nfd(input),
        NormForm::Nfc => input == to_nfc(input),
    }
}

/// テキストが純 ASCII (0x00-0x7F) のみかどうかを判定。
///
/// ASCII のみのテキストは正規化不要。
#[inline]
#[must_use]
pub const fn is_ascii_only(input: &str) -> bool {
    input.is_ascii()
}

/// アクセント除去 (Strip Accents)。
///
/// NFD 分解後、結合文字を除去して基底文字のみを返す。
/// 例: "café" → "cafe", "naïve" → "naive"
#[must_use]
pub fn strip_accents(input: &str) -> String {
    let nfd = to_nfd(input);
    nfd.chars().filter(|c| combining_class(*c) == 0).collect()
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // --- ASCII 素通し ---

    #[test]
    fn ascii_passthrough_nfd() {
        let text = "Hello, World! 12345";
        assert_eq!(to_nfd(text), text);
    }

    #[test]
    fn ascii_passthrough_nfc() {
        let text = "Hello, World! 12345";
        assert_eq!(to_nfc(text), text);
    }

    #[test]
    fn ascii_is_normalized() {
        let text = "pure ASCII text 2024";
        assert!(is_normalized(text, NormForm::Nfd));
        assert!(is_normalized(text, NormForm::Nfc));
    }

    #[test]
    fn ascii_only_check() {
        assert!(is_ascii_only("hello"));
        assert!(!is_ascii_only("héllo"));
        assert!(!is_ascii_only("日本語"));
    }

    // --- NFD 分解 ---

    #[test]
    fn nfd_e_acute() {
        // é (U+00E9) → e + ◌́ (U+0301)
        let result = to_nfd("é");
        assert_eq!(result, "e\u{0301}");
    }

    #[test]
    fn nfd_cafe() {
        let result = to_nfd("café");
        assert_eq!(result, "cafe\u{0301}");
    }

    #[test]
    fn nfd_uppercase_accents() {
        // Ä → A + ◌̈
        let result = to_nfd("Ä");
        assert_eq!(result, "A\u{0308}");
    }

    #[test]
    fn nfd_n_tilde() {
        // ñ → n + ◌̃
        let result = to_nfd("señor");
        assert_eq!(result, "sen\u{0303}or");
    }

    #[test]
    fn nfd_cedilla() {
        // ç → c + ◌̧
        let result = to_nfd("ç");
        assert_eq!(result, "c\u{0327}");
    }

    #[test]
    fn nfd_ring_above() {
        // å → a + ◌̊
        let result = to_nfd("å");
        assert_eq!(result, "a\u{030A}");
    }

    #[test]
    fn nfd_already_decomposed() {
        // すでに分解済みの入力はそのまま
        let input = "e\u{0301}";
        let result = to_nfd(input);
        assert_eq!(result, input);
    }

    #[test]
    fn nfd_mixed_text() {
        let result = to_nfd("Héllo Wörld");
        assert_eq!(result, "He\u{0301}llo Wo\u{0308}rld");
    }

    // --- NFC 合成 ---

    #[test]
    fn nfc_compose_e_acute() {
        // e + ◌́ → é
        let input = "e\u{0301}";
        let result = to_nfc(input);
        assert_eq!(result, "é");
    }

    #[test]
    fn nfc_compose_cafe() {
        let input = "cafe\u{0301}";
        let result = to_nfc(input);
        assert_eq!(result, "café");
    }

    #[test]
    fn nfc_already_composed() {
        let input = "café";
        let result = to_nfc(input);
        assert_eq!(result, "café");
    }

    #[test]
    fn nfc_compose_uppercase() {
        let input = "A\u{0308}";
        let result = to_nfc(input);
        assert_eq!(result, "Ä");
    }

    #[test]
    fn nfc_compose_n_tilde() {
        let input = "n\u{0303}";
        let result = to_nfc(input);
        assert_eq!(result, "ñ");
    }

    // --- Roundtrip ---

    #[test]
    fn roundtrip_nfd_nfc() {
        let texts = ["café", "naïve", "Ñoño", "Ångström", "résumé", "über"];
        for text in texts {
            let nfd = to_nfd(text);
            let nfc = to_nfc(&nfd);
            assert_eq!(nfc, text, "roundtrip failed for {text}");
        }
    }

    #[test]
    fn roundtrip_nfc_nfd_nfc() {
        let input = "e\u{0301}";
        let nfc = to_nfc(input);
        let nfd = to_nfd(&nfc);
        let nfc2 = to_nfc(&nfd);
        assert_eq!(nfc, nfc2);
    }

    // --- is_normalized ---

    #[test]
    fn is_nfd_normalized() {
        let nfd = to_nfd("café");
        assert!(is_normalized(&nfd, NormForm::Nfd));
        assert!(!is_normalized(&nfd, NormForm::Nfc));
    }

    #[test]
    fn is_nfc_normalized() {
        let nfc = "café"; // NFC form
        assert!(is_normalized(nfc, NormForm::Nfc));
        assert!(!is_normalized(nfc, NormForm::Nfd));
    }

    // --- strip_accents ---

    #[test]
    fn strip_accents_basic() {
        assert_eq!(strip_accents("café"), "cafe");
        assert_eq!(strip_accents("naïve"), "naive");
        assert_eq!(strip_accents("résumé"), "resume");
    }

    #[test]
    fn strip_accents_complex() {
        assert_eq!(strip_accents("Ángström"), "Angstrom");
        assert_eq!(strip_accents("señor"), "senor");
        assert_eq!(strip_accents("über"), "uber");
    }

    #[test]
    fn strip_accents_ascii() {
        let text = "hello world";
        assert_eq!(strip_accents(text), text);
    }

    // --- Empty/Edge cases ---

    #[test]
    fn empty_string() {
        assert_eq!(to_nfd(""), "");
        assert_eq!(to_nfc(""), "");
        assert!(is_normalized("", NormForm::Nfd));
        assert!(is_normalized("", NormForm::Nfc));
        assert_eq!(strip_accents(""), "");
    }

    #[test]
    fn non_latin_passthrough() {
        // 日本語はテーブルに無いのでそのまま
        let text = "日本語テスト";
        assert_eq!(to_nfd(text), text);
        assert_eq!(to_nfc(text), text);
    }

    #[test]
    fn combining_class_values() {
        assert_eq!(combining_class('a'), 0);
        assert_eq!(combining_class('\u{0301}'), 230); // Acute
        assert_eq!(combining_class('\u{0327}'), 202); // Cedilla
        assert_eq!(combining_class('\u{0323}'), 220); // Below dot
    }

    #[test]
    fn is_combining_check() {
        assert!(!is_combining('a'));
        assert!(!is_combining('Z'));
        assert!(is_combining('\u{0301}')); // Acute
        assert!(is_combining('\u{0308}')); // Diaeresis
        assert!(is_combining('\u{0327}')); // Cedilla
    }

    // --- Latin Extended-A ---

    #[test]
    fn latin_extended_caron() {
        // č → c + ◌̌
        let result = to_nfd("č");
        assert_eq!(result, "c\u{030C}");
        assert_eq!(to_nfc(&result), "č");
    }

    #[test]
    fn latin_extended_macron() {
        // ā → a + ◌̄
        let result = to_nfd("ā");
        assert_eq!(result, "a\u{0304}");
        assert_eq!(to_nfc(&result), "ā");
    }

    #[test]
    fn latin_extended_breve() {
        // ă → a + ◌̆
        let result = to_nfd("ă");
        assert_eq!(result, "a\u{0306}");
        assert_eq!(to_nfc(&result), "ă");
    }

    // --- NormForm display ---

    #[test]
    fn norm_form_debug() {
        assert_eq!(format!("{:?}", NormForm::Nfd), "Nfd");
        assert_eq!(format!("{:?}", NormForm::Nfc), "Nfc");
    }

    #[test]
    fn norm_form_clone_eq() {
        let a = NormForm::Nfd;
        let b = a;
        assert_eq!(a, b);
        assert_ne!(NormForm::Nfd, NormForm::Nfc);
    }
}
