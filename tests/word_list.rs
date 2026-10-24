//! The word list: which words of a list occur in a text, compared by Unicode's compatibility caseless match
//! (D146: NFKD(fold(NFKD(fold(NFD(x)))))), so full-width and half-width forms, ligatures and case do not hide a
//! word. By default a word must start and end at UAX #29 word boundaries of the text; `WordList::substring` drops
//! that condition.
//!
//! Oracles: the definition itself, applied by brute force (each word's caseless key searched in the text's caseless
//! key, at the boundaries `word_break::boundaries` gives, which its own test checks against WordBreakTest.txt) over
//! a fixed pseudo-random corpus, and hand cases from the Unicode data.

use alice_text::unicode_norm::{case_fold, to_nfd, to_nfkd};
use alice_text::word_break::boundaries;
use alice_text::word_list::{WordList, WordListError};

/// D146, written out here from the forms (the oracle)
fn key(s: &str) -> String {
    to_nfkd(&case_fold(&to_nfkd(&case_fold(&to_nfd(s)))))
}

fn brute(words: &[&str], text: &str) -> Vec<usize> {
    let t = key(text);
    (0..words.len())
        .filter(|&i| t.contains(&key(words[i])))
        .collect()
}

/// The text's key and the char positions in it of the text's word boundaries (each segment between two boundaries
/// keyed on its own: no combining mark starts a segment, so the keys concatenate)
fn keyed_boundaries(text: &str) -> (Vec<char>, Vec<usize>) {
    let b = boundaries(text);
    let mut k = Vec::new();
    let mut at = vec![0];
    for w in b.windows(2) {
        k.extend(key(&text[w[0]..w[1]]).chars());
        at.push(k.len());
    }
    (k, at)
}

/// The words whose key occurs in the text's key starting and ending at a boundary
fn brute_words(words: &[&str], text: &str) -> Vec<usize> {
    let (t, at) = keyed_boundaries(text);
    (0..words.len())
        .filter(|&i| {
            let w: Vec<char> = key(words[i]).chars().collect();
            (0..t.len().saturating_sub(w.len() - 1)).any(|s| {
                t[s..s + w.len()] == w[..] && at.contains(&s) && at.contains(&(s + w.len()))
            })
        })
        .collect()
}

#[test]
fn a_word_is_found_whatever_its_width_case_or_ligature() {
    let list = WordList::new(&["bad", "バカ", "strasse", "ﬁne"]).unwrap();
    for (text, want) in [
        ("this is BAD", vec![0]),
        ("ｂａｄ (full width)", vec![0]),
        ("ﾊﾞｶ (half width)", vec![1]),
        ("ばか (hiragana is another word)", vec![]),
        ("Straße", vec![2]),
        ("STRASSE", vec![2]),
        ("a fine day", vec![3]),
        ("nothing here", vec![]),
        ("bad バカ fine", vec![0, 1, 3]),
    ] {
        assert_eq!(list.matches(text), want, "{text}");
        assert_eq!(
            list.matches(text),
            brute_words(&["bad", "バカ", "strasse", "ﬁne"], text),
            "{text}"
        );
    }
}

#[test]
fn every_match_is_the_definition_applied_by_brute_force() {
    // words and texts drawn from a small alphabet that mixes forms (so overlaps and near-misses are common)
    let alphabet: Vec<char> = "abAB ａｂＡＢﬀßsS゛カｶﾞガ\u{0301}é".chars().collect();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = |n: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) as usize) % n
    };
    let mut cases = 0usize;
    let mut found = 0usize;
    let mut found_words = 0usize;
    for _ in 0..300 {
        let words: Vec<String> = (0..=next(6))
            .map(|_| {
                (0..=next(4))
                    .map(|_| alphabet[next(alphabet.len())])
                    .collect::<String>()
            })
            .filter(|w| !key(w).is_empty())
            .collect();
        if words.is_empty() {
            continue;
        }
        let refs: Vec<&str> = words.iter().map(String::as_str).collect();
        let list = WordList::new(&refs).unwrap();
        let sub = WordList::substring(&refs).unwrap();
        for _ in 0..10 {
            let text: String = (0..next(24))
                .map(|_| alphabet[next(alphabet.len())])
                .collect();
            let want = brute(&refs, &text);
            found += want.len();
            assert_eq!(
                sub.matches(&text),
                want,
                "substring: words {refs:?} text {text:?}"
            );
            let want_words = brute_words(&refs, &text);
            found_words += want_words.len();
            assert_eq!(
                list.matches(&text),
                want_words,
                "words {refs:?} text {text:?}"
            );
            cases += 1;
        }
    }
    // compared something, and the corpus is not all misses
    assert!(cases > 2_000, "{cases}");
    assert!(found > 500, "{found}");
    // the boundary condition removes some matches but not all
    assert!(
        found_words > 100 && found_words < found,
        "{found_words} of {found}"
    );
}

#[test]
fn an_empty_word_is_refused() {
    assert_eq!(
        WordList::new(&["ok", ""]).err(),
        Some(WordListError::EmptyWord(1))
    );
    // a word whose caseless key is empty (a lone combining mark folds to itself, so only "" is empty)
    assert!(WordList::new(&[]).unwrap().matches("anything").is_empty());
}

#[test]
fn a_list_is_a_value_the_same_words_give_the_same_answers() {
    let a = WordList::new(&["bad", "ＢＡＤ"]).unwrap();
    // two words with the same key are both reported (each is a word of the list)
    assert_eq!(a.matches("bad"), vec![0, 1]);
    assert_eq!(a.len(), 2);
}

#[test]
fn the_substring_mode_ignores_word_and_grapheme_boundaries() {
    // the substring mode: no word boundary and no grapheme boundary
    let list = WordList::substring(&["ass", "e"]).unwrap();
    assert_eq!(list.matches("class"), vec![0]);
    assert_eq!(list.matches("caf\u{e9}"), vec![1]); // é decomposes to e + U+0301
}

#[test]
fn by_default_a_word_starts_and_ends_at_word_boundaries() {
    let list = WordList::new(&["ass", "e", "ice cream"]).unwrap();
    assert_eq!(list.matches("class"), Vec::<usize>::new());
    assert_eq!(list.matches("an ass."), vec![0]);
    assert_eq!(list.matches("café"), Vec::<usize>::new());
    // a word of several segments: the spaces inside it are part of the match
    assert_eq!(list.matches("ICE CREAM, please"), vec![2]);
    assert_eq!(list.matches("nice creamy"), Vec::<usize>::new());
    // katakana runs are one word (WB13), hiragana after them is another
    let jp = WordList::new(&["バカ"]).unwrap();
    assert_eq!(jp.matches("バカだ"), vec![0]);
    assert_eq!(jp.matches("バカヤロー"), Vec::<usize>::new());
}
