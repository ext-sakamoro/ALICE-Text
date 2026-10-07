//! The line-skeleton predictor kept as a law, with its measured exception rate
//!
//! ALICE-Text replaces every recognised value in a line (timestamps, IP
//! addresses, UUIDs, numbers, paths … see [`TunedPatternLearner`]) with a
//! numbered placeholder. What is left is the line's *skeleton*:
//!
//! ```text
//! 2024-01-15 10:30:45 INFO request 17 accepted   →   {0} {1} request {2} accepted
//! ```
//!
//! [`TextLaw`] treats "the next line has a skeleton that was seen before" as
//! the prediction. A line whose skeleton is new is an *exception*: it has to be
//! stored in full, while a line with a known skeleton only needs its values.
//! The law is the set of learned skeletons; its residual is the **exception
//! rate**, `exception lines / lines`, always counted on actual text and never
//! estimated.
//!
//! | quantity | definition |
//! |---|---|
//! | learning rate ([`TextLaw::learned_rate`]) | lines of the corpus whose skeleton had not appeared on an earlier corpus line, divided by the corpus lines (the cost of learning the corpus once, in order) |
//! | exception rate of new text ([`TextLaw::exception_rate`]) | lines of the text whose skeleton is not in the law, divided by the lines of the text (the law is fixed while measuring) |
//!
//! Lines are those of [`str::lines`]: split at `\n`, a trailing `\r` removed,
//! no final empty line after a trailing newline. An empty line is a line (its
//! skeleton is the empty string).
//!
//! A text made of one period of `p` lines with `p` distinct skeletons,
//! repeated `k` times, has exactly `p` exception lines while it is learned (all
//! in the first period) and a learning rate of `p / (p·k) = 1 / k`; further
//! periods have an exception rate of 0 whatever their values are.
//!
//! # Judging new text
//!
//! [`TextLaw::ingest`] follows the rule order of [`alice_zip::law::Verdict`]
//! and does not change the law. With `band = max(policy.abs_tolerance,
//! learned_rate())` and `rate` the exception rate of the new text:
//!
//! 1. no lines → [`TextVerdict::NoEvidence`]
//! 2. `rate ≤ band` → [`TextVerdict::Supports`]
//! 3. learning the new lines after the corpus (the learning rate of corpus +
//!    new text) gives a rate `≤ band` → [`TextVerdict::ParameterUpdate`]
//!    (carries the law learned on both)
//! 4. `rate ≤ band · policy.break_factor` → [`TextVerdict::ResidualGrew`]
//! 5. otherwise → [`TextVerdict::Breaks`]
//!
//! `alice_zip`'s `OutOfRange` rule has no counterpart: a polynomial law has a
//! measured `x` interval outside which it must not be evaluated, but every
//! line of text can be reduced to a skeleton and either is or is not in the
//! law. A line the law has never seen is exactly what the exception rate
//! counts, so it is a residual, not a domain violation.
//!
//! ```
//! use alice_text::law::{TextLaw, TextVerdict};
//! use alice_zip::law::{IngestPolicy, Provenance};
//!
//! let corpus: String = (0..10)
//!     .map(|i| format!("2024-01-15 10:30:{i:02} request {i} accepted\n"))
//!     .collect();
//! let law = TextLaw::learn(&corpus, Provenance::new("service log", "line skeletons"))?;
//! assert_eq!(law.learned_rate(), 0.1); // 1 skeleton over 10 lines
//!
//! let policy = IngestPolicy { abs_tolerance: 0.01, break_factor: 4.0 };
//! let more = "2024-01-16 08:00:00 request 99 accepted\n";
//! assert!(matches!(law.ingest(more, &policy), TextVerdict::Supports { rate } if rate == 0.0));
//! # Ok::<(), alice_zip::law::LawError>(())
//! ```

use std::collections::BTreeSet;
use std::sync::OnceLock;

use alice_zip::law::{IngestPolicy, LawError, Provenance};

use crate::tuned_pattern_learner::TunedPatternLearner;

/// The pattern learner shared by every law (its regex is compiled once)
fn learner() -> &'static TunedPatternLearner {
    static LEARNER: OnceLock<TunedPatternLearner> = OnceLock::new();
    LEARNER.get_or_init(TunedPatternLearner::new)
}

/// Skeleton of one line: every recognised value replaced by a placeholder
fn skeleton(line: &str) -> String {
    learner().extract_skeleton(line).0
}

/// Lines and exception lines counted on a text
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExceptionCount {
    /// Number of lines
    pub lines: usize,
    /// Number of lines whose skeleton was not predicted
    pub exceptions: usize,
}

impl ExceptionCount {
    /// `exceptions / lines`
    ///
    /// # Errors
    ///
    /// [`LawError::TooFewPoints`] when there are no lines (the rate is
    /// undefined).
    pub fn rate(&self) -> Result<f64, LawError> {
        if self.lines == 0 {
            return Err(LawError::TooFewPoints);
        }
        Ok(self.exceptions as f64 / self.lines as f64)
    }
}

/// What new text does to a [`TextLaw`] (rules in the module docs)
#[derive(Debug, Clone, PartialEq)]
pub enum TextVerdict {
    /// The text has no lines
    NoEvidence,
    /// The text agrees with the law
    Supports {
        /// Exception rate of the new text
        rate: f64,
    },
    /// The law learned on corpus + new text stays within the band
    ParameterUpdate {
        /// Exception rate of the new text under the current law
        previous_rate: f64,
        /// The law learned on corpus + new text
        updated: Box<TextLaw>,
    },
    /// The exception rate exceeds the band but not the break threshold
    ResidualGrew {
        /// Exception rate of the new text
        rate: f64,
    },
    /// The text is not described by the learned skeletons
    Breaks {
        /// Exception rate of the new text
        rate: f64,
    },
}

/// Learned line skeletons with the exception rate measured while learning
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextLaw {
    skeletons: BTreeSet<String>,
    lines: usize,
    exceptions: usize,
    provenance: Provenance,
}

impl TextLaw {
    /// Learns the skeletons of `corpus`, counting a line as an exception when
    /// its skeleton has not appeared on an earlier line
    ///
    /// # Errors
    ///
    /// [`LawError::TooFewPoints`] when `corpus` has no lines.
    pub fn learn(corpus: &str, provenance: Provenance) -> Result<Self, LawError> {
        let mut law = Self {
            skeletons: BTreeSet::new(),
            lines: 0,
            exceptions: 0,
            provenance,
        };
        law.learn_more(corpus);
        if law.lines == 0 {
            return Err(LawError::TooFewPoints);
        }
        Ok(law)
    }

    /// Continues learning on `text` (the same counting as [`Self::learn`])
    fn learn_more(&mut self, text: &str) {
        for line in text.lines() {
            self.lines += 1;
            if self.skeletons.insert(skeleton(line)) {
                self.exceptions += 1;
            }
        }
    }

    /// Counts the lines of `text` and those whose skeleton is not in the law
    ///
    /// The law is not changed: a new skeleton seen twice counts twice.
    #[must_use]
    pub fn count_exceptions(&self, text: &str) -> ExceptionCount {
        let mut count = ExceptionCount {
            lines: 0,
            exceptions: 0,
        };
        for line in text.lines() {
            count.lines += 1;
            if !self.skeletons.contains(&skeleton(line)) {
                count.exceptions += 1;
            }
        }
        count
    }

    /// Exception rate of `text` under the law ([`Self::count_exceptions`])
    ///
    /// # Errors
    ///
    /// [`LawError::TooFewPoints`] when `text` has no lines.
    pub fn exception_rate(&self, text: &str) -> Result<f64, LawError> {
        self.count_exceptions(text).rate()
    }

    /// Exception rate measured while learning the corpus
    /// (`corpus_exceptions / corpus_lines`)
    #[must_use]
    pub fn learned_rate(&self) -> f64 {
        self.exceptions as f64 / self.lines as f64
    }

    /// Lines of the corpus the law was learned from
    #[must_use]
    pub const fn corpus_lines(&self) -> usize {
        self.lines
    }

    /// Exception lines counted while learning the corpus
    #[must_use]
    pub const fn corpus_exceptions(&self) -> usize {
        self.exceptions
    }

    /// Number of distinct skeletons the law holds
    #[must_use]
    pub fn skeleton_count(&self) -> usize {
        self.skeletons.len()
    }

    /// Whether the law predicts `line` (its skeleton has been learned)
    #[must_use]
    pub fn predicts(&self, line: &str) -> bool {
        self.skeletons.contains(&skeleton(line))
    }

    /// Where the corpus came from and how the law was obtained
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Judges new text against the law without changing it
    ///
    /// See the module documentation for the order of the rules.
    #[must_use]
    pub fn ingest(&self, text: &str, policy: &IngestPolicy) -> TextVerdict {
        let Ok(rate) = self.exception_rate(text) else {
            return TextVerdict::NoEvidence;
        };
        let band = policy.abs_tolerance.max(self.learned_rate());
        if rate <= band {
            return TextVerdict::Supports { rate };
        }
        let mut updated = self.clone();
        updated.learn_more(text);
        if updated.learned_rate() <= band {
            return TextVerdict::ParameterUpdate {
                previous_rate: rate,
                updated: Box::new(updated),
            };
        }
        if rate <= band * policy.break_factor {
            TextVerdict::ResidualGrew { rate }
        } else {
            TextVerdict::Breaks { rate }
        }
    }
}

#[cfg(test)]
// The expected rates are exact quotients of small integers written in each
// test; `==` is the intended comparison, a tolerance would hide an off-by-one.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    const POLICY: IngestPolicy = IngestPolicy {
        abs_tolerance: 0.01,
        break_factor: 4.0,
    };

    fn prov() -> Provenance {
        Provenance::new("test corpus", "line skeletons")
    }

    /// One period of three lines with three distinct skeletons; `n` changes
    /// only the values (timestamp, id, depth), never the skeleton
    fn period(n: usize) -> String {
        let s = n % 60;
        format!(
            "2024-01-15 10:30:{s:02} request id={n} accepted\n\
             2024-01-15 10:30:{s:02} queue depth {} above limit\n\
             2024-01-15 10:30:{s:02} request id={n} finished\n",
            n * 7 + 3
        )
    }

    fn periods(range: std::ops::Range<usize>) -> String {
        range.map(period).collect()
    }

    /// The first `count` lines (each with its newline) of `period(n)`
    fn first_lines(n: usize, count: usize) -> String {
        period(n).split_inclusive('\n').take(count).collect()
    }

    /// A second period of two lines, skeletons disjoint from `period`
    fn other_period(n: usize) -> String {
        format!(
            "cache shard {n} rebalanced\n\
             cache shard {n} flushed to /var/cache/shard{n}\n"
        )
    }

    /// Lowercase word from an index (no digits, so nothing is a value)
    fn word(mut i: usize) -> String {
        let mut w = String::new();
        loop {
            w.push((b'a' + (i % 26) as u8) as char);
            i /= 26;
            if i == 0 {
                break;
            }
        }
        w
    }

    /// `m` lines with pairwise distinct skeletons sharing none with `period`
    fn unrelated(m: usize) -> String {
        (0..m).fold(String::new(), |mut s, i| {
            let _ = writeln!(s, "zz {} qq {}", word(i), word(i + 1000));
            s
        })
    }

    // --- learning: closed forms -------------------------------------------

    #[test]
    fn periodic_corpus_has_one_period_of_exceptions() {
        // p = 3 skeletons, k = 10 periods → exceptions 3, lines 30, rate 1/10
        let law = TextLaw::learn(&periods(0..10), prov()).unwrap();
        assert_eq!(law.corpus_lines(), 30);
        assert_eq!(law.corpus_exceptions(), 3);
        assert_eq!(law.skeleton_count(), 3);
        assert_eq!(law.learned_rate(), 3.0 / 30.0);
    }

    #[test]
    fn warm_up_is_exactly_the_first_period() {
        // learning only the first period already yields every exception:
        // the remaining k − 1 periods add none
        let first = TextLaw::learn(&period(0), prov()).unwrap();
        assert_eq!(first.corpus_exceptions(), 3);
        for k in [2usize, 5, 40] {
            let law = TextLaw::learn(&periods(0..k), prov()).unwrap();
            assert_eq!(law.corpus_exceptions(), 3, "k = {k}");
            assert_eq!(law.learned_rate(), 3.0 / (3 * k) as f64, "k = {k}");
        }
    }

    #[test]
    fn later_periods_have_exception_rate_zero() {
        let law = TextLaw::learn(&period(0), prov()).unwrap();
        let later = periods(100..150);
        assert_eq!(
            law.count_exceptions(&later),
            ExceptionCount {
                lines: 150,
                exceptions: 0
            }
        );
        assert_eq!(law.exception_rate(&later).unwrap(), 0.0);
    }

    #[test]
    fn unrelated_lines_are_all_exceptions() {
        let law = TextLaw::learn(&periods(0..10), prov()).unwrap();
        let m = 20;
        assert_eq!(
            law.count_exceptions(&unrelated(m)),
            ExceptionCount {
                lines: m,
                exceptions: m
            }
        );
        assert_eq!(law.exception_rate(&unrelated(m)).unwrap(), 1.0);
    }

    #[test]
    fn mixed_text_counts_only_unseen_lines() {
        // 8 known lines + 2 unrelated → 2 / 10
        let law = TextLaw::learn(&periods(0..10), prov()).unwrap();
        let text = format!("{}{}{}", periods(50..52), unrelated(2), first_lines(60, 1));
        let lines_known = 2 * 3 + 1;
        let count = law.count_exceptions(&text);
        assert_eq!(count.lines, lines_known + 2);
        assert_eq!(count.exceptions, 2);
    }

    #[test]
    fn values_do_not_change_the_skeleton_but_words_do() {
        let law = TextLaw::learn("2024-01-15 10:30:45 request id=1 accepted", prov()).unwrap();
        assert!(law.predicts("1999-12-31 23:59:59 request id=987654 accepted"));
        assert!(!law.predicts("2024-01-15 10:30:45 request id=1 rejected"));
    }

    #[test]
    fn crlf_and_lf_lines_are_the_same_lines() {
        let law = TextLaw::learn(&periods(0..2), prov()).unwrap();
        let crlf = periods(5..7).replace('\n', "\r\n");
        assert_eq!(law.exception_rate(&crlf).unwrap(), 0.0);
        let law_crlf = TextLaw::learn(&periods(0..2).replace('\n', "\r\n"), prov()).unwrap();
        assert_eq!(law_crlf, law);
    }

    // --- degenerate input ---------------------------------------------------

    #[test]
    fn empty_corpus_is_too_few_points() {
        assert_eq!(TextLaw::learn("", prov()), Err(LawError::TooFewPoints));
    }

    #[test]
    fn empty_text_has_no_rate_and_no_evidence() {
        let law = TextLaw::learn(&period(0), prov()).unwrap();
        assert_eq!(law.exception_rate(""), Err(LawError::TooFewPoints));
        assert_eq!(law.ingest("", &POLICY), TextVerdict::NoEvidence);
        assert_eq!(
            ExceptionCount {
                lines: 0,
                exceptions: 0
            }
            .rate(),
            Err(LawError::TooFewPoints)
        );
    }

    #[test]
    fn very_short_texts() {
        // one line, one character: one skeleton, rate 1
        let law = TextLaw::learn("a", prov()).unwrap();
        assert_eq!((law.corpus_lines(), law.corpus_exceptions()), (1, 1));
        assert_eq!(law.learned_rate(), 1.0);
        assert_eq!(law.exception_rate("a").unwrap(), 0.0);
        assert_eq!(law.exception_rate("b").unwrap(), 1.0);
        // a lone newline is one empty line
        let empty_line = TextLaw::learn("\n", prov()).unwrap();
        assert_eq!(
            (empty_line.corpus_lines(), empty_line.skeleton_count()),
            (1, 1)
        );
        assert!(empty_line.predicts(""));
    }

    // --- verdicts -------------------------------------------------------------

    #[test]
    fn same_skeletons_with_other_values_support() {
        let law = TextLaw::learn(&periods(0..10), prov()).unwrap();
        assert_eq!(
            law.ingest(&periods(200..230), &POLICY),
            TextVerdict::Supports { rate: 0.0 }
        );
    }

    #[test]
    fn rate_equal_to_band_supports() {
        // band = max(0.01, 3/30) = 1/10; new text 9 known + 1 unseen = 1/10
        let law = TextLaw::learn(&periods(0..10), prov()).unwrap();
        let text = format!("{}{}", periods(70..73), unrelated(1));
        assert_eq!(law.count_exceptions(&text).lines, 10);
        assert_eq!(
            law.ingest(&text, &POLICY),
            TextVerdict::Supports { rate: 1.0 / 10.0 }
        );
    }

    #[test]
    fn abs_tolerance_widens_the_band() {
        // band = max(0.5, 1/10) = 0.5; text with 2 of 4 unseen → 0.5 supports
        let law = TextLaw::learn(&periods(0..10), prov()).unwrap();
        let text = format!("{}{}", unrelated(2), first_lines(9, 2));
        assert_eq!(
            law.count_exceptions(&text),
            ExceptionCount {
                lines: 4,
                exceptions: 2
            }
        );
        let wide = IngestPolicy {
            abs_tolerance: 0.5,
            break_factor: 4.0,
        };
        assert_eq!(
            law.ingest(&text, &wide),
            TextVerdict::Supports { rate: 0.5 }
        );
    }

    #[test]
    fn second_periodic_pattern_is_a_parameter_update() {
        // corpus: 3 skeletons / 30 lines (band 1/10)
        // new: 2 skeletons repeated 20 times = 40 lines, all unseen → rate 1
        // corpus + new: (3 + 2) / (30 + 40) = 1/14 ≤ 1/10 → ParameterUpdate
        let law = TextLaw::learn(&periods(0..10), prov()).unwrap();
        let second: String = (0..20).map(other_period).collect();
        let TextVerdict::ParameterUpdate {
            previous_rate,
            updated,
        } = law.ingest(&second, &POLICY)
        else {
            panic!(
                "expected ParameterUpdate, got {:?}",
                law.ingest(&second, &POLICY)
            );
        };
        assert_eq!(previous_rate, 1.0);
        assert_eq!(updated.corpus_lines(), 70);
        assert_eq!(updated.corpus_exceptions(), 5);
        assert_eq!(updated.skeleton_count(), 5);
        assert_eq!(updated.learned_rate(), 5.0 / 70.0);
        assert_eq!(updated.provenance(), law.provenance());
        // the updated law predicts both patterns
        assert_eq!(updated.exception_rate(&other_period(77)).unwrap(), 0.0);
        assert_eq!(updated.exception_rate(&period(77)).unwrap(), 0.0);
    }

    #[test]
    fn short_second_pattern_grows_the_residual() {
        // new: 8 known + 2 unseen distinct lines → rate 2/10 = 0.2
        // band 1/10 < 0.2 ≤ 4/10; corpus + new = 5/40 = 0.125 > 1/10
        let law = TextLaw::learn(&periods(0..10), prov()).unwrap();
        let text = format!("{}{}{}", periods(40..42), first_lines(42, 2), unrelated(2));
        assert_eq!(
            law.count_exceptions(&text),
            ExceptionCount {
                lines: 10,
                exceptions: 2
            }
        );
        assert_eq!(
            law.ingest(&text, &POLICY),
            TextVerdict::ResidualGrew { rate: 2.0 / 10.0 }
        );
    }

    #[test]
    fn unrelated_text_breaks() {
        // rate 20/20 = 1 > band · break_factor = 0.4
        // corpus + new = (3 + 20) / (30 + 20) = 0.46 > 0.1
        let law = TextLaw::learn(&periods(0..10), prov()).unwrap();
        assert_eq!(
            law.ingest(&unrelated(20), &POLICY),
            TextVerdict::Breaks { rate: 1.0 }
        );
    }

    #[test]
    fn rate_equal_to_break_threshold_grows() {
        // band 1/10, threshold 4/10; 6 known + 4 unseen = 4/10
        // corpus + new = 7/40 > 1/10
        let law = TextLaw::learn(&periods(0..10), prov()).unwrap();
        let text = format!("{}{}", periods(10..12), unrelated(4));
        assert_eq!(law.count_exceptions(&text).lines, 10);
        assert_eq!(
            law.ingest(&text, &POLICY),
            TextVerdict::ResidualGrew { rate: 4.0 / 10.0 }
        );
    }

    #[test]
    fn ingest_does_not_change_the_law() {
        let law = TextLaw::learn(&periods(0..10), prov()).unwrap();
        let before = law.clone();
        let second: String = (0..20).map(other_period).collect();
        for text in [periods(5..9), second, unrelated(20), unrelated(1)] {
            let _ = law.ingest(&text, &POLICY);
            assert_eq!(law, before);
        }
        assert_eq!(law.skeleton_count(), 3);
        assert!(law.predicts(first_lines(0, 1).trim_end()));
        assert!(!law.predicts("cache shard 0 rebalanced"));
    }
}
