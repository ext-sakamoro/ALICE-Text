//! Columnar Encoder - Struct of Arrays (SoA) Implementation
//!
//! Organizes data by pattern type for better compression.
//! - Same types are stored together (reduces entropy)
//! - Type-specific encodings (IP as u32, LogLevel as u8, etc.)
//! - Delta encoding for timestamps (massive compression gains)

use crate::tuned_pattern_learner::{PatternType, TunedPatternLearner};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::Ipv4Addr;

/// Log level encoded as u8
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[repr(u8)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
    Fatal = 5,
    Critical = 6,
    Unknown = 7,
}

impl LogLevel {
    pub fn parse_level(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "TRACE" => Self::Trace,
            "DEBUG" => Self::Debug,
            "INFO" => Self::Info,
            "WARN" | "WARNING" => Self::Warn,
            "ERROR" => Self::Error,
            "FATAL" => Self::Fatal,
            "CRITICAL" => Self::Critical,
            _ => Self::Unknown,
        }
    }

    pub fn to_str(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Fatal => "FATAL",
            Self::Critical => "CRITICAL",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Supported timestamp formats for parsing
/// Ordered by specificity (most specific first)
const TIMESTAMP_FORMATS_NAIVE: &[&str] = &[
    "%Y-%m-%d %H:%M:%S%.f",       // 2024-01-15 10:30:45.123
    "%Y-%m-%d %H:%M:%S",          // 2024-01-15 10:30:45
    "%Y-%m-%dT%H:%M:%S%.f",       // 2024-01-15T10:30:45.123
    "%Y-%m-%dT%H:%M:%S",          // 2024-01-15T10:30:45
];

/// Timestamp formats with timezone (Z or +09:00)
const TIMESTAMP_FORMATS_TZ: &[&str] = &[
    "%Y-%m-%dT%H:%M:%S%.f%:z",    // 2024-01-15T10:30:45.123+09:00
    "%Y-%m-%dT%H:%M:%S%:z",       // 2024-01-15T10:30:45+09:00
    "%Y-%m-%dT%H:%M:%S%.fZ",      // 2024-01-15T10:30:45.123Z
    "%Y-%m-%dT%H:%M:%SZ",         // 2024-01-15T10:30:45Z
];

/// Timestamp with delta encoding support
///
/// Delta encoding dramatically reduces size for time-series data:
/// - Before: "2024-01-15 10:30:45" (19 bytes) × N
/// - After: base + [0, 1000, 1000, ...] (few bytes each after Zstd)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimestampColumn {
    /// First timestamp as full string (for reconstruction)
    pub base: Option<String>,
    /// Base timestamp in milliseconds (epoch)
    pub base_ms: Option<i64>,
    /// Delta values in milliseconds from previous timestamp
    /// Delta[0] is always 0 (base), Delta[n] = Timestamp[n] - Timestamp[n-1]
    pub deltas: Vec<i64>,
    /// Fallback: raw strings for timestamps that couldn't be parsed
    pub raw: Vec<String>,
    /// Cached format type and index for fast parsing
    #[serde(default)]
    pub cached_format_idx: Option<CachedFormatType>,
    /// Last timestamp in milliseconds (for O(1) delta calculation)
    #[serde(default)]
    pub last_ms: i64,
    /// Timezone offset in seconds (e.g., +09:00 = 32400)
    /// None for naive timestamps, Some(0) for Z/UTC
    #[serde(default)]
    pub base_offset_secs: Option<i32>,
}



/// Cached format type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum CachedFormatType {
    #[default]
    None,
    Naive(usize),
    Tz(usize),
}

impl TimestampColumn {
    /// Try to parse a timestamp string into milliseconds
    /// Uses cached format if available for O(1) parsing after first success
    /// Also captures timezone offset for the base timestamp
    fn parse_timestamp(&mut self, s: &str) -> Option<i64> {
        use chrono::DateTime;

        // Try cached format first (fast path)
        match self.cached_format_idx {
            Some(CachedFormatType::Naive(idx)) => {
                if let Some(fmt) = TIMESTAMP_FORMATS_NAIVE.get(idx) {
                    if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
                        return Some(dt.and_utc().timestamp_millis());
                    }
                }
            }
            Some(CachedFormatType::Tz(idx)) => {
                if let Some(fmt) = TIMESTAMP_FORMATS_TZ.get(idx) {
                    if let Ok(dt) = DateTime::parse_from_str(s, fmt) {
                        // Capture offset for base timestamp
                        if self.base_offset_secs.is_none() {
                            self.base_offset_secs = Some(dt.offset().local_minus_utc());
                        }
                        return Some(dt.timestamp_millis());
                    }
                }
            }
            _ => {}
        }

        // Try timezone-aware formats first (more specific)
        for (idx, fmt) in TIMESTAMP_FORMATS_TZ.iter().enumerate() {
            if let Ok(dt) = DateTime::parse_from_str(s, fmt) {
                self.cached_format_idx = Some(CachedFormatType::Tz(idx));
                // Capture offset for base timestamp
                if self.base_offset_secs.is_none() {
                    self.base_offset_secs = Some(dt.offset().local_minus_utc());
                }
                return Some(dt.timestamp_millis());
            }
        }

        // Try naive formats
        for (idx, fmt) in TIMESTAMP_FORMATS_NAIVE.iter().enumerate() {
            if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
                self.cached_format_idx = Some(CachedFormatType::Naive(idx));
                return Some(dt.and_utc().timestamp_millis());
            }
        }

        None
    }

    /// Add a timestamp, using delta encoding if possible
    /// Returns (is_delta, index) where index is into deltas or raw array
    pub fn add(&mut self, text: &str) -> (bool, usize) {
        if let Some(ts_ms) = self.parse_timestamp(text) {
            let delta_idx = self.deltas.len();
            if self.base_ms.is_none() {
                // First timestamp: store as base
                self.base = Some(text.to_string());
                self.base_ms = Some(ts_ms);
                self.last_ms = ts_ms;
                self.deltas.push(0); // Delta from self is 0
            } else {
                // Subsequent: store delta from previous (O(1) now)
                let delta = ts_ms - self.last_ms;
                self.last_ms = ts_ms;
                self.deltas.push(delta);
            }
            (true, delta_idx)
        } else {
            // Can't parse: store as raw string
            let raw_idx = self.raw.len();
            self.raw.push(text.to_string());
            (false, raw_idx)
        }
    }

    /// Precompute prefix sums for O(1) timestamp lookup
    /// Call this once after deserialization before accessing timestamps
    pub fn prepare_for_read(&self) -> Vec<i64> {
        let base = self.base_ms.unwrap_or(0);
        let mut prefix_sums = Vec::with_capacity(self.deltas.len());
        let mut sum = base;
        for delta in &self.deltas {
            sum += delta;
            prefix_sums.push(sum);
        }
        prefix_sums
    }

    /// Get delta-encoded timestamp by index (O(1) with precomputed prefix sums)
    pub fn get_delta(&self, delta_idx: usize, prefix_sums: &[i64]) -> Option<String> {
        use chrono::FixedOffset;

        let base_str = self.base.as_ref()?;
        let total_ms = *prefix_sums.get(delta_idx)?;

        // Reconstruct DateTime from milliseconds
        let dt_utc = chrono::DateTime::from_timestamp_millis(total_ms)?;

        // Check if we have a timezone offset to apply
        if let Some(offset_secs) = self.base_offset_secs {
            // Apply timezone offset
            let offset = FixedOffset::east_opt(offset_secs)?;
            let dt_local = dt_utc.with_timezone(&offset);

            // Format with timezone
            if base_str.ends_with('Z') {
                // Original was Zulu time
                Some(dt_utc.format("%Y-%m-%dT%H:%M:%SZ").to_string())
            } else if base_str.contains('+') || base_str.contains("-") && base_str.len() > 19 {
                // Original had explicit timezone offset (+09:00 or -05:00)
                Some(dt_local.format("%Y-%m-%dT%H:%M:%S%:z").to_string())
            } else {
                Some(dt_local.format("%Y-%m-%dT%H:%M:%S").to_string())
            }
        } else {
            // Naive timestamp (no timezone info)
            let naive = dt_utc.naive_utc();

            // Detect format from base string
            if base_str.contains('T') {
                Some(naive.format("%Y-%m-%dT%H:%M:%S").to_string())
            } else {
                Some(naive.format("%Y-%m-%d %H:%M:%S").to_string())
            }
        }
    }

    /// Reconstruct timestamp (for backwards compatibility)
    pub fn get(&self, idx: usize) -> Option<String> {
        if idx < self.deltas.len() {
            let prefix_sums = self.prepare_for_read();
            self.get_delta(idx, &prefix_sums)
        } else {
            // Fallback to raw
            self.raw.get(idx - self.deltas.len()).cloned()
        }
    }

    /// Number of timestamps stored
    pub fn len(&self) -> usize {
        self.deltas.len() + self.raw.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.deltas.is_empty() && self.raw.is_empty()
    }
}

/// Skeleton token for binary representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SkeletonToken {
    /// Literal text segment
    Text(String),
    /// Reference to placeholder index
    Ref(u32),
}

/// Columnar payload - Struct of Arrays layout
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnarPayload {
    /// Binary skeleton tokens (faster restore than string parsing)
    pub skeleton_tokens: Vec<SkeletonToken>,

    /// Placeholder order: maps placeholder index to (column_type, column_index)
    pub placeholder_map: Vec<(u8, u32)>,

    // Type-specific columns
    /// Timestamps (delta-encoded where possible)
    pub timestamps: TimestampColumn,

    /// IPv4 addresses as u32 (big-endian)
    pub ipv4_addrs: Vec<u32>,

    /// IPv6 addresses as u128
    #[serde(default)]
    pub ipv6_addrs: Vec<u128>,

    /// Log levels as u8
    pub log_levels: Vec<u8>,

    /// Numbers as f64
    pub numbers: Vec<f64>,

    /// UUIDs as u128 (16 bytes vs 36 bytes string)
    pub uuids: Vec<u128>,

    /// Emails
    pub emails: Vec<String>,

    /// URLs
    pub urls: Vec<String>,

    /// Paths
    pub paths: Vec<String>,

    /// Dates as epoch days (u32, 1970-01-01 = 0)
    #[serde(default)]
    pub date_days: Vec<u32>,

    /// Dates (raw string fallback)
    pub dates: Vec<String>,

    /// Times as milliseconds from midnight (u32)
    #[serde(default)]
    pub time_ms: Vec<u32>,

    /// Times (raw string fallback)
    pub times: Vec<String>,

    /// Hex values
    pub hex_values: Vec<String>,

    /// Other/custom patterns
    pub others: Vec<String>,
}

impl ColumnarPayload {
    pub fn new(skeleton: String) -> Self {
        // Parse skeleton string into binary tokens
        let skeleton_tokens = Self::parse_skeleton(&skeleton);

        Self {
            skeleton_tokens,
            placeholder_map: Vec::new(),
            timestamps: TimestampColumn::default(),
            ipv4_addrs: Vec::new(),
            ipv6_addrs: Vec::new(),
            log_levels: Vec::new(),
            numbers: Vec::new(),
            uuids: Vec::new(),
            emails: Vec::new(),
            urls: Vec::new(),
            paths: Vec::new(),
            date_days: Vec::new(),
            dates: Vec::new(),
            time_ms: Vec::new(),
            times: Vec::new(),
            hex_values: Vec::new(),
            others: Vec::new(),
        }
    }

    /// Parse skeleton string into binary tokens
    fn parse_skeleton(skeleton: &str) -> Vec<SkeletonToken> {
        let mut tokens = Vec::new();
        let mut current_text = String::new();
        let mut chars = skeleton.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '{' {
                // Collect digits
                let mut num_str = String::new();
                while let Some(&next_c) = chars.peek() {
                    if next_c == '}' {
                        chars.next();
                        break;
                    }
                    if next_c.is_ascii_digit() {
                        num_str.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }

                if let Ok(idx) = num_str.parse::<u32>() {
                    // Save accumulated text
                    if !current_text.is_empty() {
                        tokens.push(SkeletonToken::Text(std::mem::take(&mut current_text)));
                    }
                    tokens.push(SkeletonToken::Ref(idx));
                } else {
                    // Not a valid placeholder, keep as text
                    current_text.push('{');
                    current_text.push_str(&num_str);
                }
            } else {
                current_text.push(c);
            }
        }

        // Push remaining text
        if !current_text.is_empty() {
            tokens.push(SkeletonToken::Text(current_text));
        }

        tokens
    }

    /// Add a match to the appropriate column
    pub fn add_match(&mut self, pattern_type: PatternType, text: &str) {
        let (col_type, col_idx) = match pattern_type {
            PatternType::Timestamp => {
                // Use delta encoding for timestamps
                let (is_delta, idx) = self.timestamps.add(text);
                if is_delta {
                    (0u8, idx as u32)  // Delta-encoded: col_type 0
                } else {
                    (13u8, idx as u32) // Raw string: col_type 13
                }
            }
            PatternType::IPv4 => {
                let ip_u32 = parse_ipv4(text).unwrap_or(0);
                self.ipv4_addrs.push(ip_u32);
                (1u8, (self.ipv4_addrs.len() - 1) as u32)
            }
            PatternType::LogLevel => {
                let level = LogLevel::parse_level(text);
                self.log_levels.push(level as u8);
                (2u8, (self.log_levels.len() - 1) as u32)
            }
            PatternType::Number => {
                let num = text.parse::<f64>().unwrap_or(0.0);
                self.numbers.push(num);
                (3u8, (self.numbers.len() - 1) as u32)
            }
            PatternType::UUID => {
                let uuid = parse_uuid(text).unwrap_or(0);
                self.uuids.push(uuid);
                (4u8, (self.uuids.len() - 1) as u32)
            }
            PatternType::Email => {
                self.emails.push(text.to_string());
                (5u8, (self.emails.len() - 1) as u32)
            }
            PatternType::URL => {
                self.urls.push(text.to_string());
                (6u8, (self.urls.len() - 1) as u32)
            }
            PatternType::Path => {
                self.paths.push(text.to_string());
                (7u8, (self.paths.len() - 1) as u32)
            }
            PatternType::Date => {
                // Try to parse as epoch days
                if let Some(days) = parse_date_to_days(text) {
                    self.date_days.push(days);
                    (8u8, (self.date_days.len() - 1) as u32)
                } else {
                    // Fallback to raw string
                    self.dates.push(text.to_string());
                    (14u8, (self.dates.len() - 1) as u32)
                }
            }
            PatternType::Time => {
                // Try to parse as milliseconds from midnight
                if let Some(ms) = parse_time_to_ms(text) {
                    self.time_ms.push(ms);
                    (9u8, (self.time_ms.len() - 1) as u32)
                } else {
                    // Fallback to raw string
                    self.times.push(text.to_string());
                    (15u8, (self.times.len() - 1) as u32)
                }
            }
            PatternType::Hex => {
                self.hex_values.push(text.to_string());
                (10u8, (self.hex_values.len() - 1) as u32)
            }
            PatternType::IPv6 => {
                // Parse IPv6 to u128
                if let Some(ip) = parse_ipv6(text) {
                    self.ipv6_addrs.push(ip);
                    (12u8, (self.ipv6_addrs.len() - 1) as u32)
                } else {
                    // Fallback to string
                    self.others.push(text.to_string());
                    (11u8, (self.others.len() - 1) as u32)
                }
            }
            PatternType::Custom => {
                self.others.push(text.to_string());
                (11u8, (self.others.len() - 1) as u32)
            }
        };

        self.placeholder_map.push((col_type, col_idx));
    }

    /// Get value for placeholder N (optimized with precomputed prefix sums)
    fn get_value_fast(&self, placeholder_idx: usize, ts_prefix_sums: &[i64]) -> Option<String> {
        let (col_type, col_idx) = self.placeholder_map.get(placeholder_idx)?;
        let idx = *col_idx as usize;

        Some(match col_type {
            0 => {
                // Delta-encoded timestamp: O(1) lookup
                self.timestamps.get_delta(idx, ts_prefix_sums)?
            }
            1 => {
                let ip = *self.ipv4_addrs.get(idx)?;
                format_ipv4(ip)
            }
            2 => {
                let level = *self.log_levels.get(idx)?;
                LogLevel::from_u8(level).to_str().to_string()
            }
            3 => {
                let num = *self.numbers.get(idx)?;
                format_number(num)
            }
            4 => {
                let uuid = *self.uuids.get(idx)?;
                format_uuid(uuid)
            }
            5 => self.emails.get(idx)?.clone(),
            6 => self.urls.get(idx)?.clone(),
            7 => self.paths.get(idx)?.clone(),
            8 => {
                // Date as epoch days (u32)
                let days = *self.date_days.get(idx)?;
                format_date_from_days(days)
            }
            9 => {
                // Time as milliseconds from midnight (u32)
                let ms = *self.time_ms.get(idx)?;
                format_time_from_ms(ms)
            }
            10 => self.hex_values.get(idx)?.clone(),
            11 => self.others.get(idx)?.clone(),
            12 => {
                // IPv6 as u128
                let ip = *self.ipv6_addrs.get(idx)?;
                format_ipv6(ip)
            }
            13 => {
                // Raw timestamp string
                self.timestamps.raw.get(idx)?.clone()
            }
            14 => {
                // Raw date string (fallback)
                self.dates.get(idx)?.clone()
            }
            15 => {
                // Raw time string (fallback)
                self.times.get(idx)?.clone()
            }
            _ => return None,
        })
    }

    /// Get value for placeholder N
    pub fn get_value(&self, placeholder_idx: usize) -> Option<String> {
        let ts_prefix_sums = self.timestamps.prepare_for_read();
        self.get_value_fast(placeholder_idx, &ts_prefix_sums)
    }

    /// Restore original text from skeleton tokens and columns
    ///
    /// Uses pre-parsed binary tokens for O(N) performance with zero parsing overhead.
    pub fn restore(&self) -> String {
        // Pre-compute timestamp prefix sums once for O(1) lookup
        let ts_prefix_sums = self.timestamps.prepare_for_read();

        // Estimate capacity from tokens
        let estimated_size: usize = self.skeleton_tokens.iter().map(|t| {
            match t {
                SkeletonToken::Text(s) => s.len(),
                SkeletonToken::Ref(_) => 20, // Average placeholder value length
            }
        }).sum();

        let mut result = String::with_capacity(estimated_size);

        // Direct token iteration - no parsing needed
        for token in &self.skeleton_tokens {
            match token {
                SkeletonToken::Text(text) => {
                    result.push_str(text);
                }
                SkeletonToken::Ref(idx) => {
                    if let Some(value) = self.get_value_fast(*idx as usize, &ts_prefix_sums) {
                        result.push_str(&value);
                    }
                }
            }
        }

        result
    }

    /// Get compression statistics
    pub fn stats(&self) -> HashMap<&'static str, usize> {
        let mut stats = HashMap::new();
        stats.insert("timestamps", self.timestamps.len());
        stats.insert("ipv4", self.ipv4_addrs.len());
        stats.insert("ipv6", self.ipv6_addrs.len());
        stats.insert("log_levels", self.log_levels.len());
        stats.insert("numbers", self.numbers.len());
        stats.insert("uuids", self.uuids.len());
        stats.insert("emails", self.emails.len());
        stats.insert("urls", self.urls.len());
        stats.insert("paths", self.paths.len());
        stats.insert("date_days", self.date_days.len());
        stats.insert("dates_raw", self.dates.len());
        stats.insert("time_ms", self.time_ms.len());
        stats.insert("times_raw", self.times.len());
        stats.insert("hex", self.hex_values.len());
        stats.insert("others", self.others.len());
        stats
    }
}

impl LogLevel {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Trace,
            1 => Self::Debug,
            2 => Self::Info,
            3 => Self::Warn,
            4 => Self::Error,
            5 => Self::Fatal,
            6 => Self::Critical,
            _ => Self::Unknown,
        }
    }
}

/// Parse IPv4 string to u32
fn parse_ipv4(s: &str) -> Option<u32> {
    s.parse::<Ipv4Addr>().ok().map(u32::from)
}

/// Format u32 back to IPv4 string
fn format_ipv4(ip: u32) -> String {
    Ipv4Addr::from(ip).to_string()
}

/// Parse UUID string to u128 (removes dashes)
fn parse_uuid(s: &str) -> Option<u128> {
    // UUID format: 8-4-4-4-12 hex chars with dashes
    // e.g., "550e8400-e29b-41d4-a716-446655440000"
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 32 {
        return None;
    }
    u128::from_str_radix(&hex, 16).ok()
}

/// Format u128 back to UUID string
fn format_uuid(uuid: u128) -> String {
    let hex = format!("{:032x}", uuid);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Format number, preserving integer appearance when possible
fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
        (n as i64).to_string()
    } else {
        n.to_string()
    }
}

/// Parse IPv6 string to u128
fn parse_ipv6(s: &str) -> Option<u128> {
    use std::net::Ipv6Addr;
    s.parse::<Ipv6Addr>().ok().map(u128::from)
}

/// Format u128 back to IPv6 string
fn format_ipv6(ip: u128) -> String {
    use std::net::Ipv6Addr;
    Ipv6Addr::from(ip).to_string()
}

/// Date formats for parsing
const DATE_FORMATS: &[&str] = &[
    "%Y-%m-%d",    // 2024-01-15
    "%Y/%m/%d",    // 2024/01/15
    "%d-%m-%Y",    // 15-01-2024
    "%d/%m/%Y",    // 15/01/2024
];

/// Parse date string to epoch days (days since 1970-01-01)
fn parse_date_to_days(s: &str) -> Option<u32> {
    use chrono::NaiveDate;

    for fmt in DATE_FORMATS {
        if let Ok(date) = NaiveDate::parse_from_str(s, fmt) {
            // Days since Unix epoch (1970-01-01)
            let epoch = NaiveDate::from_ymd_opt(1970, 1, 1)?;
            let days = date.signed_duration_since(epoch).num_days();
            if days >= 0 {
                return Some(days as u32);
            }
        }
    }
    None
}

/// Format epoch days back to date string (YYYY-MM-DD format)
fn format_date_from_days(days: u32) -> String {
    use chrono::NaiveDate;

    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
    let date = epoch + chrono::Duration::days(days as i64);
    date.format("%Y-%m-%d").to_string()
}

/// Time formats for parsing
const TIME_FORMATS: &[&str] = &[
    "%H:%M:%S%.f",  // 10:30:45.123
    "%H:%M:%S",     // 10:30:45
    "%H:%M",        // 10:30
];

/// Parse time string to milliseconds from midnight
fn parse_time_to_ms(s: &str) -> Option<u32> {
    use chrono::NaiveTime;

    for fmt in TIME_FORMATS {
        if let Ok(time) = NaiveTime::parse_from_str(s, fmt) {
            let midnight = NaiveTime::from_hms_opt(0, 0, 0)?;
            let ms = time.signed_duration_since(midnight).num_milliseconds();
            if (0..=86_400_000).contains(&ms) {
                return Some(ms as u32);
            }
        }
    }
    None
}

/// Format milliseconds from midnight back to time string (HH:MM:SS format)
fn format_time_from_ms(ms: u32) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    let millis = ms % 1000;

    if millis > 0 {
        format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
    } else {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    }
}

/// Columnar Encoder
pub struct ColumnarEncoder {
    learner: TunedPatternLearner,
}

impl ColumnarEncoder {
    pub fn new() -> Self {
        Self {
            learner: TunedPatternLearner::new(),
        }
    }

    /// Encode text into columnar payload
    pub fn encode(&self, text: &str) -> ColumnarPayload {
        let (skeleton, matches) = self.learner.extract_skeleton(text);
        let mut payload = ColumnarPayload::new(skeleton);

        for m in matches {
            payload.add_match(m.pattern_type, &m.matched_text);
        }

        payload
    }

    /// Decode columnar payload back to text
    pub fn decode(&self, payload: &ColumnarPayload) -> String {
        payload.restore()
    }
}

impl Default for ColumnarEncoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipv4_encoding() {
        assert_eq!(parse_ipv4("192.168.1.100"), Some(0xC0A80164));
        assert_eq!(format_ipv4(0xC0A80164), "192.168.1.100");
    }

    #[test]
    fn test_log_level_encoding() {
        assert_eq!(LogLevel::parse_level("INFO") as u8, 2);
        assert_eq!(LogLevel::parse_level("ERROR") as u8, 4);
        assert_eq!(LogLevel::Info.to_str(), "INFO");
    }

    #[test]
    fn test_columnar_roundtrip() {
        let encoder = ColumnarEncoder::new();
        let text = "2024-01-15 10:30:45 INFO User logged in from 192.168.1.100";

        let payload = encoder.encode(text);
        let restored = encoder.decode(&payload);

        assert_eq!(text, restored);
    }

    #[test]
    fn test_columnar_multiline() {
        let encoder = ColumnarEncoder::new();
        let text = "2024-01-15 INFO Server started\n\
                    2024-01-16 WARN High load\n\
                    2024-01-17 ERROR Connection failed from 10.0.0.1";

        let payload = encoder.encode(text);
        let restored = encoder.decode(&payload);

        assert_eq!(text, restored);
    }

    #[test]
    fn test_payload_stats() {
        let encoder = ColumnarEncoder::new();
        let text = "2024-01-15 INFO 192.168.1.1 2024-01-16 ERROR 192.168.1.2";

        let payload = encoder.encode(text);
        let stats = payload.stats();

        // Dates are now stored as epoch days (date_days) or raw strings (dates_raw)
        assert!(stats["date_days"] + stats["dates_raw"] >= 2);
        assert!(stats["ipv4"] >= 2);
        assert!(stats["log_levels"] >= 2);
    }

    #[test]
    fn test_number_preservation() {
        let encoder = ColumnarEncoder::new();
        let text = "Count: 42 Value: 3.14159";

        let payload = encoder.encode(text);
        let restored = encoder.decode(&payload);

        assert!(restored.contains("42"));
        // Note: floating point formatting may differ slightly
    }

    #[test]
    fn test_email_extraction() {
        let encoder = ColumnarEncoder::new();
        let text = "Contact: admin@example.com and user@test.org";

        let payload = encoder.encode(text);

        assert_eq!(payload.emails.len(), 2);
        assert!(payload.emails.contains(&"admin@example.com".to_string()));
    }
}
