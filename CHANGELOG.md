# Changelog

All notable changes to ALICE-Text will be documented in this file.

## [Unreleased]

### Changed
- `PatternType` is now `#[repr(u8)]` with explicit discriminants; `PatternType::to_tag` / `from_tag` / `ALL` are the single source for the wire tag
- `tuned_pattern_learner::PatternType` is a re-export of `pattern_learner::PatternType` (the duplicated enum and its `as_u8` / `from_u8` are removed; `TunedPatternType` alias unchanged)

### Fixed
- `.cargo/config.toml` から `target-cpu=native` を外した (`[build]` と `[target.*]` 3 つの計 4 箇所) CI runner の CPU 世代に依存して rustc 自身が SIGILL で落ちるため (2026-09-28 に ALICE-LLM の rustdoc job で実測、run 36431060432) commit と無関係に red / green が揺れる native が要るのは bench だけなので、local の opt-in を `RUSTFLAGS="-C target-cpu=native" cargo bench` と `cargo bench --config 'build.rustflags=["-C","target-cpu=native"]'` の 2 経路に集約 (README / README_ja に記載) `.cargo/config.local.toml` は cargo が自動では読まないので使えない (2026-09-29 実測) 同じ方針を ALICE-LLM / ALICE-View と揃えた
- Exception decoder rejected nothing: any pattern tag outside `0..=12` was silently decoded as `Custom`; it now returns `ALICETextError::InvalidPatternTag(u8)`
- Encoder / decoder / tuned learner each carried a hand-copied tag table (4 copies); replaced by the single mapping above, guarded by exhaustive round-trip tests (13 variants, tags 13..=255 rejected)

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
