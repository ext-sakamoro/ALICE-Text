# ALICE-Text

**Exception-Based Text Compression**

> Send only surprises, not predictions.

ALICE-Text is a text compression system that uses pattern recognition and columnar encoding for structured text like logs. It extracts known patterns (timestamps, IPs, UUIDs, etc.) and stores them in type-specific binary formats.

## Principle

```
┌─────────────────────────────────────────────────────────────┐
│  Input Text                                                 │
│       ↓                                                     │
│  Pattern Recognition (Timestamp, IP, UUID, LogLevel, ...)   │
│       ↓                                                     │
│  Columnar Encoding (Struct of Arrays)                       │
│       ↓                                                     │
│  Type-Specific Compression                                  │
│    - Timestamps: Delta encoding (milliseconds)              │
│    - IPv4: u32, IPv6: u128                                  │
│    - UUID: u128                                             │
│    - LogLevel: u8                                           │
│       ↓                                                     │
│  Zstd Compression                                           │
└─────────────────────────────────────────────────────────────┘
```

## Installation

### Rust CLI

```bash
# Build from source
cargo build --release

# Install
cargo install --path .
```

### Python (via maturin)

```bash
# Install maturin
pip install maturin

# Build and install
maturin develop --release
```

## Quick Start

### CLI Usage

```bash
# Compress a file
alice-text compress server.log -o server.atxt --level balanced

# Decompress
alice-text decompress server.atxt -o server.log

# Show file information
alice-text info server.atxt

# Estimate compression
alice-text estimate server.log --detailed

# Verify integrity
alice-text verify server.atxt
```

### Compression Levels

| Level | Zstd Level | Use Case |
|-------|------------|----------|
| `fast` | 3 | Quick compression, larger files |
| `balanced` | 10 | Default, good balance |
| `best` | 19 | Maximum compression, slower |

### Rust API

```rust
use alice_text::{ALICEText, EncodingMode};

// Create compressor
let mut alice = ALICEText::new(EncodingMode::Pattern);

// Compress
let text = "2024-01-15 10:30:45 INFO User logged in from 192.168.1.100";
let compressed = alice.compress(text).unwrap();

// Decompress
let decompressed = alice.decompress(&compressed).unwrap();
assert_eq!(text, decompressed);

// Check stats
if let Some(stats) = alice.last_stats() {
    println!("Ratio: {:.1}%", stats.compression_ratio() * 100.0);
}
```

## Features

### Pattern Recognition

Automatically detects and extracts:

| Pattern | Storage | Example |
|---------|---------|---------|
| Timestamp | Delta-encoded i64 (ms) | `2024-01-15T10:30:45+09:00` |
| IPv4 | u32 | `192.168.1.100` |
| IPv6 | u128 | `2001:db8::1` |
| UUID | u128 | `550e8400-e29b-41d4-a716-446655440000` |
| LogLevel | u8 | `INFO`, `WARN`, `ERROR` |
| Date | u32 (epoch days) | `2024-01-15` |
| Time | u32 (ms from midnight) | `10:30:45` |
| Number | f64 | `42`, `3.14159` |
| Email | String | `user@example.com` |
| URL | String | `https://example.com` |
| Path | String | `/var/log/syslog` |

### Delta Encoding

Sequential timestamps benefit from delta encoding:

```
Before: "2024-01-15 10:30:45" (19 bytes) × N
After:  base + [0, 1000, 1000, ...] (few bytes each after compression)
```

### Timezone Support

Preserves timezone information in timestamps:

- `2024-01-15T10:30:45+09:00` → Restored with `+09:00`
- `2024-01-15T10:30:45Z` → Restored with `Z`

## When to Use ALICE-Text

✅ **Use ALICE-Text when:**
- You need to query compressed log data without full decompression
- Preserving timestamp timezone information is critical (`+09:00`, `Z`)
- You want columnar access to specific fields (IP, UUID, LogLevel)
- Building an ETL-less analytics pipeline (compressed data is already structured)

❌ **Do NOT use ALICE-Text when:**
- Maximum compression ratio is your only goal (use gzip or zstd instead)
- You're compressing non-structured text (novels, articles, prose)
- You don't need to query or analyze the compressed data
- File size is the only metric that matters

**Key Trade-off:** ALICE-Text v3 achieves 27-43% compression ratio with queryable columnar storage.

## Query Engine (v3 Format)

ALICE-Text v3 introduces a columnar storage format that enables **SQL-like queries without full decompression**.

### Query Workflow

```
┌─────────────────────────────────────────────────────────────┐
│  1. Read header only (metadata, column directory)           │
│       ↓                                                     │
│  2. Decompress ONLY the filter column (e.g., log_levels)    │
│       ↓                                                     │
│  3. Scan and find matching indices (e.g., ERROR entries)    │
│       ↓                                                     │
│  4. Decompress ONLY the select columns at those indices     │
│       ↓                                                     │
│  Result: 10x-100x faster than full decompression            │
└─────────────────────────────────────────────────────────────┘
```

### CLI Usage

```bash
# Compress with v3 format (queryable)
alice-text compress-v3 server.log -o server.atxt --level balanced

# Show file statistics (header only read - instant)
alice-text query server.atxt --stats

# List available columns
alice-text query server.atxt --columns

# Select specific columns (partial decompression)
alice-text query server.atxt --select timestamps,log_levels,ipv4

# Filter: find ERROR entries only
alice-text query server.atxt --select timestamps,ipv4 --where "log_levels=ERROR"

# Filter: timestamp range query
alice-text query server.atxt --select log_levels,ipv4 --where "timestamps>=2024-01-15 10:30:00"

# Output as JSON
alice-text query server.atxt --select log_levels,ipv4 -w "log_levels=ERROR" --format json

# Limit results
alice-text query server.atxt --select log_levels,ipv4 --limit 100
```

### Rust API

```rust
use alice_text::{QueryEngine, Op, compress_v3, CompressionLevel};
use std::io::Cursor;

// Compress with v3 format
let text = "2024-01-15 10:30:45 INFO User logged in from 192.168.1.100\n...";
let compressed = compress_v3(text, CompressionLevel::Balanced)?;

// Open for querying
let cursor = Cursor::new(&compressed);
let mut engine = QueryEngine::from_reader(cursor)?;

// Get statistics (header only - O(1))
let stats = engine.stats();
println!("Rows: {}, Columns: {}", stats.row_count, stats.column_count);

// Select specific columns (partial decompression)
let levels = engine.select_column("log_levels")?;

// Filter with operator
let error_indices = engine.filter_op("log_levels", Op::Eq, "ERROR")?;

// Full query: filter on one column, select from others
let result = engine.query(
    &["timestamps", "ipv4"],  // SELECT
    "log_levels",              // WHERE column
    Op::Eq,                    // operator
    "ERROR",                   // value
)?;

for row in &result.rows {
    println!("{:?}", row.values);
}
```

### Query Performance (Measured)

**5.7 MB log file (100,000 lines), Apple M3:**

| Operation | Time | Notes |
|-----------|------|-------|
| Stats (header only) | **11ms** | O(1) metadata read |
| Filter: log_levels=ERROR | **16ms** | Typed u8 scan, 20K matches |
| Filter: timestamps>= | **28ms** | Typed i64 scan, 70K matches |
| Filter: ipv4= | **7ms** | Typed u32 scan |

**Columnar Compression (Measured):**

| Column | 100K entries | Compressed | Ratio |
|--------|--------------|------------|-------|
| timestamps | ~2 MB | **151 bytes** | 0.007% |
| log_levels | ~500 KB | 34 KB | 6.8% |
| ipv4 | ~1.5 MB | 249 KB | 16.6% |

## Benchmarks

Tested on Apple M3 (arm64), macOS, Rust 1.84.0

### Random Log Data

| Lines | Size | ALICE-Text | gzip -9 | zstd -19 |
|-------|------|------------|---------|----------|
| 1K | 52 KB | 43.0% | 25.6% | 23.6% |
| 10K | 515 KB | 39.3% | 24.0% | 21.6% |
| 100K | 5.0 MB | 39.5% | 23.8% | 20.0% |

### Structured Log Data (Sequential Timestamps)

| Lines | Size | ALICE-Text | gzip -9 | zstd -19 |
|-------|------|------------|---------|----------|
| 100K | 6.5 MB | 34.2% | 12.9% | 10.9% |

*Lower percentage = better compression. Ratio = compressed size / original size.*

### Speed (100K lines, 5 MB)

| Operation | ALICE-Text |
|-----------|------------|
| Compression | ~340 ms (~15 MB/s) |
| Decompression | ~350 ms (~14 MB/s) |

### Notes

- ALICE-Text achieves **34-43% compression ratio** on typical log data
- General-purpose compressors (gzip, zstd) achieve better ratios on raw text
- ALICE-Text's advantage is in **pattern-aware columnar storage** and **type-specific encoding**
- For maximum compression, use gzip or zstd directly
- ALICE-Text is useful when you need structured access to log components

## File Format

ALICE-Text files use the `.atxt` extension.

```
┌────────────────────────────────────────────────────────────┐
│ Magic: "ALICETXT" (8 bytes)                                │
├────────────────────────────────────────────────────────────┤
│ Version: 2.0 (2 bytes)                                     │
├────────────────────────────────────────────────────────────┤
│ Header (24 bytes)                                          │
│   - Original length (8 bytes)                              │
│   - Compression mode (1 byte)                              │
│   - Pattern count (4 bytes)                                │
│   - Skeleton length (4 bytes)                              │
├────────────────────────────────────────────────────────────┤
│ Compressed Payload (Zstd)                                  │
│   - Skeleton tokens (binary)                               │
│   - Columnar data (Bincode serialized)                     │
│     - timestamps (delta-encoded)                           │
│     - ipv4_addrs (Vec<u32>)                                │
│     - ipv6_addrs (Vec<u128>)                               │
│     - uuids (Vec<u128>)                                    │
│     - log_levels (Vec<u8>)                                 │
│     - ... other columns                                    │
└────────────────────────────────────────────────────────────┘
```

## Build Configuration

Optimized for maximum performance:

```toml
# Cargo.toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
```

Uses mimalloc allocator for improved memory allocation performance.

## Dependencies

**Rust:**
- zstd - Compression
- bincode - Binary serialization
- chrono - Timestamp parsing
- regex - Pattern matching
- mimalloc - High-performance allocator
- clap - CLI argument parsing

**Python (optional):**
- maturin - Build system
- pyo3 - Python bindings

## License

BSL 1.1 (Business Source License 1.1)

- Non-commercial, personal, and research use: Free
- Commercial SaaS use: Requires paid license
- Change Date: 2028-01-31 (converts to MIT License)

See [LICENSE](LICENSE) for details.

## Author

Moroya Sakamoto

## See Also

- [ALICE-Zip](https://github.com/ext-sakamoro/ALICE-Zip) - Procedural generation compression
- [ALICE-DB](https://github.com/ext-sakamoro/ALICE-DB) - Model-based database
