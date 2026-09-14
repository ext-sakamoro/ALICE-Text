# Changelog

All notable changes to ALICE-Text will be documented in this file.

## [Unreleased]

### Changed
- `PatternType` is now `#[repr(u8)]` with explicit discriminants; `PatternType::to_tag` / `from_tag` / `ALL` are the single source for the wire tag
- `tuned_pattern_learner::PatternType` is a re-export of `pattern_learner::PatternType` (the duplicated enum and its `as_u8` / `from_u8` are removed; `TunedPatternType` alias unchanged)

### Fixed
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
