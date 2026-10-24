# Changelog

All notable changes to ALICE-Text will be documented in this file.

## [Unreleased]

### Added
- `unicode_norm`: Unicode 16.0.0 normalization for the whole code space — `to_nfd` / `to_nfc` (existing) and the new `to_nfkd` / `to_nfkc` (full-width / half-width forms and other compatibility characters), with Hangul composed and decomposed arithmetically; `case_fold` (full default case folding, status C + F) and `case_fold_simple` (C + S); `UNICODE_VERSION` and `NORMALIZATION_SEMANTICS_ID` (SHA-256 of the version and the data files' hashes, so another Unicode version is another id)
- `data/ucd/16.0.0/`: UnicodeData, CompositionExclusions, CaseFolding and NormalizationTest at 16.0.0 with `SHA256SUMS`, and the Unicode licence; `scripts/gen_unicode_tables.py` writes `src/unicode_tables.rs` from them after checking the hashes (`--check` compares); nothing is read from the platform's or the standard library's Unicode tables
- `tests/unicode_conformance.rs`: every line of all six parts of NormalizationTest, every code point outside part 1 unchanged by the four forms, and full and simple folding of every scalar value against CaseFolding
- `word_break::boundaries`: Unicode 16.0.0 word boundaries (UAX #29, rules WB1–WB999) as byte offsets, from tables generated from `data/ucd/16.0.0/word-break/` (WordBreakProperty, emoji-data's Extended_Pictographic; its own SHA256SUMS and `WORD_BREAK_SEMANTICS_ID`); `tests/word_break.rs` checks every line of the Consortium's WordBreakTest (1826)
- `word_list::WordList`: which words of a list occur in a text, compared by Unicode's compatibility caseless match (D146, built on the forms and folding above) so full-width, half-width, ligature and case variants of a word are found; by default (`WordList::new`) a word must start and end at word boundaries of the text ("ass" is not found in "class"; a katakana run is one word), `WordList::substring` finds it anywhere; an Aho-Corasick automaton over the keys with transitions in code point order (the same list gives the same automaton everywhere); an empty word is refused by its index; `tests/word_list.rs` compares both modes with a brute force over 3000 pseudo-random lists and texts
- `examples/unicode_dump.rs` + `scripts/unicode_crosscheck.py` (+ tests): every scalar value's four forms and folding compared with Python's `unicodedata` at the same Unicode version (another version fails); CI job `unicode` on Linux, macOS and Windows with the generator check
- `law` module: `TextLaw` keeps the skeletons of a corpus (lines with values replaced by placeholders) as a law and measures its exception rate (lines with an unseen skeleton / lines) on actual text; `ingest` returns a `TextVerdict` in the rule order of `alice_zip::law::Verdict` (NoEvidence / Supports / ParameterUpdate / ResidualGrew / Breaks; there is no OutOfRange for text) and does not change the law; `ExceptionCount` holds the counts; 18 tests with closed-form expectations
- `examples/text_law.rs`: learns a 300-line log and judges four inputs
- dependency `alice-zip` (`git = "https://github.com/ext-sakamoro/ALICE-Zip"`, `rev = "fec22a06eab879153a140b057f53f081876a2ff7"`, `version = "0.11"`, `default-features = false`) for `Provenance` / `IngestPolicy` / `LawError`; the git source and the full commit name the dependency (the same source string in every ALICE repository resolves to a single `alice-zip`), so CI and `scripts/preflight.sh` need no checkout of `ALICE-Zip` next to this one; `deny.toml` allows that git source
- fuzz targets `fuzz_decompress`, `fuzz_decompress_v3`, `fuzz_roundtrip` and `.github/workflows/fuzz.yml` (60 s per target on changes to `src/` or `fuzz/`)
- `.github/workflows/security-audit.yml` (cargo audit, cargo deny, cargo machete, stub guard), `deny.toml`, `scripts/stub_guard.sh`
- `scripts/docs_lint.py` + `scripts/test_docs_lint.py`: public-document vocabulary, CHANGELOG structure, README example = crate doctest
- `README_JP.md` (renamed from `README_ja.md`)

### Changed
- Licence: from `MIT OR Apache-2.0` to `LicenseRef-Proprietary` from this version (the source stays published for reading; using it needs a written agreement); the crate depends on alice-ml, which is `LicenseRef-Proprietary`
- `unicode_norm::to_nfd` / `to_nfc` / `strip_accents` / `is_combining` cover every Unicode 16.0.0 character instead of Latin-1 Supplement and Latin Extended-A (`is_combining` stays a `const fn`: a const binary search over the generated table)
- license files match the `license` field of `Cargo.toml` (`MIT OR Apache-2.0`): `LICENSE` (Business Source License 1.1 text) is replaced by `LICENSE-MIT`; `LICENSE-APACHE` is unchanged; `pyproject.toml` declares `MIT OR Apache-2.0` (and the author address of `Cargo.toml`); README / README_JP / TRADEMARK_NOTICE point to both files, and the module headers of `dialogue` / `font_bridge` no longer say BSL 1.1
- crate documentation: the principle section describes the implementation (skeleton + typed value columns + Zstd) instead of a next-token prediction model
- CI: tests on Linux x86_64 / arm64, macOS and Windows with default features and `ffi`, the example and benches compile; clippy pedantic + nursery for default and `ffi,python`; MSRV job; rustdoc `-D warnings`; docs lint on three OS; `scripts/preflight.sh` runs the same commands (`--quick` runs `cargo test --lib`)
- clippy `pedantic` and `nursery` enabled in `[lints.clippy]`; `ffi` uses `let … else` for the UTF-8 checks and `alice_text_version` is a `const fn`
- `rust-version = "1.87"` (measured: 1.86 is rejected by `alice-zip`)
- `pyo3` 0.22 → 0.29 (`allow_threads` → `detach`); the Python module API is unchanged
- `PatternType` is now `#[repr(u8)]` with explicit discriminants; `PatternType::to_tag` / `from_tag` / `ALL` are the single source for the wire tag
- `tuned_pattern_learner::PatternType` is a re-export of `pattern_learner::PatternType` (the duplicated enum and its `as_u8` / `from_u8` are removed; `TunedPatternType` alias unchanged)
- `LICENSE-MIT`: copyright year range is `2025-2026`

### Removed
- **Breaking (2.0.0):** the `font` feature and `font_bridge` module (character sets and text shaping with ALICE-Font). The bridge now lives on the ALICE-Font side, which depends on this crate, so `alice-text` no longer has a path dependency on it. The other feature flags and the compression API are unchanged.
- unused dependencies `log` and `bytemuck`

### Fixed
- Zstd streams were decoded without a size limit, so a frame of a few kilobytes could expand to gigabytes (format v2, v3 and dialogue / localization tables). The v3 writer now records each column's decompressed size in `uncompressed_size` (it wrote 0 before) and the reader decodes no further than that size and rejects a column that expands to any other size; streams without a recorded size (v2, dialogue tables, v3 columns with `uncompressed_size == 0` from earlier versions) are limited to 256 MiB. The format version is unchanged and earlier files remain readable. **Behavior change:** `compress` / `compress_table` / `compress_localization` return `EncodingError` when the serialized payload of one call exceeds 256 MiB, instead of writing a stream that the reader would refuse
- compression was not lossless for values whose spelling is not the canonical form of the parsed value (found by `fuzz_roundtrip`: `":03"` came back as `":3"`); the typed columns stored only the value, so `007`, `00`, `1.050`, `1.0`, `000.25`, integers beyond `f64` precision, `WARNING`, upper-case UUIDs, non-canonical IPv6 forms, times such as `03:04:05.1` / `03:04:05.000`, timestamps with fractional seconds, and timestamps whose form or offset differs from the first one (a later offset also shifted earlier timestamps) were rewritten, and an IPv4 address with leading zeros became `0.0.0.0`. A value now goes into a typed column only when it formats back to exactly the matched text; otherwise it is stored as a string (the existing `others` / raw timestamp / raw date / raw time columns, so files stay readable by earlier versions). **Behavior change:** v3 queries on `numbers` / `log_levels` / `ipv4` no longer see the non-canonical spellings (for example `log_levels=WARN` does not match `WARNING`), because those are now stored as strings
- `decompress_v3` / `FormatV3Writer::read_columns` allocated the compressed size declared in a column entry before reading it, so an input of 89 bytes could request about 1.75 GB (found by `fuzz_decompress_v3`); a column whose offset + size lies past the end of the input now returns `DecompressionError` before any allocation. Files written by `FormatV3Writer` are unaffected (their last column ends exactly at the end of the file)
- dialogue / localization table decoders: the length check is written without `16 + len`, which could overflow on 32-bit targets
- `cargo bench` did not compile: the bench profile inherited `panic = "abort"` from the release profile while the bench target is built with unwinding; `[profile.bench] panic = "unwind"` builds the dependencies of benches with unwinding (cargo prints that the setting is ignored for the bench target itself)
- `.cargo/config.toml` から `target-cpu=native` を外した (`[build]` と `[target.*]` 3 つの計 4 箇所) CI runner の CPU 世代に依存して rustc 自身が SIGILL で落ちるため (2026-09-28 に ALICE-LLM の rustdoc job で実測、run 36431060432) commit と無関係に red / green が揺れる native が要るのは bench だけなので、local の opt-in を `RUSTFLAGS="-C target-cpu=native" cargo bench` と `cargo bench --config 'build.rustflags=["-C","target-cpu=native"]'` の 2 経路に集約 (README / README_JP に記載) `.cargo/config.local.toml` は cargo が自動では読まないので使えない (2026-09-29 実測) 同じ方針を ALICE-LLM / ALICE-View と揃えた
- Exception decoder rejected nothing: any pattern tag outside `0..=12` was silently decoded as `Custom`; it now returns `ALICETextError::InvalidPatternTag(u8)`
- Encoder / decoder / tuned learner each carried a hand-copied tag table (4 copies); replaced by the single mapping above, guarded by exhaustive round-trip tests (13 variants, tags 13..=255 rejected)

### Security
- `pyo3` 0.29 includes the fixes for RUSTSEC-2025-0020 and RUSTSEC-2026-0177 (`python` feature); RUSTSEC-2025-0141 (bincode 1.x unmaintained) is listed in `deny.toml` because bincode 1.x encodes the stored v2 / v3 formats

## [1.0.1] - 2026-03-04

### Added
- `ffi` — 20 `extern "C"` FFI functions (compress, decompress, tuned, stats, entropy, dialogue)
- Unity C# bindings (`bindings/unity/AliceText.cs`) — 20 DllImport + Compressor/DialogueTable classes
- UE5 C++ header (`bindings/ue5/AliceText.h`) — 20 extern C + RAII FCompressor/FDialogueTable wrappers

### Fixed
- `cargo fmt` trailing whitespace in multiple source files

## [1.0.0] - 2026-02-23

### Added
- `exception_encoder` — Predictive coding exception-based encoder
- `exception_decoder` — Exception stream decoder
- `arithmetic_coder` — Arithmetic encoder/decoder for entropy coding
- `entropy_estimator` — Shannon entropy and compression estimator
- `pattern_learner` — Regex-based structured pattern extraction (IP, UUID, timestamp, etc.)
- `columnar_encoder` — Columnar encoding with delta-encoded timestamps and binary IP/UUID
- `tuned_compressor` — Zstd + columnar pipeline (v2 format)
- `tuned_pattern_learner` — Optimized pattern learner with SmallVec
- `format_v3` — Column-oriented compressed format with partial decompression
- `query_engine` — SQL-like query engine over compressed v3 files (mmap + Rayon)
- `dialogue` — Game dialogue compression, delta tables, ruby annotations, localization
- Feature flags: `python`, `ml`, `voice`, `search`, `font`
- PyO3 Python bindings (feature-gated: `python`)
- mimalloc global allocator
- 168 unit tests + 1 doc-test
