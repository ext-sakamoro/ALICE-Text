//! The word list: which words of a list occur in a text, compared by Unicode's compatibility caseless match
//! (D146: NFKD(fold(NFKD(fold(NFD(x)))))), so full-width and half-width forms, ligatures and case do not hide a
//! word.
//!
//! Oracles: the definition itself, applied by brute force (each word's caseless key searched in the text's caseless
//! key with `str::contains`) over a fixed pseudo-random corpus, and hand cases from the Unicode data (each pair is
//! a compatibility caseless match by the standard's examples).

use alice_text::unicode_norm::{case_fold, to_nfd, to_nfkd};
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
            brute(&["bad", "バカ", "strasse", "ﬁne"], text),
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
        for _ in 0..10 {
            let text: String = (0..next(24))
                .map(|_| alphabet[next(alphabet.len())])
                .collect();
            let want = brute(&refs, &text);
            found += want.len();
            assert_eq!(list.matches(&text), want, "words {refs:?} text {text:?}");
            cases += 1;
        }
    }
    // compared something, and the corpus is not all misses
    assert!(cases > 2_000, "{cases}");
    assert!(found > 500, "{found}");
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
fn the_match_is_a_substring_of_the_key_not_a_whole_word() {
    // the documented meaning: no word boundary and no grapheme boundary
    let list = WordList::new(&["ass", "e"]).unwrap();
    assert_eq!(list.matches("class"), vec![0]);
    assert_eq!(list.matches("caf\u{e9}"), vec![1]); // é decomposes to e + U+0301
}
