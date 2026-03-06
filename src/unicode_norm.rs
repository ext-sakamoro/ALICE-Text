//! Unicode 正規化モジュール
//!
//! NFD (Canonical Decomposition) と NFC (Canonical Composition) を提供する。
//! 圧縮前にテキストを正規化することで、同一内容の異なるバイト表現による
//! 圧縮効率の低下を防ぐ。
//!
//! # 対応範囲
//!
//! - ASCII 素通し (0x00-0x7F)
//! - ラテン文字アクセント分解 / 合成 (Latin-1 Supplement, Latin Extended-A)
//! - 合成済み文字 ↔ 基底文字 + 結合文字の変換
//!
//! Author: Moroya Sakamoto

/// 正規化形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormForm {
    /// NFD: Canonical Decomposition (合成済み → 基底 + 結合文字)。
    Nfd,
    /// NFC: Canonical Composition (基底 + 結合文字 → 合成済み)。
    Nfc,
}

/// 分解テーブルエントリ: (合成済み文字, 基底文字, 結合文字)。
const DECOMPOSITION_TABLE: &[(char, char, char)] = &[
    // Latin-1 Supplement: grave, acute, circumflex, tilde, diaeresis, ring, cedilla
    ('\u{00C0}', 'A', '\u{0300}'), // À
    ('\u{00C1}', 'A', '\u{0301}'), // Á
    ('\u{00C2}', 'A', '\u{0302}'), // Â
    ('\u{00C3}', 'A', '\u{0303}'), // Ã
    ('\u{00C4}', 'A', '\u{0308}'), // Ä
    ('\u{00C5}', 'A', '\u{030A}'), // Å
    ('\u{00C7}', 'C', '\u{0327}'), // Ç
    ('\u{00C8}', 'E', '\u{0300}'), // È
    ('\u{00C9}', 'E', '\u{0301}'), // É
    ('\u{00CA}', 'E', '\u{0302}'), // Ê
    ('\u{00CB}', 'E', '\u{0308}'), // Ë
    ('\u{00CC}', 'I', '\u{0300}'), // Ì
    ('\u{00CD}', 'I', '\u{0301}'), // Í
    ('\u{00CE}', 'I', '\u{0302}'), // Î
    ('\u{00CF}', 'I', '\u{0308}'), // Ï
    ('\u{00D1}', 'N', '\u{0303}'), // Ñ
    ('\u{00D2}', 'O', '\u{0300}'), // Ò
    ('\u{00D3}', 'O', '\u{0301}'), // Ó
    ('\u{00D4}', 'O', '\u{0302}'), // Ô
    ('\u{00D5}', 'O', '\u{0303}'), // Õ
    ('\u{00D6}', 'O', '\u{0308}'), // Ö
    ('\u{00D9}', 'U', '\u{0300}'), // Ù
    ('\u{00DA}', 'U', '\u{0301}'), // Ú
    ('\u{00DB}', 'U', '\u{0302}'), // Û
    ('\u{00DC}', 'U', '\u{0308}'), // Ü
    ('\u{00DD}', 'Y', '\u{0301}'), // Ý
    ('\u{00E0}', 'a', '\u{0300}'), // à
    ('\u{00E1}', 'a', '\u{0301}'), // á
    ('\u{00E2}', 'a', '\u{0302}'), // â
    ('\u{00E3}', 'a', '\u{0303}'), // ã
    ('\u{00E4}', 'a', '\u{0308}'), // ä
    ('\u{00E5}', 'a', '\u{030A}'), // å
    ('\u{00E7}', 'c', '\u{0327}'), // ç
    ('\u{00E8}', 'e', '\u{0300}'), // è
    ('\u{00E9}', 'e', '\u{0301}'), // é
    ('\u{00EA}', 'e', '\u{0302}'), // ê
    ('\u{00EB}', 'e', '\u{0308}'), // ë
    ('\u{00EC}', 'i', '\u{0300}'), // ì
    ('\u{00ED}', 'i', '\u{0301}'), // í
    ('\u{00EE}', 'i', '\u{0302}'), // î
    ('\u{00EF}', 'i', '\u{0308}'), // ï
    ('\u{00F1}', 'n', '\u{0303}'), // ñ
    ('\u{00F2}', 'o', '\u{0300}'), // ò
    ('\u{00F3}', 'o', '\u{0301}'), // ó
    ('\u{00F4}', 'o', '\u{0302}'), // ô
    ('\u{00F5}', 'o', '\u{0303}'), // õ
    ('\u{00F6}', 'o', '\u{0308}'), // ö
    ('\u{00F9}', 'u', '\u{0300}'), // ù
    ('\u{00FA}', 'u', '\u{0301}'), // ú
    ('\u{00FB}', 'u', '\u{0302}'), // û
    ('\u{00FC}', 'u', '\u{0308}'), // ü
    ('\u{00FD}', 'y', '\u{0301}'), // ý
    ('\u{00FF}', 'y', '\u{0308}'), // ÿ
    // Latin Extended-A (よく使うもの)
    ('\u{0100}', 'A', '\u{0304}'), // Ā
    ('\u{0101}', 'a', '\u{0304}'), // ā
    ('\u{0102}', 'A', '\u{0306}'), // Ă
    ('\u{0103}', 'a', '\u{0306}'), // ă
    ('\u{0106}', 'C', '\u{0301}'), // Ć
    ('\u{0107}', 'c', '\u{0301}'), // ć
    ('\u{010C}', 'C', '\u{030C}'), // Č
    ('\u{010D}', 'c', '\u{030C}'), // č
    ('\u{010E}', 'D', '\u{030C}'), // Ď
    ('\u{010F}', 'd', '\u{030C}'), // ď
    ('\u{0112}', 'E', '\u{0304}'), // Ē
    ('\u{0113}', 'e', '\u{0304}'), // ē
    ('\u{011A}', 'E', '\u{030C}'), // Ě
    ('\u{011B}', 'e', '\u{030C}'), // ě
    ('\u{011E}', 'G', '\u{0306}'), // Ğ
    ('\u{011F}', 'g', '\u{0306}'), // ğ
    ('\u{012A}', 'I', '\u{0304}'), // Ī
    ('\u{012B}', 'i', '\u{0304}'), // ī
    ('\u{0143}', 'N', '\u{0301}'), // Ń
    ('\u{0144}', 'n', '\u{0301}'), // ń
    ('\u{0147}', 'N', '\u{030C}'), // Ň
    ('\u{0148}', 'n', '\u{030C}'), // ň
    ('\u{014C}', 'O', '\u{0304}'), // Ō
    ('\u{014D}', 'o', '\u{0304}'), // ō
    ('\u{0158}', 'R', '\u{030C}'), // Ř
    ('\u{0159}', 'r', '\u{030C}'), // ř
    ('\u{015A}', 'S', '\u{0301}'), // Ś
    ('\u{015B}', 's', '\u{0301}'), // ś
    ('\u{0160}', 'S', '\u{030C}'), // Š
    ('\u{0161}', 's', '\u{030C}'), // š
    ('\u{016A}', 'U', '\u{0304}'), // Ū
    ('\u{016B}', 'u', '\u{0304}'), // ū
    ('\u{017D}', 'Z', '\u{030C}'), // Ž
    ('\u{017E}', 'z', '\u{030C}'), // ž
];

/// 結合文字の Canonical Combining Class (CCC)。
/// CCC > 0 の文字は結合文字。
#[inline]
const fn combining_class(c: char) -> u8 {
    match c {
        '\u{0300}'..='\u{0314}' => 230, // Above (grave, acute, circumflex, caron, etc.)
        '\u{0315}' | '\u{031A}' => 232, // Above right
        '\u{0316}'..='\u{0319}' | '\u{0323}'..='\u{0326}' | '\u{0330}'..='\u{0333}' => 220, // Below
        '\u{031B}' => 216,              // Attached above right (horn)
        '\u{0327}'..='\u{0328}' => 202, // Attached below (cedilla, ogonek)
        '\u{0338}' => 1,                // Overlay
        _ => 0,
    }
}

/// 結合文字かどうかを判定。
#[inline]
#[must_use]
pub const fn is_combining(c: char) -> bool {
    combining_class(c) > 0
}

/// 合成済み文字を NFD 分解する。分解不可の場合は `None`。
fn decompose_char(c: char) -> Option<(char, char)> {
    // ASCII は分解不要
    if c.is_ascii() {
        return None;
    }
    // テーブル検索
    for &(composed, base, combining) in DECOMPOSITION_TABLE {
        if composed == c {
            return Some((base, combining));
        }
    }
    None
}

/// 基底文字 + 結合文字を NFC 合成する。合成不可の場合は `None`。
fn compose_pair(base: char, combining: char) -> Option<char> {
    for &(composed, b, comb) in DECOMPOSITION_TABLE {
        if b == base && comb == combining {
            return Some(composed);
        }
    }
    None
}

/// テキストを NFD (Canonical Decomposition) に変換。
///
/// 合成済み文字を基底文字 + 結合文字に分解し、
/// 結合文字を Canonical Combining Class 順にソートする。
#[must_use]
pub fn to_nfd(input: &str) -> String {
    let mut result = String::with_capacity(input.len());

    for c in input.chars() {
        if let Some((base, combining)) = decompose_char(c) {
            result.push(base);
            result.push(combining);
        } else {
            result.push(c);
        }
    }

    // 結合文字列を CCC 順にソート (Canonical Ordering)
    canonical_order(&mut result);
    result
}

/// テキストを NFC (Canonical Composition) に変換。
///
/// まず NFD に分解し、その後再合成を試みる。
#[must_use]
pub fn to_nfc(input: &str) -> String {
    let nfd = to_nfd(input);
    let chars: Vec<char> = nfd.chars().collect();
    let len = chars.len();

    if len == 0 {
        return String::new();
    }

    let mut result = String::with_capacity(input.len());
    let mut i = 0;

    while i < len {
        let current = chars[i];

        // 結合文字が続くかチェック
        if i + 1 < len && combining_class(chars[i + 1]) > 0 {
            // 合成を試みる
            if let Some(composed) = compose_pair(current, chars[i + 1]) {
                result.push(composed);
                i += 2;
                continue;
            }
        }

        result.push(current);
        i += 1;
    }

    result
}

/// Canonical Ordering: 結合文字列を CCC 順に安定ソート。
fn canonical_order(s: &mut String) {
    let mut chars: Vec<char> = s.chars().collect();
    let len = chars.len();

    // バブルソート (結合文字列は通常短いので十分)
    // 基底文字の境界を越えないようにソート
    let mut i = 1;
    while i < len {
        let cc_i = combining_class(chars[i]);
        if cc_i == 0 {
            i += 1;
            continue;
        }
        // 結合文字列の開始位置を探す
        let mut j = i;
        while j > 0 && combining_class(chars[j - 1]) > cc_i {
            chars.swap(j - 1, j);
            j -= 1;
        }
        i += 1;
    }

    s.clear();
    for c in chars {
        s.push(c);
    }
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
