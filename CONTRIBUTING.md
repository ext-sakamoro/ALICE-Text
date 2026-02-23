# Contributing to ALICE-Text

## Build

```bash
cargo build
cargo build --features python
```

## Test

```bash
cargo test
```

## Lint

```bash
cargo clippy -- -W clippy::all
cargo fmt -- --check
cargo doc --no-deps 2>&1 | grep warning
```

## Design Constraints

- **Exception-based compression**: send only surprises (prediction failures), not predictions.
- **Columnar encoding**: timestamps (delta), IPs (binary u32/u128), UUIDs (binary u128) stored in typed columns.
- **Zstd backend**: tuned compressor (v2) uses Zstd for final entropy coding.
- **Format v3**: column-oriented compressed format with partial decompression and mmap query engine.
- **Game dialogue**: delta tables, ruby annotations, speaker dictionaries, and localization support.
- **Python bindings**: PyO3 with GIL release for heavy computation (feature-gated: `python`).
