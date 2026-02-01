//! Tuned Compressor - Zstd + Bincode Implementation
//!
//! High-performance compression using:
//! - Bincode for fast binary serialization (vs JSON)
//! - Zstd for fast compression with dictionary support
//! - Columnar data layout for better compression ratios

use crate::columnar_encoder::{ColumnarEncoder, ColumnarPayload};
use crate::{ALICETextError, Result, ALICE_TEXT_MAGIC};
use serde::{Deserialize, Serialize};

/// Tuned compressor version
pub const TUNED_VERSION: (u8, u8) = (2, 0);

/// Compression mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum CompressionMode {
    /// Fast mode (zstd level 1-3)
    Fast = 0,
    /// Balanced mode (zstd level 5-10)
    Balanced = 1,
    /// Best compression (zstd level 15-22)
    Best = 2,
}

impl CompressionMode {
    fn zstd_level(self) -> i32 {
        match self {
            Self::Fast => 3,
            Self::Balanced => 10,
            Self::Best => 19,
        }
    }
}

impl Default for CompressionMode {
    fn default() -> Self {
        Self::Balanced
    }
}

/// Header for tuned compressed data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunedHeader {
    /// Original text length
    pub original_length: u64,
    /// Compression mode used
    pub mode: CompressionMode,
    /// Number of patterns extracted
    pub pattern_count: u32,
    /// Skeleton length
    pub skeleton_length: u32,
}

impl TunedHeader {
    /// Header size in bytes (fixed)
    pub const SIZE: usize = 24;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];

        bytes[0..8].copy_from_slice(&self.original_length.to_le_bytes());
        bytes[8] = self.mode as u8;
        bytes[9..12].fill(0); // reserved
        bytes[12..16].copy_from_slice(&self.pattern_count.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.skeleton_length.to_le_bytes());
        bytes[20..24].fill(0); // reserved

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
            mode: match bytes[8] {
                0 => CompressionMode::Fast,
                1 => CompressionMode::Balanced,
                2 => CompressionMode::Best,
                _ => CompressionMode::Balanced,
            },
            pattern_count: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            skeleton_length: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
        })
    }
}

/// Compression statistics
#[derive(Debug, Clone)]
pub struct TunedStats {
    pub original_size: usize,
    pub compressed_size: usize,
    pub skeleton_size: usize,
    pub pattern_count: usize,
    pub compression_ratio: f64,
    pub space_savings: f64,
}

/// Tuned Compressor
///
/// High-performance compressor using columnar layout + Zstd.
pub struct TunedCompressor {
    encoder: ColumnarEncoder,
    mode: CompressionMode,
    last_stats: Option<TunedStats>,
}

impl TunedCompressor {
    /// Create a new tuned compressor
    pub fn new(mode: CompressionMode) -> Self {
        Self {
            encoder: ColumnarEncoder::new(),
            mode,
            last_stats: None,
        }
    }

    /// Create with default balanced mode
    pub fn default_balanced() -> Self {
        Self::new(CompressionMode::Balanced)
    }

    /// Create with fast mode
    pub fn fast() -> Self {
        Self::new(CompressionMode::Fast)
    }

    /// Create with best compression
    pub fn best() -> Self {
        Self::new(CompressionMode::Best)
    }

    /// Compress text to bytes
    pub fn compress(&mut self, text: &str) -> Result<Vec<u8>> {
        let original_size = text.len();

        // Step 1: Extract patterns and create columnar payload
        let payload = self.encoder.encode(text);
        let pattern_count = payload.placeholder_map.len();
        let skeleton_size = payload.skeleton_tokens.len();

        // Step 2: Serialize payload with Bincode
        let serialized = bincode::serialize(&payload)
            .map_err(|e| ALICETextError::EncodingError(format!("Bincode error: {}", e)))?;

        // Step 3: Compress with Zstd
        let compressed = zstd::stream::encode_all(
            std::io::Cursor::new(&serialized),
            self.mode.zstd_level(),
        )
        .map_err(|e| ALICETextError::EncodingError(format!("Zstd error: {}", e)))?;

        // Step 4: Build final output
        // Format: MAGIC (8) + VERSION (2) + HEADER (24) + COMPRESSED_DATA
        let mut output = Vec::with_capacity(8 + 2 + TunedHeader::SIZE + compressed.len());

        // Magic bytes
        output.extend_from_slice(ALICE_TEXT_MAGIC);

        // Version (2.0 for tuned format)
        output.push(TUNED_VERSION.0);
        output.push(TUNED_VERSION.1);

        // Header
        let header = TunedHeader {
            original_length: original_size as u64,
            mode: self.mode,
            pattern_count: pattern_count as u32,
            skeleton_length: skeleton_size as u32,
        };
        output.extend_from_slice(&header.to_bytes());

        // Compressed data
        output.extend_from_slice(&compressed);

        let compressed_size = output.len();

        // Update stats
        self.last_stats = Some(TunedStats {
            original_size,
            compressed_size,
            skeleton_size,
            pattern_count,
            compression_ratio: compressed_size as f64 / original_size as f64,
            space_savings: 1.0 - (compressed_size as f64 / original_size as f64),
        });

        Ok(output)
    }

    /// Decompress bytes to text
    pub fn decompress(&self, data: &[u8]) -> Result<String> {
        // Minimum size check
        let min_size = 8 + 2 + TunedHeader::SIZE;
        if data.len() < min_size {
            return Err(ALICETextError::DecompressionError(
                "Data too short".to_string(),
            ));
        }

        // Verify magic
        if &data[0..8] != ALICE_TEXT_MAGIC {
            return Err(ALICETextError::InvalidMagic);
        }

        // Check version
        let version = (data[8], data[9]);
        if version.0 < 2 {
            return Err(ALICETextError::DecompressionError(
                "Legacy format - use ALICEText instead".to_string(),
            ));
        }

        // Parse header
        let _header = TunedHeader::from_bytes(&data[10..10 + TunedHeader::SIZE])?;

        // Get compressed data
        let compressed_data = &data[10 + TunedHeader::SIZE..];

        // Decompress with Zstd
        let decompressed = zstd::stream::decode_all(std::io::Cursor::new(compressed_data))
            .map_err(|e| ALICETextError::DecompressionError(format!("Zstd error: {}", e)))?;

        // Deserialize with Bincode
        let payload: ColumnarPayload = bincode::deserialize(&decompressed)
            .map_err(|e| ALICETextError::DecompressionError(format!("Bincode error: {}", e)))?;

        // Restore text
        Ok(self.encoder.decode(&payload))
    }

    /// Get last compression statistics
    pub fn last_stats(&self) -> Option<&TunedStats> {
        self.last_stats.as_ref()
    }

    /// Get compression mode
    pub fn mode(&self) -> CompressionMode {
        self.mode
    }

    /// Set compression mode
    pub fn set_mode(&mut self, mode: CompressionMode) {
        self.mode = mode;
    }

    /// Verify compressed data without full decompression
    pub fn verify(&self, data: &[u8]) -> Result<bool> {
        let min_size = 8 + 2 + TunedHeader::SIZE;
        if data.len() < min_size {
            return Ok(false);
        }

        if &data[0..8] != ALICE_TEXT_MAGIC {
            return Ok(false);
        }

        let version = (data[8], data[9]);
        if version.0 < 2 {
            return Ok(false);
        }

        TunedHeader::from_bytes(&data[10..10 + TunedHeader::SIZE])?;

        Ok(true)
    }

    /// Read header from compressed data
    pub fn read_header(&self, data: &[u8]) -> Result<TunedHeader> {
        let min_size = 8 + 2 + TunedHeader::SIZE;
        if data.len() < min_size {
            return Err(ALICETextError::DecompressionError(
                "Data too short".to_string(),
            ));
        }

        if &data[0..8] != ALICE_TEXT_MAGIC {
            return Err(ALICETextError::InvalidMagic);
        }

        TunedHeader::from_bytes(&data[10..10 + TunedHeader::SIZE])
    }
}

impl Default for TunedCompressor {
    fn default() -> Self {
        Self::new(CompressionMode::Balanced)
    }
}

/// Convenience function to compress with tuned compressor
pub fn compress_tuned(text: &str, mode: CompressionMode) -> Result<Vec<u8>> {
    let mut compressor = TunedCompressor::new(mode);
    compressor.compress(text)
}

/// Convenience function to decompress tuned format
pub fn decompress_tuned(data: &[u8]) -> Result<String> {
    let compressor = TunedCompressor::default();
    compressor.decompress(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tuned_roundtrip() {
        let mut compressor = TunedCompressor::default();
        let text = "2024-01-15 10:30:45 INFO User logged in from 192.168.1.100";

        let compressed = compressor.compress(text).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(text, decompressed);
    }

    #[test]
    fn test_tuned_multiline() {
        let mut compressor = TunedCompressor::default();
        let text = "2024-01-15 10:30:45 INFO Server started\n\
                    2024-01-15 10:30:46 INFO Listening on port 8080\n\
                    2024-01-15 10:31:00 WARN High memory usage";

        let compressed = compressor.compress(text).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(text, decompressed);
    }

    #[test]
    fn test_compression_ratio() {
        let mut compressor = TunedCompressor::new(CompressionMode::Best);

        // Generate repetitive log data
        let log_line = "2024-01-15 10:30:45 INFO User logged in from 192.168.1.100\n";
        let text = log_line.repeat(100);

        let compressed = compressor.compress(&text).unwrap();
        let stats = compressor.last_stats().unwrap();

        // Should achieve good compression for repetitive data
        assert!(stats.compression_ratio < 0.5, "Ratio: {}", stats.compression_ratio);
        assert!(stats.space_savings > 0.5, "Savings: {}", stats.space_savings);
    }

    #[test]
    fn test_fast_mode() {
        let mut compressor = TunedCompressor::fast();
        let text = "2024-01-15 INFO Test message";

        let compressed = compressor.compress(text).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(text, decompressed);
    }

    #[test]
    fn test_best_mode() {
        let mut compressor = TunedCompressor::best();
        let text = "2024-01-15 INFO Test message";

        let compressed = compressor.compress(text).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(text, decompressed);
    }

    #[test]
    fn test_header_parsing() {
        let mut compressor = TunedCompressor::default();
        let text = "2024-01-15 INFO Test 192.168.1.1";

        let compressed = compressor.compress(text).unwrap();
        let header = compressor.read_header(&compressed).unwrap();

        assert_eq!(header.original_length, text.len() as u64);
        assert!(header.pattern_count > 0);
    }

    #[test]
    fn test_verify() {
        let mut compressor = TunedCompressor::default();
        let text = "Test message";

        let compressed = compressor.compress(text).unwrap();
        assert!(compressor.verify(&compressed).unwrap());

        // Invalid data
        assert!(!compressor.verify(&[0u8; 50]).unwrap());
    }

    #[test]
    fn test_empty_text() {
        let mut compressor = TunedCompressor::default();
        let text = "";

        let compressed = compressor.compress(text).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(text, decompressed);
    }

    #[test]
    fn test_large_text() {
        let mut compressor = TunedCompressor::new(CompressionMode::Fast);

        // 1MB of log data
        let log_line = "2024-01-15 10:30:45 INFO User logged in from 192.168.1.100\n";
        let text = log_line.repeat(15000); // ~1MB

        let start = std::time::Instant::now();
        let compressed = compressor.compress(&text).unwrap();
        let compress_time = start.elapsed();

        let start = std::time::Instant::now();
        let decompressed = compressor.decompress(&compressed).unwrap();
        let decompress_time = start.elapsed();

        assert_eq!(text, decompressed);

        // Should be fast (< 500ms for 1MB)
        assert!(
            compress_time.as_millis() < 500,
            "Compress took {:?}",
            compress_time
        );
        assert!(
            decompress_time.as_millis() < 500,
            "Decompress took {:?}",
            decompress_time
        );

        let stats = compressor.last_stats().unwrap();
        println!(
            "1MB test: {:.1}% ratio, {:?} compress, {:?} decompress",
            stats.compression_ratio * 100.0,
            compress_time,
            decompress_time
        );
    }
}
