# ALICE-Text

[日本語](README_JP.md)

Exception-based compression for structured text such as logs. Each line is
split into a *skeleton* (the line with its values replaced by placeholders)
and typed value columns: timestamps (delta encoded), dates, times, IPv4 / IPv6
addresses, UUIDs, log levels, numbers, paths, URLs and e-mail addresses. The
columnar payload is compressed with Zstd. A second format (v3) stores each
column separately so that a single column can be read and filtered without
decompressing the others.

The crate also contains a `law` module that keeps the learned line skeletons
as a law and measures how many lines of new text it fails to predict.

## What it is not for

- Maximum compression ratio on its own: on the log data measured below, gzip
  and zstd used directly produce smaller files.
- Prose and other unstructured text: without recognised values there is
  nothing to separate from the skeleton.
- A statistical next-token model: the prediction used here is "this line's
  skeleton has been seen before", nothing finer.

## Contents

- [Installation](#installation)
- [Example](#example)
- [Text law](#text-law)
- [Command line](#command-line)
- [Query engine (v3 format)](#query-engine-v3-format)
- [Features](#features)
- [Measurements](#measurements)
- [File format](#file-format)
- [Minimum supported Rust version](#minimum-supported-rust-version)
- [Building and testing](#building-and-testing)
- [Related crates](#related-crates)
- [License](#license)

## Installation

The crate is not published on crates.io; depend on the repository:

```sh
cargo add alice-text --git https://github.com/ext-sakamoro/ALICE-Text
```

Python bindings are built with maturin (`pyproject.toml` enables the `python`
feature):

```sh
pip install maturin
maturin develop --release
```

## Example

```rust
use alice_text::{ALICEText, EncodingMode};

let mut alice = ALICEText::new(EncodingMode::Pattern);

// Compress
let text = "2024-01-15 INFO User logged in from 192.168.1.100";
let compressed = alice.compress(text).unwrap();

// Decompress
let decompressed = alice.decompress(&compressed).unwrap();
assert_eq!(text, decompressed);
```

## Text law

`alice_text::law::TextLaw` learns the skeletons of a corpus and keeps them as
a law, together with its residual, the **exception rate**: lines whose
skeleton was not predicted divided by all lines, always counted on actual
text. While learning, a line is an exception when its skeleton has not
appeared on an earlier line, so a corpus of `k` repetitions of a period of `p`
lines with `p` distinct skeletons has exactly `p` exceptions and a learning
rate of `1 / k`.

`TextLaw::ingest` judges new text without changing the law, in the rule order
of `alice_zip::law::Verdict`, with `band = max(abs_tolerance, learned rate)`:

| Verdict | Condition |
|---------|-----------|
| `NoEvidence` | the text has no lines |
| `Supports` | exception rate of the text ≤ band |
| `ParameterUpdate` | learning the text after the corpus keeps the learning rate ≤ band (carries the updated law) |
| `ResidualGrew` | exception rate ≤ band × `break_factor` |
| `Breaks` | otherwise |

There is no `OutOfRange`: every line reduces to a skeleton, and an unseen one
is what the exception rate counts.

```rust
use alice_text::law::{TextLaw, TextVerdict};
use alice_zip::law::{IngestPolicy, LawError, Provenance};

fn main() -> Result<(), LawError> {
    let corpus: String = (0..10)
        .map(|i| format!("2024-01-15 10:30:{i:02} request {i} accepted\n"))
        .collect();
    let law = TextLaw::learn(&corpus, Provenance::new("service log", "line skeletons"))?;
    assert_eq!(law.learned_rate(), 0.1); // 1 skeleton over 10 lines

    let policy = IngestPolicy { abs_tolerance: 0.01, break_factor: 4.0 };
    let verdict = law.ingest("2024-01-16 08:00:00 request 99 accepted\n", &policy);
    assert!(matches!(verdict, TextVerdict::Supports { rate } if rate == 0.0));
    Ok(())
}
```

`cargo run --example text_law` learns a 300-line log and judges four inputs
(the same log with other values, a second periodic log, unrelated prose, empty
text).

## Command line

```sh
alice-text compress server.log -o server.atxt --level balanced
alice-text decompress server.atxt -o server.log
alice-text info server.atxt
alice-text estimate server.log --detailed
alice-text verify server.atxt

# v3 format (columnar, queryable)
alice-text compress-v3 server.log -o server.atxt --level balanced
alice-text query server.atxt --stats
alice-text query server.atxt --columns
alice-text query server.atxt --select timestamps,ipv4 --where "log_levels=ERROR"
alice-text query server.atxt --select log_levels,ipv4 --where "timestamps>=2024-01-15 10:30:00" --format json
```

| Level | Zstd level |
|-------|------------|
| `fast` | 3 |
| `balanced` | 10 |
| `best` | 19 |

## Query engine (v3 format)

The v3 format writes a header and a column directory, then one compressed
block per column. `QueryEngine` reads the header and decompresses only the
columns a query names:

```rust,ignore
use alice_text::{compress_v3, CompressionLevel, Op, QueryEngine};
use std::io::Cursor;

let compressed = compress_v3(text, CompressionLevel::Balanced)?;
let engine = QueryEngine::from_reader(Cursor::new(&compressed))?;

let stats = engine.stats();                                   // header only
let levels = engine.select_column("log_levels")?;             // one column
let errors = engine.filter_op("log_levels", Op::Eq, "ERROR")?; // row indices
let result = engine.query(&["timestamps", "ipv4"], "log_levels", Op::Eq, "ERROR")?;
```

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `ffi` | no | C ABI (`extern "C"`) for compression, statistics, entropy and dialogue tables; C# and C++ headers in `bindings/` |
| `python` | no | PyO3 module `alice_text` (built with maturin) |
| `ml` | no | bridge to ALICE-ML: ternary next-token scoring from pre-trained weights |
| `voice` | no | bridge to ALICE-Voice: speech encoding hints |
| `search` | no | bridge to ALICE-Search: search index over compressed text |
| `font` | no | bridge to ALICE-Font: character sets and text shaping |

The four bridge features are path dependencies on sibling checkouts
(`../ALICE-ML` and so on).

## Measurements

<!-- perf-measured: earlier release, not re-run for this one -->
Measured with an earlier release (Rust 1.84.0, macOS, an arm64 laptop); the
figures depend on the data and the machine. Ratio = compressed size /
original size, lower is better.

| Data | Lines | Size | ALICE-Text | gzip -9 | zstd -19 |
|------|-------|------|------------|---------|----------|
| random log lines | 1K | 52 KB | 43.0% | 25.6% | 23.6% |
| random log lines | 10K | 515 KB | 39.3% | 24.0% | 21.6% |
| random log lines | 100K | 5.0 MB | 39.5% | 23.8% | 20.0% |
| sequential timestamps | 100K | 6.5 MB | 34.2% | 12.9% | 10.9% |

v3 queries on a 5.7 MB log (100,000 lines), same machine:

| Operation | Time |
|-----------|------|
| statistics (header only) | 11 ms |
| filter `log_levels=ERROR` (20K matches) | 16 ms |
| filter `timestamps>=` (70K matches) | 28 ms |
| filter `ipv4=` | 7 ms |

`target-cpu=native` is not set in `.cargo/config.toml` (rustc aborts with
SIGILL on CI runners of another CPU generation). For benchmarks, opt in per
invocation:

```sh
RUSTFLAGS="-C target-cpu=native" cargo bench
cargo bench --config 'build.rustflags=["-C","target-cpu=native"]'
```

`.cargo/config.local.toml` is not read by cargo.

## File format

Files use the `.atxt` extension. The default format (v2):

```text
Magic "ALICETXT" (8 bytes)
Version 2.0 (2 bytes)
Header (24 bytes): original length, compression mode, pattern count, skeleton length
Zstd payload: bincode-serialized skeleton tokens and value columns
```

`ALICEText::decompress` also reads the older v1 format (LZMA).

## Minimum supported Rust version

Rust 1.87 (`rust-version` in `Cargo.toml`), checked in CI for the library with
default features and with `ffi` + `python`. 1.86 is rejected by the
`alice-zip` dependency (its `rust-version` is 1.87). Development and CI use the
toolchain pinned in `rust-toolchain.toml`.

## Building and testing

The bridge features are optional path dependencies, but cargo needs their
manifests to resolve the package. Clone the siblings next to this checkout,
or create empty manifests with `bash .github/stub-siblings.sh` (this
overwrites `../ALICE-ML/Cargo.toml` and the others).

```sh
cargo test                                           # unit + doc tests
cargo test --features ffi
cargo run --example text_law
cargo clippy --all-targets --features ffi,python -- -D warnings   # pedantic + nursery (Cargo.toml)
cd fuzz && cargo +nightly fuzz run fuzz_decompress   # decoders on untrusted bytes
scripts/preflight.sh            # every CI step with the same arguments
scripts/preflight.sh --quick    # static checks, clippy, docs and `cargo test --lib`
```

## Related crates

- [ALICE-Zip](https://github.com/ext-sakamoro/ALICE-Zip): procedural compression; its `law` module defines `Provenance`, `IngestPolicy` and the verdict order used by `TextLaw`
- [ALICE-DB](https://github.com/ext-sakamoro/ALICE-DB): model-based database

## License

`Cargo.toml` declares `MIT OR Apache-2.0`, with the Apache text in
[LICENSE-APACHE](LICENSE-APACHE). [LICENSE](LICENSE) contains the text of the
Business Source License 1.1 (change license: MIT); the two do not agree, so
read both files. Trademark terms are in [TRADEMARK_NOTICE](TRADEMARK_NOTICE).
