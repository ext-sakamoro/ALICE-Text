//! Query Engine for ALICE-Text v3 (Tuned)
//!
//! Optimizations:
//! - **Typed Filtering**: Compares raw primitives (u8, u32, f64) instead of Strings
//! - **Parallel Decompression**: Uses Rayon to fetch columns simultaneously
//! - **Memory Mapping**: Uses mmap for zero-copy random access
//!
//! ## Example
//!
//! ```rust,ignore
//! use alice_text::QueryEngine;
//!
//! // Open with mmap (zero-copy)
//! let engine = QueryEngine::open("server.atxt")?;
//!
//! // Filter without String allocation
//! let errors = engine.filter_op("log_levels", Op::Eq, "ERROR")?;
//!
//! // Parallel column fetch
//! let result = engine.query(&["timestamps", "ipv4"], "log_levels", Op::Eq, "ERROR")?;
//! ```

use crate::format_v3::{
    ColumnType, CompressionLevel, FormatV3Metadata, FormatV3Writer, PartialPayload,
};
use crate::columnar_encoder::LogLevel;
use crate::{ALICETextError, Result};
use chrono::NaiveDateTime;
use memmap2::Mmap;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::Arc;

/// Query result row
#[derive(Debug, Clone)]
pub struct QueryRow {
    pub values: HashMap<String, String>,
}

/// Query result set
#[derive(Debug)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<QueryRow>,
}

impl QueryResult {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn column_values(&self, column: &str) -> Vec<&str> {
        self.rows
            .iter()
            .filter_map(|row| row.values.get(column).map(|s| s.as_str()))
            .collect()
    }

    pub fn to_table(&self) -> String {
        if self.rows.is_empty() {
            return "(empty result)".to_string();
        }

        let mut output = String::new();
        output.push_str(&self.columns.join("\t"));
        output.push('\n');
        output.push_str(&"-".repeat(self.columns.len() * 20));
        output.push('\n');

        for row in &self.rows {
            let values: Vec<&str> = self.columns
                .iter()
                .map(|c| row.values.get(c).map(|s| s.as_str()).unwrap_or(""))
                .collect();
            output.push_str(&values.join("\t"));
            output.push('\n');
        }

        output
    }
}

/// Comparison operators
#[derive(Debug, Clone, Copy)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
    StartsWith,
    EndsWith,
}

/// Column statistics
#[derive(Debug, Clone)]
pub struct ColumnStats {
    pub name: String,
    pub col_type: ColumnType,
    pub row_count: u32,
    pub compressed_size: u32,
}

/// File statistics
#[derive(Debug, Clone)]
pub struct FileStats {
    pub original_size: u64,
    pub compressed_size: u64,
    pub compression_ratio: f64,
    pub row_count: u64,
    pub column_count: usize,
    pub columns: Vec<ColumnStats>,
}

/// Query Engine with Memory Mapping (Optimized)
pub struct QueryEngine<S: QuerySource> {
    source: S,
    metadata: FormatV3Metadata,
}

/// Trait for different data sources
pub trait QuerySource: Send + Sync {
    fn as_slice(&self) -> &[u8];
}

/// Memory-mapped file source (zero-copy)
pub struct MmapSource {
    mmap: Arc<Mmap>,
}

impl QuerySource for MmapSource {
    fn as_slice(&self) -> &[u8] {
        &self.mmap[..]
    }
}

/// In-memory buffer source (for Cursor/tests)
pub struct BufferSource {
    data: Arc<Vec<u8>>,
}

impl QuerySource for BufferSource {
    fn as_slice(&self) -> &[u8] {
        &self.data[..]
    }
}

impl QueryEngine<MmapSource> {
    /// Open a file with memory mapping for maximum speed
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path.as_ref()).map_err(ALICETextError::Io)?;
        let mmap = unsafe {
            Mmap::map(&file).map_err(ALICETextError::Io)?
        };

        let mut cursor = Cursor::new(&mmap[..]);
        let metadata = FormatV3Metadata::read_from(&mut cursor)?;

        Ok(Self {
            source: MmapSource { mmap: Arc::new(mmap) },
            metadata,
        })
    }
}

impl QueryEngine<BufferSource> {
    /// Create from in-memory data (for tests/Cursor compatibility)
    pub fn from_reader<R: Read>(mut reader: R) -> Result<Self> {
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;

        let mut cursor = Cursor::new(&data[..]);
        let metadata = FormatV3Metadata::read_from(&mut cursor)?;

        Ok(Self {
            source: BufferSource { data: Arc::new(data) },
            metadata,
        })
    }
}

impl<S: QuerySource> QueryEngine<S> {
    /// Get file statistics (header only - O(1))
    pub fn stats(&self) -> FileStats {
        let compressed_size = self.metadata.compressed_size();
        let original_size = self.metadata.header.original_length;

        FileStats {
            original_size,
            compressed_size,
            compression_ratio: if original_size > 0 {
                compressed_size as f64 / original_size as f64
            } else {
                0.0
            },
            row_count: self.metadata.header.row_count,
            column_count: self.metadata.columns.len(),
            columns: self.metadata.columns.iter().map(|c| ColumnStats {
                name: c.col_type.name().to_string(),
                col_type: c.col_type,
                row_count: c.row_count,
                compressed_size: c.compressed_size,
            }).collect(),
        }
    }

    /// Get available column names
    pub fn columns(&self) -> Vec<&str> {
        self.metadata.column_names()
    }

    /// Check if column exists
    pub fn has_column(&self, name: &str) -> bool {
        self.metadata.columns.iter().any(|c| c.col_type.name() == name)
    }

    /// Read a single column with String conversion (backward compatible)
    pub fn select_column(&self, name: &str) -> Result<Vec<String>> {
        let col_type = self.name_to_type(name)?;
        let partial = self.read_raw_column(col_type)?;
        self.partial_to_strings(&partial, col_type)
    }

    /// Read multiple columns (parallel decompression)
    pub fn select_columns(&self, names: &[&str]) -> Result<QueryResult> {
        let col_types: Vec<ColumnType> = names
            .iter()
            .map(|n| self.name_to_type(n))
            .collect::<Result<Vec<_>>>()?;

        // Parallel column fetch using Rayon
        let partials: Result<Vec<PartialPayload>> = col_types
            .par_iter()
            .map(|&ct| self.read_raw_column(ct))
            .collect();
        let partials = partials?;

        // Get max row count
        let max_rows = col_types.iter()
            .filter_map(|ct| self.metadata.get_column(*ct))
            .map(|e| e.row_count as usize)
            .max()
            .unwrap_or(0);

        // Build rows
        let mut result = QueryResult {
            columns: names.iter().map(|s| s.to_string()).collect(),
            rows: Vec::with_capacity(max_rows),
        };

        for i in 0..max_rows {
            let mut row = QueryRow { values: HashMap::new() };
            for (j, name) in names.iter().enumerate() {
                if let Some(value) = self.get_value_at(&partials[j], col_types[j], i) {
                    row.values.insert(name.to_string(), value);
                }
            }
            result.rows.push(row);
        }

        Ok(result)
    }

    /// Optimized filter: Scans raw primitives without String allocation
    pub fn filter_op(&self, column: &str, op: Op, value: &str) -> Result<Vec<usize>> {
        let col_type = self.name_to_type(column)?;
        let partial = self.read_raw_column(col_type)?;

        // Typed comparison dispatch - no String allocations in hot loop!
        match col_type {
            ColumnType::LogLevels => {
                let target = LogLevel::parse_level(value) as u8;
                if let Some(data) = &partial.log_levels {
                    Ok(self.scan_primitive(data, op, target))
                } else {
                    Ok(Vec::new())
                }
            }
            ColumnType::IPv4 => {
                let target = self.parse_ipv4(value)?;
                if let Some(data) = &partial.ipv4_addrs {
                    Ok(self.scan_primitive(data, op, target))
                } else {
                    Ok(Vec::new())
                }
            }
            ColumnType::IPv6 => {
                let target = self.parse_ipv6(value)?;
                if let Some(data) = &partial.ipv6_addrs {
                    Ok(self.scan_primitive(data, op, target))
                } else {
                    Ok(Vec::new())
                }
            }
            ColumnType::Numbers => {
                let target = value.parse::<f64>().unwrap_or(0.0);
                if let Some(data) = &partial.numbers {
                    Ok(self.scan_f64(data, op, target))
                } else {
                    Ok(Vec::new())
                }
            }
            ColumnType::UUIDs => {
                let target = self.parse_uuid(value)?;
                if let Some(data) = &partial.uuids {
                    Ok(self.scan_primitive(data, op, target))
                } else {
                    Ok(Vec::new())
                }
            }
            ColumnType::Timestamps => {
                // Typed timestamp filtering: parse query ONCE, compare as i64
                let target_ms = self.parse_query_timestamp(value)?;
                if let Some(ts_col) = &partial.timestamps {
                    // Get prefix sums (absolute timestamps in ms) - pure numeric computation
                    let timestamps_i64 = ts_col.prepare_for_read();
                    Ok(self.scan_primitive(&timestamps_i64, op, target_ms))
                } else {
                    Ok(Vec::new())
                }
            }
            _ => {
                // Fallback for string types (emails, urls, paths, etc.)
                let strings = self.partial_to_strings(&partial, col_type)?;
                Ok(self.scan_strings(&strings, op, value))
            }
        }
    }

    /// Filter with closure (legacy compatibility)
    pub fn filter<F>(&self, column: &str, predicate: F) -> Result<Vec<usize>>
    where
        F: Fn(&str) -> bool,
    {
        let values = self.select_column(column)?;
        Ok(values
            .iter()
            .enumerate()
            .filter(|(_, v)| predicate(v))
            .map(|(i, _)| i)
            .collect())
    }

    /// Select values at specific indices
    pub fn select_at(&self, column: &str, indices: &[usize]) -> Result<Vec<String>> {
        let all_values = self.select_column(column)?;
        Ok(indices
            .iter()
            .filter_map(|&i| all_values.get(i).cloned())
            .collect())
    }

    /// Full query: filter on one column, select from others (parallel)
    pub fn query(
        &self,
        select_columns: &[&str],
        filter_column: &str,
        op: Op,
        filter_value: &str,
    ) -> Result<QueryResult> {
        // Step 1: Filter using typed scan (fast, no String allocation)
        let indices = self.filter_op(filter_column, op, filter_value)?;

        if indices.is_empty() {
            return Ok(QueryResult {
                columns: select_columns.iter().map(|s| s.to_string()).collect(),
                rows: Vec::new(),
            });
        }

        // Step 2: Fetch selected columns in PARALLEL
        let col_types: Vec<ColumnType> = select_columns
            .iter()
            .map(|n| self.name_to_type(n))
            .collect::<Result<Vec<_>>>()?;

        let partials: Result<Vec<PartialPayload>> = col_types
            .par_iter()
            .map(|&ct| self.read_raw_column(ct))
            .collect();
        let partials = partials?;

        // Step 3: Materialize only matching rows (pinpoint extraction)
        let mut result = QueryResult {
            columns: select_columns.iter().map(|s| s.to_string()).collect(),
            rows: Vec::with_capacity(indices.len()),
        };

        for &idx in &indices {
            let mut row = QueryRow { values: HashMap::new() };
            for (j, name) in select_columns.iter().enumerate() {
                if let Some(val) = self.get_value_at(&partials[j], col_types[j], idx) {
                    row.values.insert(name.to_string(), val);
                }
            }
            result.rows.push(row);
        }

        Ok(result)
    }

    /// Decompress entire file
    pub fn decompress_all(&self) -> Result<String> {
        FormatV3Writer::decompress(self.source.as_slice())
    }

    // === Private: Typed Scanners ===

    /// Generic scanner for primitive types (u8, u32, u128)
    /// Compiles down to SIMD instructions in release mode
    #[inline]
    fn scan_primitive<T>(&self, data: &[T], op: Op, target: T) -> Vec<usize>
    where
        T: PartialOrd + Copy,
    {
        data.iter()
            .enumerate()
            .filter(|(_, &v)| match op {
                Op::Eq => v == target,
                Op::Ne => v != target,
                Op::Lt => v < target,
                Op::Le => v <= target,
                Op::Gt => v > target,
                Op::Ge => v >= target,
                Op::Contains | Op::StartsWith | Op::EndsWith => false,
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Scanner for f64 (needs special handling for NaN)
    #[inline]
    fn scan_f64(&self, data: &[f64], op: Op, target: f64) -> Vec<usize> {
        data.iter()
            .enumerate()
            .filter(|(_, &v)| match op {
                Op::Eq => (v - target).abs() < f64::EPSILON,
                Op::Ne => (v - target).abs() >= f64::EPSILON,
                Op::Lt => v < target,
                Op::Le => v <= target,
                Op::Gt => v > target,
                Op::Ge => v >= target,
                Op::Contains | Op::StartsWith | Op::EndsWith => false,
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Scanner for string types
    #[inline]
    fn scan_strings(&self, data: &[String], op: Op, target: &str) -> Vec<usize> {
        data.iter()
            .enumerate()
            .filter(|(_, v)| match op {
                Op::Eq => v.as_str() == target,
                Op::Ne => v.as_str() != target,
                Op::Lt => v.as_str() < target,
                Op::Le => v.as_str() <= target,
                Op::Gt => v.as_str() > target,
                Op::Ge => v.as_str() >= target,
                Op::Contains => v.contains(target),
                Op::StartsWith => v.starts_with(target),
                Op::EndsWith => v.ends_with(target),
            })
            .map(|(i, _)| i)
            .collect()
    }

    // === Private: Column Reading ===

    fn read_raw_column(&self, col_type: ColumnType) -> Result<PartialPayload> {
        let mut cursor = Cursor::new(self.source.as_slice());
        FormatV3Writer::read_columns(&mut cursor, &self.metadata, &[col_type])
    }

    fn name_to_type(&self, name: &str) -> Result<ColumnType> {
        match name {
            "skeleton" => Ok(ColumnType::Skeleton),
            "timestamps" => Ok(ColumnType::Timestamps),
            "ipv4" => Ok(ColumnType::IPv4),
            "ipv6" => Ok(ColumnType::IPv6),
            "log_levels" => Ok(ColumnType::LogLevels),
            "numbers" => Ok(ColumnType::Numbers),
            "uuids" => Ok(ColumnType::UUIDs),
            "emails" => Ok(ColumnType::Emails),
            "urls" => Ok(ColumnType::URLs),
            "paths" => Ok(ColumnType::Paths),
            "date_days" => Ok(ColumnType::DateDays),
            "dates_raw" => Ok(ColumnType::DatesRaw),
            "time_ms" => Ok(ColumnType::TimeMs),
            "times_raw" => Ok(ColumnType::TimesRaw),
            "hex_values" => Ok(ColumnType::HexValues),
            "others" => Ok(ColumnType::Others),
            "placeholder_map" => Ok(ColumnType::PlaceholderMap),
            "timestamps_raw" => Ok(ColumnType::TimestampsRaw),
            _ => Err(ALICETextError::DecompressionError(
                format!("Unknown column: {}", name),
            )),
        }
    }

    // === Private: Parsing ===

    fn parse_ipv4(&self, s: &str) -> Result<u32> {
        s.parse::<std::net::Ipv4Addr>()
            .map(u32::from)
            .map_err(|_| ALICETextError::DecompressionError(format!("Invalid IPv4: {}", s)))
    }

    fn parse_ipv6(&self, s: &str) -> Result<u128> {
        s.parse::<std::net::Ipv6Addr>()
            .map(u128::from)
            .map_err(|_| ALICETextError::DecompressionError(format!("Invalid IPv6: {}", s)))
    }

    fn parse_uuid(&self, s: &str) -> Result<u128> {
        let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if hex.len() != 32 {
            return Err(ALICETextError::DecompressionError(format!("Invalid UUID: {}", s)));
        }
        u128::from_str_radix(&hex, 16)
            .map_err(|_| ALICETextError::DecompressionError(format!("Invalid UUID hex: {}", s)))
    }

    /// Parse query timestamp string to Unix milliseconds (i64)
    /// Supports multiple formats: "YYYY-MM-DD HH:MM:SS", "YYYY-MM-DDTHH:MM:SS", etc.
    fn parse_query_timestamp(&self, s: &str) -> Result<i64> {
        // Try common formats
        let formats = [
            "%Y-%m-%d %H:%M:%S",      // 2024-01-15 10:30:45
            "%Y-%m-%dT%H:%M:%S",      // 2024-01-15T10:30:45
            "%Y-%m-%d %H:%M:%S%.f",   // 2024-01-15 10:30:45.123
            "%Y-%m-%dT%H:%M:%S%.f",   // 2024-01-15T10:30:45.123
            "%Y-%m-%d",               // 2024-01-15 (assumes 00:00:00)
        ];

        for fmt in &formats {
            if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
                return Ok(dt.and_utc().timestamp_millis());
            }
        }

        // Try date-only format with time defaulting to 00:00:00
        if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            let dt = date.and_hms_opt(0, 0, 0).unwrap();
            return Ok(dt.and_utc().timestamp_millis());
        }

        Err(ALICETextError::DecompressionError(
            format!("Invalid timestamp format: {}. Expected YYYY-MM-DD HH:MM:SS", s)
        ))
    }

    // === Private: Value Extraction ===

    fn partial_to_strings(&self, partial: &PartialPayload, col_type: ColumnType) -> Result<Vec<String>> {
        Ok(match col_type {
            ColumnType::LogLevels => {
                partial.log_level_strings().unwrap_or_default()
            }
            ColumnType::IPv4 => {
                partial.ipv4_strings().unwrap_or_default()
            }
            ColumnType::IPv6 => {
                partial.ipv6_addrs.as_ref().map(|addrs| {
                    addrs.iter().map(|&ip| std::net::Ipv6Addr::from(ip).to_string()).collect()
                }).unwrap_or_default()
            }
            ColumnType::Timestamps => {
                partial.timestamp_strings().unwrap_or_default()
            }
            ColumnType::Numbers => {
                partial.numbers.as_ref().map(|nums| {
                    nums.iter().map(|n| {
                        if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
                            (*n as i64).to_string()
                        } else {
                            n.to_string()
                        }
                    }).collect()
                }).unwrap_or_default()
            }
            ColumnType::UUIDs => {
                partial.uuids.as_ref().map(|uuids| {
                    uuids.iter().map(|&uuid| {
                        let hex = format!("{:032x}", uuid);
                        format!("{}-{}-{}-{}-{}",
                            &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
                    }).collect()
                }).unwrap_or_default()
            }
            ColumnType::Emails => partial.emails.clone().unwrap_or_default(),
            ColumnType::URLs => partial.urls.clone().unwrap_or_default(),
            ColumnType::Paths => partial.paths.clone().unwrap_or_default(),
            _ => Vec::new(),
        })
    }

    fn get_value_at(&self, partial: &PartialPayload, col_type: ColumnType, index: usize) -> Option<String> {
        match col_type {
            ColumnType::LogLevels => {
                partial.log_levels.as_ref()?.get(index).map(|&l| {
                    match l {
                        0 => "TRACE", 1 => "DEBUG", 2 => "INFO", 3 => "WARN",
                        4 => "ERROR", 5 => "FATAL", 6 => "CRITICAL", _ => "UNKNOWN",
                    }.to_string()
                })
            }
            ColumnType::IPv4 => {
                partial.ipv4_addrs.as_ref()?.get(index).map(|&ip| {
                    std::net::Ipv4Addr::from(ip).to_string()
                })
            }
            ColumnType::IPv6 => {
                partial.ipv6_addrs.as_ref()?.get(index).map(|&ip| {
                    std::net::Ipv6Addr::from(ip).to_string()
                })
            }
            ColumnType::Timestamps => {
                let ts = partial.timestamps.as_ref()?;
                let prefix_sums = ts.prepare_for_read();
                ts.get_delta(index, &prefix_sums)
            }
            ColumnType::Numbers => {
                partial.numbers.as_ref()?.get(index).map(|&n| {
                    if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
                        (n as i64).to_string()
                    } else {
                        n.to_string()
                    }
                })
            }
            ColumnType::UUIDs => {
                partial.uuids.as_ref()?.get(index).map(|&uuid| {
                    let hex = format!("{:032x}", uuid);
                    format!("{}-{}-{}-{}-{}",
                        &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
                })
            }
            ColumnType::Emails => partial.emails.as_ref()?.get(index).cloned(),
            ColumnType::URLs => partial.urls.as_ref()?.get(index).cloned(),
            ColumnType::Paths => partial.paths.as_ref()?.get(index).cloned(),
            _ => None,
        }
    }
}

/// Query builder for fluent API
pub struct QueryBuilder<'a, S: QuerySource> {
    engine: &'a QueryEngine<S>,
    select_cols: Vec<String>,
    filter_col: Option<String>,
    filter_op: Option<Op>,
    filter_value: Option<String>,
}

impl<'a, S: QuerySource> QueryBuilder<'a, S> {
    pub fn new(engine: &'a QueryEngine<S>) -> Self {
        Self {
            engine,
            select_cols: Vec::new(),
            filter_col: None,
            filter_op: None,
            filter_value: None,
        }
    }

    pub fn select(mut self, columns: &[&str]) -> Self {
        self.select_cols = columns.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn filter(mut self, column: &str, op: Op, value: &str) -> Self {
        self.filter_col = Some(column.to_string());
        self.filter_op = Some(op);
        self.filter_value = Some(value.to_string());
        self
    }

    pub fn execute(self) -> Result<QueryResult> {
        let select_refs: Vec<&str> = self.select_cols.iter().map(|s| s.as_str()).collect();

        if let (Some(filter_col), Some(op), Some(filter_value)) =
            (self.filter_col, self.filter_op, self.filter_value)
        {
            self.engine.query(&select_refs, &filter_col, op, &filter_value)
        } else {
            self.engine.select_columns(&select_refs)
        }
    }
}

/// Convenience function to compress text with v3 format
pub fn compress_v3(text: &str, level: CompressionLevel) -> Result<Vec<u8>> {
    FormatV3Writer::new(level).compress(text)
}

/// Convenience function to decompress v3 format
pub fn decompress_v3(data: &[u8]) -> Result<String> {
    FormatV3Writer::decompress(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn create_test_data() -> Vec<u8> {
        let text = "2024-01-15 10:30:45 INFO User1 logged in from 192.168.1.1\n\
                    2024-01-15 10:30:46 ERROR Connection failed from 192.168.1.2\n\
                    2024-01-15 10:30:47 WARN High memory from 192.168.1.3\n\
                    2024-01-15 10:30:48 INFO User2 logged in from 192.168.1.4\n\
                    2024-01-15 10:30:49 ERROR Timeout from 192.168.1.5";
        compress_v3(text, CompressionLevel::Fast).unwrap()
    }

    #[test]
    fn test_query_engine_stats() {
        let data = create_test_data();
        let engine = QueryEngine::from_reader(Cursor::new(&data)).unwrap();

        let stats = engine.stats();
        assert!(stats.original_size > 0);
        assert!(stats.column_count > 0);
        assert_eq!(stats.row_count, 5);
    }

    #[test]
    fn test_query_engine_columns() {
        let data = create_test_data();
        let engine = QueryEngine::from_reader(Cursor::new(&data)).unwrap();

        let columns = engine.columns();
        assert!(columns.contains(&"log_levels"));
        assert!(columns.contains(&"ipv4"));
        assert!(columns.contains(&"timestamps"));
    }

    #[test]
    fn test_select_column() {
        let data = create_test_data();
        let engine = QueryEngine::from_reader(Cursor::new(&data)).unwrap();

        let levels = engine.select_column("log_levels").unwrap();
        assert_eq!(levels.len(), 5);
        assert!(levels.contains(&"INFO".to_string()));
        assert!(levels.contains(&"ERROR".to_string()));
        assert!(levels.contains(&"WARN".to_string()));
    }

    #[test]
    fn test_typed_filter() {
        let data = create_test_data();
        let engine = QueryEngine::from_reader(Cursor::new(&data)).unwrap();

        // Typed filter on log_levels (u8 comparison)
        let error_indices = engine.filter_op("log_levels", Op::Eq, "ERROR").unwrap();
        assert_eq!(error_indices.len(), 2);
    }

    #[test]
    fn test_typed_filter_ipv4() {
        let data = create_test_data();
        let engine = QueryEngine::from_reader(Cursor::new(&data)).unwrap();

        // Typed filter on IPv4 (u32 comparison)
        let indices = engine.filter_op("ipv4", Op::Eq, "192.168.1.2").unwrap();
        assert_eq!(indices.len(), 1);
    }

    #[test]
    fn test_parallel_query() {
        let data = create_test_data();
        let engine = QueryEngine::from_reader(Cursor::new(&data)).unwrap();

        // Parallel column fetch + typed filter
        let result = engine.query(
            &["log_levels", "ipv4"],
            "log_levels",
            Op::Eq,
            "ERROR",
        ).unwrap();

        assert_eq!(result.len(), 2);
        for row in &result.rows {
            assert_eq!(row.values.get("log_levels").unwrap(), "ERROR");
        }
    }

    #[test]
    fn test_query_builder() {
        let data = create_test_data();
        let engine = QueryEngine::from_reader(Cursor::new(&data)).unwrap();

        let result = QueryBuilder::new(&engine)
            .select(&["log_levels", "ipv4"])
            .filter("log_levels", Op::Eq, "ERROR")
            .execute()
            .unwrap();

        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_decompress_all() {
        let text = "2024-01-15 INFO Test message";
        let data = compress_v3(text, CompressionLevel::Fast).unwrap();

        let engine = QueryEngine::from_reader(Cursor::new(&data)).unwrap();
        let decompressed = engine.decompress_all().unwrap();
        assert_eq!(text, decompressed);
    }
}
