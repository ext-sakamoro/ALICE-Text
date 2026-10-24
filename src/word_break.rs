//! 語の境界 (Unicode 16.0.0、UAX #29 Word Boundaries の既定規則 WB1-WB999)
//!
//! [`boundaries`] は文の語の境界を byte offset で返す (両端を含む、空文なら空) 表は
//! `data/ucd/16.0.0/word-break/` の `WordBreakProperty` と `emoji-data` から `scripts/gen_unicode_tables.py` が
//! 生成し ([`WORD_BREAK_SEMANTICS_ID`] に版と data の hash)、OS の表には依らない 適合性は Consortium の
//! `WordBreakTest.txt` で試験する (`tests/word_break.rs`)
//!
//! Author: Moroya Sakamoto

pub use crate::unicode_tables::WORD_BREAK_SEMANTICS_ID;
use crate::unicode_tables::{Wb, EXTENDED_PICTOGRAPHIC, WORD_BREAK};

fn word_break(c: char) -> Wb {
    let cp = c as u32;
    WORD_BREAK
        .binary_search_by(|&(first, last, _)| {
            if last < cp {
                core::cmp::Ordering::Less
            } else if first > cp {
                core::cmp::Ordering::Greater
            } else {
                core::cmp::Ordering::Equal
            }
        })
        .map_or(Wb::Other, |i| WORD_BREAK[i].2)
}

fn extended_pictographic(c: char) -> bool {
    let cp = c as u32;
    EXTENDED_PICTOGRAPHIC
        .binary_search_by(|&(first, last)| {
            if last < cp {
                core::cmp::Ordering::Less
            } else if first > cp {
                core::cmp::Ordering::Greater
            } else {
                core::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// WB4's ignorables: Extend, Format and ZWJ attach to what precedes them
const fn ignorable(p: Wb) -> bool {
    matches!(p, Wb::Extend | Wb::Format | Wb::Zwj)
}

const fn ah_letter(p: Wb) -> bool {
    matches!(p, Wb::ALetter | Wb::HebrewLetter)
}

const fn mid_num_let_q(p: Wb) -> bool {
    matches!(p, Wb::MidNumLet | Wb::SingleQuote)
}

const fn newline(p: Wb) -> bool {
    matches!(p, Wb::Newline | Wb::Cr | Wb::Lf)
}

/// The byte offsets of the word boundaries of `text`, ascending, both ends included (none for an empty text)
#[must_use]
pub fn boundaries(text: &str) -> Vec<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let props: Vec<Wb> = chars.iter().map(|&(_, c)| word_break(c)).collect();
    let n = props.len();
    // the effective class at i once WB4 has absorbed ignorables into what precedes them: the index of the last
    // non-ignorable at or before i (an ignorable at the start of the text, or after a newline, stands for itself)
    let mut base = vec![0usize; n];
    for i in 0..n {
        // (the newline exception is WB4's wording; it cannot change a boundary: WB3a / WB3b already break
        // around every newline, and neither Newline nor Extend takes part in a later rule)
        base[i] = if i > 0 && ignorable(props[i]) && !newline(props[i - 1]) {
            base[i - 1]
        } else {
            i
        };
    }
    // the next non-ignorable at or after i
    let next_real = |mut i: usize| {
        while i < n && ignorable(props[i]) {
            i += 1;
        }
        (i < n).then(|| props[i])
    };
    // the effective class before the one at `b` (a base index), skipping ignorables the same way
    let prev_real = |b: usize| (b > 0).then(|| props[base[b - 1]]);

    let mut out = vec![0];
    for i in 1..n {
        let (a, b) = (props[i - 1], props[i]);
        let no_break = (|| {
            // WB3
            if a == Wb::Cr && b == Wb::Lf {
                return true;
            }
            // WB3a, WB3b
            if newline(a) || newline(b) {
                return false;
            }
            // WB3c
            if a == Wb::Zwj && extended_pictographic(chars[i].1) {
                return true;
            }
            // WB3d
            if a == Wb::WSegSpace && b == Wb::WSegSpace {
                return true;
            }
            // WB4
            if ignorable(b) {
                return true;
            }
            // the rules below see the text with the ignorables taken out
            let ai = base[i - 1];
            let a = props[ai];
            let c = next_real(i + 1);
            let z = prev_real(ai);
            // WB5
            if ah_letter(a) && ah_letter(b) {
                return true;
            }
            // WB6, WB7
            if ah_letter(a)
                && matches!(b, Wb::MidLetter) | mid_num_let_q(b)
                && c.is_some_and(ah_letter)
            {
                return true;
            }
            if z.is_some_and(ah_letter) && (a == Wb::MidLetter || mid_num_let_q(a)) && ah_letter(b)
            {
                return true;
            }
            // WB7a, WB7b, WB7c
            if a == Wb::HebrewLetter && b == Wb::SingleQuote {
                return true;
            }
            if a == Wb::HebrewLetter && b == Wb::DoubleQuote && c == Some(Wb::HebrewLetter) {
                return true;
            }
            if z == Some(Wb::HebrewLetter) && a == Wb::DoubleQuote && b == Wb::HebrewLetter {
                return true;
            }
            // WB8, WB9, WB10
            if (a == Wb::Numeric || ah_letter(a)) && b == Wb::Numeric
                || a == Wb::Numeric && ah_letter(b)
            {
                return true;
            }
            // WB11, WB12
            if z == Some(Wb::Numeric) && (a == Wb::MidNum || mid_num_let_q(a)) && b == Wb::Numeric {
                return true;
            }
            if a == Wb::Numeric && (b == Wb::MidNum || mid_num_let_q(b)) && c == Some(Wb::Numeric) {
                return true;
            }
            // WB13
            if a == Wb::Katakana && b == Wb::Katakana {
                return true;
            }
            // WB13a, WB13b
            if (ah_letter(a) || matches!(a, Wb::Numeric | Wb::Katakana | Wb::ExtendNumLet))
                && b == Wb::ExtendNumLet
            {
                return true;
            }
            if a == Wb::ExtendNumLet && (ah_letter(b) || matches!(b, Wb::Numeric | Wb::Katakana)) {
                return true;
            }
            // WB15, WB16: an odd run of regional indicators before b (ignorables skipped) pairs with b
            if a == Wb::RegionalIndicator && b == Wb::RegionalIndicator {
                let mut run = 0usize;
                let mut k = Some(ai);
                while let Some(j) = k {
                    if props[j] != Wb::RegionalIndicator {
                        break;
                    }
                    run += 1;
                    k = (j > 0).then(|| base[j - 1]);
                }
                return run % 2 == 1;
            }
            // WB999
            false
        })();
        if !no_break {
            out.push(chars[i].0);
        }
    }
    out.push(text.len());
    out
}
