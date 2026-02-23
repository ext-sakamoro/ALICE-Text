# Changelog

All notable changes to ALICE-Text will be documented in this file.

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
