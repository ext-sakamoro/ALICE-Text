# Changelog

All notable changes to ALICE-Text will be documented in this file.

## [Unreleased]

### Added
- `law` module: `TextLaw` keeps the skeletons of a corpus (lines with values replaced by placeholders) as a law and measures its exception rate (lines with an unseen skeleton / lines) on actual text; `ingest` returns a `TextVerdict` in the rule order of `alice_zip::law::Verdict` (NoEvidence / Supports / ParameterUpdate / ResidualGrew / Breaks; there is no OutOfRange for text) and does not change the law; `ExceptionCount` holds the counts; 18 tests with closed-form expectations
- `examples/text_law.rs`: learns a 300-line log and judges four inputs
- dependency `alice-zip` 0.5.1 (`default-features = false`) for `Provenance` / `IngestPolicy` / `LawError`
- fuzz targets `fuzz_decompress`, `fuzz_decompress_v3`, `fuzz_roundtrip` and `.github/workflows/fuzz.yml` (60 s per target on changes to `src/` or `fuzz/`)
- `.github/workflows/security-audit.yml` (cargo audit, cargo deny, cargo machete, stub guard), `deny.toml`, `scripts/stub_guard.sh`
- `scripts/docs_lint.py` + `scripts/test_docs_lint.py`: public-document vocabulary, CHANGELOG structure, README example = crate doctest
- `README_JP.md` (renamed from `README_ja.md`)

### Changed
- crate documentation: the principle section describes the implementation (skeleton + typed value columns + Zstd) instead of a next-token prediction model
- CI: tests on Linux x86_64 / arm64, macOS and Windows with default features and `ffi`, the example and benches compile; clippy pedantic + nursery for default and `ffi,python`; MSRV job; rustdoc `-D warnings`; docs lint on three OS; `scripts/preflight.sh` runs the same commands (`--quick` runs `cargo test --lib`)
- clippy `pedantic` and `nursery` enabled in `[lints.clippy]`; `ffi` uses `let … else` for the UTF-8 checks and `alice_text_version` is a `const fn`
- `rust-version = "1.87"` (measured: 1.86 is rejected by `alice-zip`)
- `pyo3` 0.22 → 0.29 (`allow_threads` → `detach`); the Python module API is unchanged
- `PatternType` is now `#[repr(u8)]` with explicit discriminants; `PatternType::to_tag` / `from_tag` / `ALL` are the single source for the wire tag
- `tuned_pattern_learner::PatternType` is a re-export of `pattern_learner::PatternType` (the duplicated enum and its `as_u8` / `from_u8` are removed; `TunedPatternType` alias unchanged)

### Removed
- unused dependencies `log` and `bytemuck`

### Fixed
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
