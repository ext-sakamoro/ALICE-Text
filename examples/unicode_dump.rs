//! Print the four normalization forms and the full case folding of every scalar value, one line per code point:
//! `<cp> <nfc> <nfd> <nfkc> <nfkd> <fold>` with each field as hex code points joined by `.`.
//!
//! scripts/unicode_crosscheck.py reads this and compares every line with Python's unicodedata at the same Unicode
//! version (an implementation independent of this crate's tables).

use alice_text::unicode_norm::{case_fold, to_nfc, to_nfd, to_nfkc, to_nfkd};
use std::io::{BufWriter, Write};

fn hex(s: &str) -> String {
    s.chars().map(|c| format!("{:X}", c as u32)).collect::<Vec<_>>().join(".")
}

fn main() -> std::io::Result<()> {
    let out = std::io::stdout();
    let mut w = BufWriter::new(out.lock());
    for c in (0..=0x10_FFFFu32).filter_map(char::from_u32) {
        let x = c.to_string();
        writeln!(
            w,
            "{:X} {} {} {} {} {}",
            c as u32,
            hex(&to_nfc(&x)),
            hex(&to_nfd(&x)),
            hex(&to_nfkc(&x)),
            hex(&to_nfkd(&x)),
            hex(&case_fold(&x))
        )?;
    }
    w.flush()
}
