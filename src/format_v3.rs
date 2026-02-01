//! ALICE-Text Format v3 - Columnar Storage with Selective Decompression
//!
//! This format enables:
//! - Header-only reads for metadata
//! - Per-column compression (seek + read for partial decompression)
//! - Index-based pinpoint access
//!
//! ## File Layout
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │ Magic: "ALICETXT" (8 bytes)             │
//! ├─────────────────────────────────────────┤
//! │ Version: 3.0 (2 bytes)                  │
//! ├─────────────────────────────────────────┤
//! │ Header (32 bytes)                       │
//! ├─────────────────────────────────────────┤
//! │ Column Directory (variable)             │
//! ├─────────────────────────────────────────┤
//! │ Column 0 Data (Zstd)                    │
//! ├─────────────────────────────────────────┤
//! │ Column 1 Data (Zstd)                    │
//! ├─────────────────────────────────────────┤
//! │ ...                                     │
//! ├─────────────────────────────────────────┤
//! │ Skeleton Data (Zstd)                    │
//! └─────────────────────────────────────────┘
//! ```

use crate::columnar_encoder::{ColumnarEncoder, ColumnarPayload, TimestampColumn};
use crate::{ALICETextError, Result, ALICE_TEXT_MAGIC};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Seek, SeekFrom};

/// Format v3 version
pub const FORMAT_V3_VERSION: (u8, u8) = (3, 0);

/// Column types for directory
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ColumnType {
    Skeleton = 0,
    Timestamps = 1,
    IPv4 = 2,
    IPv6 = 3,
    LogLevels = 4,
    Numbers = 5,
    UUIDs = 6,
    Emails = 7,
    URLs = 8,
    Paths = 9,
    DateDays = 10,
    DatesRaw = 11,
    TimeMs = 12,
    TimesRaw = 13,
    HexValues = 14,
    Others = 15,
    PlaceholderMap = 16,
    TimestampsRaw = 17,
}

impl ColumnType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Skeleton),
            1 => Some(Self::Timestamps),
            2 => Some(Self::IPv4),
            3 => Some(Self::IPv6),
            4 => Some(Self::LogLevels),
            5 => Some(Self::Numbers),
            6 => Some(Self::UUIDs),
            7 => Some(Self::Emails),
            8 => Some(Self::URLs),
            9 => Some(Self::Paths),
            10 => Some(Self::DateDays),
            11 => Some(Self::DatesRaw),
            12 => Some(Self::TimeMs),
            13 => Some(Self::TimesRaw),
            14 => Some(Self::HexValues),
            15 => Some(Self::Others),
            16 => Some(Self::PlaceholderMap),
            17 => Some(Self::TimestampsRaw),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Skeleton => "skeleton",
            Self::Timestamps => "timestamps",
            Self::IPv4 => "ipv4",
            Self::IPv6 => "ipv6",
            Self::LogLevels => "log_levels",
            Self::Numbers => "numbers",
            Self::UUIDs => "uuids",
            Self::Emails => "emails",
            Self::URLs => "urls",
            Self::Paths => "paths",
            Self::DateDays => "date_days",
            Self::DatesRaw => "dates_raw",
            Self::TimeMs => "time_ms",
            Self::TimesRaw => "times_raw",
            Self::HexValues => "hex_values",
            Self::Others => "others",
            Self::PlaceholderMap => "placeholder_map",
            Self::TimestampsRaw => "timestamps_raw",
        }
    }
}

/// Column directory entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnEntry {
    /// Column type
    pub col_type: ColumnType,
    /// Offset from start of file
    pub offset: u64,
    /// Compressed size in bytes
    pub compressed_size: u32,
    /// Uncompressed size in bytes
    pub uncompressed_size: u32,
    /// Number of rows/items
    pub row_count: u32,
}

impl ColumnEntry {
    /// Entry size in bytes (1 + 8 + 4 + 4 + 4 = 21 bytes)
    pub const SIZE: usize = 21;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0] = self.col_type as u8;
        bytes[1..9].copy_from_slice(&self.offset.to_le_bytes());
        bytes[9..13].copy_from_slice(&self.compressed_size.to_le_bytes());
        bytes[13..17].copy_from_slice(&self.uncompressed_size.to_le_bytes());
        bytes[17..21].copy_from_slice(&self.row_count.to_le_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < Self::SIZE {
            return Err(ALICETextError::DecompressionError(
                "Column entry too short".to_string(),
            ));
        }
        Ok(Self {
            col_type: ColumnType::from_u8(bytes[0])
                .ok_or_else(|| ALICETextError::DecompressionError("Invalid column type".to_string()))?,
            offset: u64::from_le_bytes(bytes[1..9].try_into().unwrap()),
            compressed_size: u32::from_le_bytes(bytes[9..13].try_into().unwrap()),
            uncompressed_size: u32::from_le_bytes(bytes[13..17].try_into().unwrap()),
            row_count: u32::from_le_bytes(bytes[17..21].try_into().unwrap()),
        })
    }
}

/// Format v3 header
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatV3Header {
    /// Original text length
    pub original_length: u64,
    /// Compression level (0=fast, 1=balanced, 2=best)
    pub compression_level: u8,
    /// Number of columns in directory
    pub column_count: u16,
    /// Total row count (for log lines)
    pub row_count: u64,
    /// Reserved for future use
    pub reserved: [u8; 13],
}

impl FormatV3Header {
    /// Header size: 8 + 1 + 2 + 8 + 13 = 32 bytes
    pub const SIZE: usize = 32;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..8].copy_from_slice(&self.original_length.to_le_bytes());
        bytes[8] = self.compression_level;
        bytes[9..11].copy_from_slice(&self.column_count.to_le_bytes());
        bytes[11..19].copy_from_slice(&self.row_count.to_le_bytes());
        bytes[19..32].copy_from_slice(&self.reserved);
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < Self::SIZE {
            return Err(ALICETextError::DecompressionError(
                "Header too short".to_string(),
            ));
        }
        Ok(Self {
            original_length: u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
            compression_level: bytes[8],
            column_count: u16::from_le_bytes(bytes[9..11].try_into().unwrap()),
            row_count: u64::from_le_bytes(bytes[11..19].try_into().unwrap()),
            reserved: bytes[19..32].try_into().unwrap(),
        })
    }
}

/// Format v3 file metadata (header + column directory)
#[derive(Debug, Clone)]
pub struct FormatV3Metadata {
    pub header: FormatV3Header,
    pub columns: Vec<ColumnEntry>,
}

impl FormatV3Metadata {
    /// Read metadata from file (header only read - no data decompression)
    pub fn read_from<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        // Read magic
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        if &magic != ALICE_TEXT_MAGIC {
            return Err(ALICETextError::InvalidMagic);
        }

        // Read version
        let mut version = [0u8; 2];
        reader.read_exact(&mut version)?;
        if version[0] != 3 {
            return Err(ALICETextError::InvalidVersion(version[0], version[1]));
        }

        // Read header
        let mut header_bytes = [0u8; FormatV3Header::SIZE];
        reader.read_exact(&mut header_bytes)?;
        let header = FormatV3Header::from_bytes(&header_bytes)?;

        // Read column directory
        let mut columns = Vec::with_capacity(header.column_count as usize);
        for _ in 0..header.column_count {
            let mut entry_bytes = [0u8; ColumnEntry::SIZE];
            reader.read_exact(&mut entry_bytes)?;
            columns.push(ColumnEntry::from_bytes(&entry_bytes)?);
        }

        Ok(Self { header, columns })
    }

    /// Get column entry by type
    pub fn get_column(&self, col_type: ColumnType) -> Option<&ColumnEntry> {
        self.columns.iter().find(|c| c.col_type == col_type)
    }

    /// Get all column names
    pub fn column_names(&self) -> Vec<&'static str> {
        self.columns.iter().map(|c| c.col_type.name()).collect()
    }

    /// Get total compressed size
    pub fn compressed_size(&self) -> u64 {
        self.columns.iter().map(|c| c.compressed_size as u64).sum()
    }
}

/// Compression level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CompressionLevel {
    Fast = 0,
    Balanced = 1,
    Best = 2,
}

impl CompressionLevel {
    fn zstd_level(self) -> i32 {
        match self {
            Self::Fast => 3,
            Self::Balanced => 10,
            Self::Best => 19,
        }
    }
}

impl Default for CompressionLevel {
    fn default() -> Self {
        Self::Balanced
    }
}

/// Format v3 writer
pub struct FormatV3Writer {
    encoder: ColumnarEncoder,
    level: CompressionLevel,
}

impl FormatV3Writer {
    pub fn new(level: CompressionLevel) -> Self {
        Self {
            encoder: ColumnarEncoder::new(),
            level,
        }
    }

    /// Compress text to v3 format
    pub fn compress(&self, text: &str) -> Result<Vec<u8>> {
        let original_length = text.len() as u64;
        let payload = self.encoder.encode(text);

        // Count rows (log lines)
        let row_count = text.lines().count() as u64;

        // Prepare columns for individual compression
        let mut column_data: Vec<(ColumnType, Vec<u8>, u32)> = Vec::new();

        // Helper to compress and add column
        let zstd_level = self.level.zstd_level();
        let mut add_column = |col_type: ColumnType, data: &[u8], count: u32| -> Result<()> {
            if !data.is_empty() || col_type == ColumnType::Skeleton || col_type == ColumnType::PlaceholderMap {
                let compressed = zstd::stream::encode_all(Cursor::new(data), zstd_level)
                    .map_err(|e| ALICETextError::EncodingError(format!("Zstd error: {}", e)))?;
                column_data.push((col_type, compressed, count));
            }
            Ok(())
        };

        // Serialize each column separately
        // 1. Skeleton tokens
        let skeleton_bytes = bincode::serialize(&payload.skeleton_tokens)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::Skeleton, &skeleton_bytes, payload.skeleton_tokens.len() as u32)?;

        // 2. Placeholder map
        let placeholder_bytes = bincode::serialize(&payload.placeholder_map)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::PlaceholderMap, &placeholder_bytes, payload.placeholder_map.len() as u32)?;

        // 3. Timestamps (delta-encoded)
        let ts_bytes = bincode::serialize(&payload.timestamps)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::Timestamps, &ts_bytes, payload.timestamps.deltas.len() as u32)?;

        // 4. IPv4
        let ipv4_bytes = bincode::serialize(&payload.ipv4_addrs)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::IPv4, &ipv4_bytes, payload.ipv4_addrs.len() as u32)?;

        // 5. IPv6
        let ipv6_bytes = bincode::serialize(&payload.ipv6_addrs)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::IPv6, &ipv6_bytes, payload.ipv6_addrs.len() as u32)?;

        // 6. Log levels
        let log_bytes = bincode::serialize(&payload.log_levels)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::LogLevels, &log_bytes, payload.log_levels.len() as u32)?;

        // 7. Numbers
        let num_bytes = bincode::serialize(&payload.numbers)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::Numbers, &num_bytes, payload.numbers.len() as u32)?;

        // 8. UUIDs
        let uuid_bytes = bincode::serialize(&payload.uuids)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::UUIDs, &uuid_bytes, payload.uuids.len() as u32)?;

        // 9. Emails
        let email_bytes = bincode::serialize(&payload.emails)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::Emails, &email_bytes, payload.emails.len() as u32)?;

        // 10. URLs
        let url_bytes = bincode::serialize(&payload.urls)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::URLs, &url_bytes, payload.urls.len() as u32)?;

        // 11. Paths
        let path_bytes = bincode::serialize(&payload.paths)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::Paths, &path_bytes, payload.paths.len() as u32)?;

        // 12. Date days
        let date_days_bytes = bincode::serialize(&payload.date_days)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::DateDays, &date_days_bytes, payload.date_days.len() as u32)?;

        // 13. Dates raw
        let dates_raw_bytes = bincode::serialize(&payload.dates)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::DatesRaw, &dates_raw_bytes, payload.dates.len() as u32)?;

        // 14. Time ms
        let time_ms_bytes = bincode::serialize(&payload.time_ms)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::TimeMs, &time_ms_bytes, payload.time_ms.len() as u32)?;

        // 15. Times raw
        let times_raw_bytes = bincode::serialize(&payload.times)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::TimesRaw, &times_raw_bytes, payload.times.len() as u32)?;

        // 16. Hex values
        let hex_bytes = bincode::serialize(&payload.hex_values)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::HexValues, &hex_bytes, payload.hex_values.len() as u32)?;

        // 17. Others
        let others_bytes = bincode::serialize(&payload.others)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::Others, &others_bytes, payload.others.len() as u32)?;

        // 18. Timestamps raw
        let ts_raw_bytes = bincode::serialize(&payload.timestamps.raw)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;
        add_column(ColumnType::TimestampsRaw, &ts_raw_bytes, payload.timestamps.raw.len() as u32)?;

        // Calculate offsets
        let header_start = 8 + 2; // Magic + Version
        let directory_start = header_start + FormatV3Header::SIZE;
        let data_start = directory_start + column_data.len() * ColumnEntry::SIZE;

        let mut current_offset = data_start as u64;
        let mut entries: Vec<ColumnEntry> = Vec::new();

        for (col_type, compressed, count) in &column_data {
            entries.push(ColumnEntry {
                col_type: *col_type,
                offset: current_offset,
                compressed_size: compressed.len() as u32,
                uncompressed_size: 0, // We don't track this for simplicity
                row_count: *count,
            });
            current_offset += compressed.len() as u64;
        }

        // Build output
        let total_size = current_offset as usize;
        let mut output = Vec::with_capacity(total_size);

        // Write magic
        output.extend_from_slice(ALICE_TEXT_MAGIC);

        // Write version
        output.push(FORMAT_V3_VERSION.0);
        output.push(FORMAT_V3_VERSION.1);

        // Write header
        let header = FormatV3Header {
            original_length,
            compression_level: self.level as u8,
            column_count: entries.len() as u16,
            row_count,
            reserved: [0u8; 13],
        };
        output.extend_from_slice(&header.to_bytes());

        // Write column directory
        for entry in &entries {
            output.extend_from_slice(&entry.to_bytes());
        }

        // Write column data
        for (_, compressed, _) in column_data {
            output.extend_from_slice(&compressed);
        }

        Ok(output)
    }

    /// Decompress v3 format to text (full decompression)
    pub fn decompress(data: &[u8]) -> Result<String> {
        let mut cursor = Cursor::new(data);
        let metadata = FormatV3Metadata::read_from(&mut cursor)?;

        // Read all columns and reconstruct payload
        let payload = Self::read_all_columns(&mut cursor, &metadata)?;

        Ok(payload.restore())
    }

    /// Read specific columns only (selective decompression)
    pub fn read_columns<R: Read + Seek>(
        reader: &mut R,
        metadata: &FormatV3Metadata,
        column_types: &[ColumnType],
    ) -> Result<PartialPayload> {
        let mut partial = PartialPayload::default();

        for col_type in column_types {
            if let Some(entry) = metadata.get_column(*col_type) {
                reader.seek(SeekFrom::Start(entry.offset))?;
                let mut compressed = vec![0u8; entry.compressed_size as usize];
                reader.read_exact(&mut compressed)?;

                let decompressed = zstd::stream::decode_all(Cursor::new(&compressed))
                    .map_err(|e| ALICETextError::DecompressionError(format!("Zstd error: {}", e)))?;

                match col_type {
                    ColumnType::LogLevels => {
                        partial.log_levels = Some(bincode::deserialize(&decompressed)
                            .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?);
                    }
                    ColumnType::Timestamps => {
                        partial.timestamps = Some(bincode::deserialize(&decompressed)
                            .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?);
                    }
                    ColumnType::IPv4 => {
                        partial.ipv4_addrs = Some(bincode::deserialize(&decompressed)
                            .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?);
                    }
                    ColumnType::IPv6 => {
                        partial.ipv6_addrs = Some(bincode::deserialize(&decompressed)
                            .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?);
                    }
                    ColumnType::Numbers => {
                        partial.numbers = Some(bincode::deserialize(&decompressed)
                            .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?);
                    }
                    ColumnType::UUIDs => {
                        partial.uuids = Some(bincode::deserialize(&decompressed)
                            .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?);
                    }
                    ColumnType::Emails => {
                        partial.emails = Some(bincode::deserialize(&decompressed)
                            .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?);
                    }
                    ColumnType::URLs => {
                        partial.urls = Some(bincode::deserialize(&decompressed)
                            .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?);
                    }
                    ColumnType::Paths => {
                        partial.paths = Some(bincode::deserialize(&decompressed)
                            .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?);
                    }
                    _ => {}
                }
            }
        }

        Ok(partial)
    }

    /// Read all columns and reconstruct full payload
    fn read_all_columns<R: Read + Seek>(
        reader: &mut R,
        metadata: &FormatV3Metadata,
    ) -> Result<ColumnarPayload> {
        let mut skeleton_tokens = Vec::new();
        let mut placeholder_map = Vec::new();
        let mut timestamps = TimestampColumn::default();
        let mut ipv4_addrs = Vec::new();
        let mut ipv6_addrs = Vec::new();
        let mut log_levels = Vec::new();
        let mut numbers = Vec::new();
        let mut uuids = Vec::new();
        let mut emails = Vec::new();
        let mut urls = Vec::new();
        let mut paths = Vec::new();
        let mut date_days = Vec::new();
        let mut dates = Vec::new();
        let mut time_ms = Vec::new();
        let mut times = Vec::new();
        let mut hex_values = Vec::new();
        let mut others = Vec::new();
        let mut timestamps_raw = Vec::new();

        for entry in &metadata.columns {
            reader.seek(SeekFrom::Start(entry.offset))?;
            let mut compressed = vec![0u8; entry.compressed_size as usize];
            reader.read_exact(&mut compressed)?;

            let decompressed = zstd::stream::decode_all(Cursor::new(&compressed))
                .map_err(|e| ALICETextError::DecompressionError(format!("Zstd error: {}", e)))?;

            match entry.col_type {
                ColumnType::Skeleton => {
                    skeleton_tokens = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::PlaceholderMap => {
                    placeholder_map = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::Timestamps => {
                    timestamps = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::TimestampsRaw => {
                    timestamps_raw = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::IPv4 => {
                    ipv4_addrs = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::IPv6 => {
                    ipv6_addrs = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::LogLevels => {
                    log_levels = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::Numbers => {
                    numbers = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::UUIDs => {
                    uuids = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::Emails => {
                    emails = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::URLs => {
                    urls = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::Paths => {
                    paths = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::DateDays => {
                    date_days = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::DatesRaw => {
                    dates = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::TimeMs => {
                    time_ms = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::TimesRaw => {
                    times = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::HexValues => {
                    hex_values = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
                ColumnType::Others => {
                    others = bincode::deserialize(&decompressed)
                        .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;
                }
            }
        }

        // Merge timestamps raw
        timestamps.raw = timestamps_raw;

        Ok(ColumnarPayload {
            skeleton_tokens,
            placeholder_map,
            timestamps,
            ipv4_addrs,
            ipv6_addrs,
            log_levels,
            numbers,
            uuids,
            emails,
            urls,
            paths,
            date_days,
            dates,
            time_ms,
            times,
            hex_values,
            others,
        })
    }
}

impl Default for FormatV3Writer {
    fn default() -> Self {
        Self::new(CompressionLevel::Balanced)
    }
}

/// Partial payload for selective column reads
#[derive(Debug, Default)]
pub struct PartialPayload {
    pub timestamps: Option<TimestampColumn>,
    pub ipv4_addrs: Option<Vec<u32>>,
    pub ipv6_addrs: Option<Vec<u128>>,
    pub log_levels: Option<Vec<u8>>,
    pub numbers: Option<Vec<f64>>,
    pub uuids: Option<Vec<u128>>,
    pub emails: Option<Vec<String>>,
    pub urls: Option<Vec<String>>,
    pub paths: Option<Vec<String>>,
}

impl PartialPayload {
    /// Get log level values as strings
    pub fn log_level_strings(&self) -> Option<Vec<String>> {
        self.log_levels.as_ref().map(|levels| {
            levels.iter().map(|&l| {
                match l {
                    0 => "TRACE",
                    1 => "DEBUG",
                    2 => "INFO",
                    3 => "WARN",
                    4 => "ERROR",
                    5 => "FATAL",
                    6 => "CRITICAL",
                    _ => "UNKNOWN",
                }.to_string()
            }).collect()
        })
    }

    /// Get IPv4 addresses as strings
    pub fn ipv4_strings(&self) -> Option<Vec<String>> {
        self.ipv4_addrs.as_ref().map(|addrs| {
            addrs.iter().map(|&ip| {
                std::net::Ipv4Addr::from(ip).to_string()
            }).collect()
        })
    }

    /// Get timestamps as strings
    pub fn timestamp_strings(&self) -> Option<Vec<String>> {
        self.timestamps.as_ref().map(|ts| {
            let prefix_sums = ts.prepare_for_read();
            (0..ts.deltas.len())
                .filter_map(|i| ts.get_delta(i, &prefix_sums))
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_v3_roundtrip() {
        let writer = FormatV3Writer::new(CompressionLevel::Fast);
        let text = "2024-01-15 10:30:45 INFO User logged in from 192.168.1.100";

        let compressed = writer.compress(text).unwrap();
        let decompressed = FormatV3Writer::decompress(&compressed).unwrap();

        assert_eq!(text, decompressed);
    }

    #[test]
    fn test_format_v3_multiline() {
        let writer = FormatV3Writer::new(CompressionLevel::Balanced);
        let text = "2024-01-15 10:30:45 INFO Server started\n\
                    2024-01-15 10:30:46 WARN High memory\n\
                    2024-01-15 10:30:47 ERROR Connection failed";

        let compressed = writer.compress(text).unwrap();
        let decompressed = FormatV3Writer::decompress(&compressed).unwrap();

        assert_eq!(text, decompressed);
    }

    #[test]
    fn test_metadata_read() {
        let writer = FormatV3Writer::new(CompressionLevel::Fast);
        let text = "2024-01-15 INFO 192.168.1.1 ERROR 192.168.1.2";

        let compressed = writer.compress(text).unwrap();
        let mut cursor = Cursor::new(&compressed);
        let metadata = FormatV3Metadata::read_from(&mut cursor).unwrap();

        assert_eq!(metadata.header.original_length, text.len() as u64);
        assert!(metadata.columns.len() > 0);

        // Check column names
        let names = metadata.column_names();
        assert!(names.contains(&"log_levels"));
        assert!(names.contains(&"ipv4"));
    }

    #[test]
    fn test_selective_column_read() {
        let writer = FormatV3Writer::new(CompressionLevel::Fast);
        let text = "2024-01-15 INFO 192.168.1.1\n\
                    2024-01-15 ERROR 192.168.1.2\n\
                    2024-01-15 WARN 192.168.1.3";

        let compressed = writer.compress(text).unwrap();
        let mut cursor = Cursor::new(&compressed);
        let metadata = FormatV3Metadata::read_from(&mut cursor).unwrap();

        // Read only log_levels column
        let partial = FormatV3Writer::read_columns(
            &mut cursor,
            &metadata,
            &[ColumnType::LogLevels],
        ).unwrap();

        let levels = partial.log_level_strings().unwrap();
        assert_eq!(levels.len(), 3);
        assert!(levels.contains(&"INFO".to_string()));
        assert!(levels.contains(&"ERROR".to_string()));
        assert!(levels.contains(&"WARN".to_string()));
    }
}
