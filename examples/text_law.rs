//! Learn the line skeletons of a log, then judge new log text against them
//!
//! ```text
//! cargo run --example text_law
//! ```

use alice_text::law::{TextLaw, TextVerdict};
use alice_zip::law::{IngestPolicy, LawError, Provenance};
use std::fmt::Write as _;

/// One period of a service log: three lines, three skeletons
fn period(n: usize) -> String {
    let s = n % 60;
    format!(
        "2024-01-15 10:30:{s:02} INFO request id={n} from 10.0.0.{} accepted\n\
         2024-01-15 10:30:{s:02} WARN queue depth {} above limit\n\
         2024-01-15 10:30:{s:02} INFO request id={n} finished in {} ms\n",
        n % 250,
        n * 7 + 3,
        n % 97
    )
}

fn describe(verdict: &TextVerdict) -> String {
    match verdict {
        TextVerdict::NoEvidence => "no evidence (no lines)".to_string(),
        TextVerdict::Supports { rate } => format!("supports (exception rate {rate:.4})"),
        TextVerdict::ParameterUpdate {
            previous_rate,
            updated,
        } => format!(
            "parameter update (rate {previous_rate:.4} under the old law; \
             {} skeletons, learning rate {:.4} after learning it)",
            updated.skeleton_count(),
            updated.learned_rate()
        ),
        TextVerdict::ResidualGrew { rate } => format!("residual grew (exception rate {rate:.4})"),
        TextVerdict::Breaks { rate } => format!("breaks (exception rate {rate:.4})"),
    }
}

fn main() -> Result<(), LawError> {
    let corpus: String = (0..100).map(period).collect();
    let law = TextLaw::learn(
        &corpus,
        Provenance::new("service log, 300 lines", "line skeletons"),
    )?;
    println!(
        "learned: {} lines, {} exceptions, {} skeletons, learning rate {:.4}",
        law.corpus_lines(),
        law.corpus_exceptions(),
        law.skeleton_count(),
        law.learned_rate()
    );

    let policy = IngestPolicy {
        abs_tolerance: 0.01,
        break_factor: 4.0,
    };
    let more: String = (1000..1050).map(period).collect();
    let second = (0..100).fold(String::new(), |mut s, n| {
        let _ = write!(s, "cache shard {n} rebalanced\ncache shard {n} flushed\n");
        s
    });
    let unrelated = "the quick brown fox\njumps over\nthe lazy dog\n";

    for (name, text) in [
        ("same log, other values", more.as_str()),
        ("a second periodic log", second.as_str()),
        ("unrelated prose", unrelated),
        ("empty", ""),
    ] {
        println!("{name:>24}: {}", describe(&law.ingest(text, &policy)));
    }
    Ok(())
}
