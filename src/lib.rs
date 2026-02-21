//! # ALICE-Text
//!
//! Exception-based text compression library.
//!
//! Send only surprises, not predictions.
//!
//! ALICE-Text is a revolutionary text compression system that uses predictive
//! coding to achieve high compression ratios for structured text like logs.
//! Instead of storing all data, it stores only the "exceptions" - tokens that
//! differ from predictions.
//!
//! ## Principle
//!
//! ```text
//! Input Text
//!     ↓
//! Prediction Model P(next|context)
//!     ↓
//! Prediction Success → Information = 0 → Don't send
//! Prediction Failure → Exception Token → Send
//! ```
//!
//! ## Example
//!
//! ```rust
//! use alice_text::{ALICEText, EncodingMode};
//!
//! let mut alice = ALICEText::new(EncodingMode::Pattern);
//!
//! // Compress
//! let text = "2024-01-15 INFO User logged in from 192.168.1.100";
//! let compressed = alice.compress(text).unwrap();
//!
//! // Decompress
//! let decompressed = alice.decompress(&compressed).unwrap();
//! assert_eq!(text, decompressed);
//! ```

// --- Global Allocator: mimalloc (Microsoft's high-performance allocator) ---
#[cfg(not(target_env = "msvc"))]
use mimalloc::MiMalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

pub mod arithmetic_coder;
pub mod entropy_estimator;
pub mod exception_decoder;
pub mod exception_encoder;
pub mod pattern_learner;

// Tuned (optimized) modules
pub mod tuned_pattern_learner;
pub mod columnar_encoder;
pub mod tuned_compressor;

// Format v3 and Query Engine
pub mod format_v3;
pub mod query_engine;

pub use arithmetic_coder::{ArithmeticDecoder, ArithmeticEncoder};
pub use entropy_estimator::{EntropyEstimate, EntropyEstimator};
pub use exception_decoder::ExceptionDecoder;
pub use exception_encoder::{EncodedText, EncodingMode, ExceptionEncoder, ExceptionHeader};
pub use pattern_learner::{LearnedPattern, PatternDatabase, PatternLearner, PatternMatch, PatternType};

// Tuned (optimized) exports
pub use tuned_pattern_learner::{TunedPatternLearner, TunedMatch, OwnedMatch, PatternType as TunedPatternType};
pub use columnar_encoder::{ColumnarEncoder, ColumnarPayload, LogLevel, TimestampColumn};
pub use tuned_compressor::{TunedCompressor, TunedHeader, TunedStats, CompressionMode, compress_tuned, decompress_tuned, TUNED_VERSION};

// Format v3 and Query Engine exports
pub use format_v3::{
    FormatV3Writer, FormatV3Header, FormatV3Metadata, ColumnType, ColumnEntry,
    CompressionLevel, PartialPayload, FORMAT_V3_VERSION,
};
pub use query_engine::{
    QueryEngine, QueryBuilder, QueryResult, QueryRow, Op, FileStats, ColumnStats,
    QuerySource, MmapSource, BufferSource,
    compress_v3, decompress_v3,
};

use std::io::{Read, Write};
use thiserror::Error;

/// ALICE-Text magic bytes
pub const ALICE_TEXT_MAGIC: &[u8; 8] = b"ALICETXT";

/// ALICE-Text version
pub const ALICE_TEXT_VERSION: (u8, u8) = (1, 0);

/// ALICE-Text fingerprint
pub const ALICE_TEXT_FINGERPRINT: &str = "ALICE-TXT-v1.0";

/// Error types for ALICE-Text operations
#[derive(Error, Debug)]
pub enum ALICETextError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid magic: expected ALICETXT")]
    InvalidMagic,

    #[error("Invalid version: {0}.{1}")]
    InvalidVersion(u8, u8),

    #[error("Decompression error: {0}")]
    DecompressionError(String),

    #[error("Encoding error: {0}")]
    EncodingError(String),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, ALICETextError>;

/// Compression statistics
#[derive(Debug, Clone)]
pub struct CompressionStats {
    pub original_size: usize,
    pub compressed_size: usize,
    pub token_count: usize,
    pub exception_count: usize,
    pub pattern_count: usize,
}

impl CompressionStats {
    /// Compression ratio (lower is better)
    pub fn compression_ratio(&self) -> f64 {
        if self.original_size == 0 {
            return 0.0;
        }
        self.compressed_size as f64 / self.original_size as f64
    }

    /// Exception rate (lower means more predictable)
    pub fn exception_rate(&self) -> f64 {
        if self.token_count == 0 {
            return 0.0;
        }
        self.exception_count as f64 / self.token_count as f64
    }

    /// Space savings percentage
    pub fn space_savings(&self) -> f64 {
        1.0 - self.compression_ratio()
    }
}

/// Main ALICE-Text compressor (v2: uses TunedCompressor with Zstd + Columnar encoding)
pub struct ALICEText {
    tuned: TunedCompressor,
    legacy_decoder: ExceptionDecoder,
    last_stats: Option<CompressionStats>,
}

impl ALICEText {
    /// Create a new ALICE-Text instance
    pub fn new(_mode: EncodingMode) -> Self {
        Self {
            tuned: TunedCompressor::default_balanced(),
            legacy_decoder: ExceptionDecoder::new(),
            last_stats: None,
        }
    }



    /// Compress text to bytes (uses TunedCompressor v2)
    pub fn compress(&mut self, text: &str) -> Result<Vec<u8>> {
        let compressed = self.tuned.compress(text)?;

        // Update stats from TunedCompressor
        if let Some(stats) = self.tuned.last_stats() {
            self.last_stats = Some(CompressionStats {
                original_size: stats.original_size,
                compressed_size: stats.compressed_size,
                token_count: stats.pattern_count,
                exception_count: 0, // Not tracked in tuned mode
                pattern_count: stats.pattern_count,
            });
        }

        Ok(compressed)
    }

    /// Decompress bytes to text (auto-detects v1 or v2 format)
    pub fn decompress(&self, data: &[u8]) -> Result<String> {
        // Check version byte to determine format
        if data.len() >= 10 && data[8] >= 2 {
            // v2 format (TunedCompressor)
            self.tuned.decompress(data)
        } else {
            // v1 format (legacy LZMA)
            self.legacy_decoder.decode_from_bytes(data)
        }
    }

    /// Compress text and write to writer
    pub fn compress_to<W: Write>(&mut self, text: &str, writer: &mut W) -> Result<CompressionStats> {
        let compressed = self.compress(text)?;
        writer.write_all(&compressed)?;
        Ok(self.last_stats.clone().unwrap())
    }

    /// Decompress from reader
    pub fn decompress_from<R: Read>(&self, reader: &mut R) -> Result<String> {
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        self.decompress(&data)
    }

    /// Get last compression statistics
    pub fn last_stats(&self) -> Option<&CompressionStats> {
        self.last_stats.as_ref()
    }

    /// Estimate compression for text
    pub fn estimate_compression(&self, text: &str) -> EntropyEstimate {
        let estimator = EntropyEstimator::new();
        estimator.estimate(text)
    }
}

impl Default for ALICEText {
    fn default() -> Self {
        Self::new(EncodingMode::Pattern)
    }
}

/// Convenience function to compress text
pub fn compress(text: &str, mode: EncodingMode) -> Result<Vec<u8>> {
    let mut alice = ALICEText::new(mode);
    alice.compress(text)
}

/// Convenience function to decompress bytes
pub fn decompress(data: &[u8]) -> Result<String> {
    let alice = ALICEText::default();
    alice.decompress(data)
}

#[cfg(feature = "ml")]
pub mod ml_bridge;

#[cfg(feature = "voice")]
pub mod voice_bridge;

#[cfg(feature = "search")]
pub mod search_bridge;

#[cfg(feature = "python")]
mod python_bindings {
    use super::*;
    use pyo3::prelude::*;
    use pyo3::types::PyModule;

    #[pyclass]
    struct PyALICEText {
        inner: ALICEText,
    }

    #[pymethods]
    impl PyALICEText {
        #[new]
        #[pyo3(signature = (mode = "pattern"))]
        fn new(mode: &str) -> PyResult<Self> {
            let mode = match mode {
                "pattern" => EncodingMode::Pattern,
                "ngram" => EncodingMode::NGram,
                _ => EncodingMode::Pattern,
            };
            Ok(Self {
                inner: ALICEText::new(mode),
            })
        }

        fn compress(&mut self, py: Python<'_>, text: &str) -> PyResult<Vec<u8>> {
            let text_owned = text.to_owned();
            let inner = &mut self.inner;
            py.allow_threads(|| {
                inner.compress(&text_owned)
            }).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
        }

        fn decompress(&self, py: Python<'_>, data: &[u8]) -> PyResult<String> {
            let data_owned = data.to_vec();
            let inner = &self.inner;
            py.allow_threads(|| {
                inner.decompress(&data_owned)
            }).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
        }
    }

    #[pymodule]
    fn alice_text(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_class::<PyALICEText>()?;
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress_roundtrip() {
        let mut alice = ALICEText::new(EncodingMode::Pattern);
        let text = "Hello, World!";

        let compressed = alice.compress(text).unwrap();
        let decompressed = alice.decompress(&compressed).unwrap();

        assert_eq!(text, decompressed);
    }

    #[test]
    fn test_log_compression() {
        let mut alice = ALICEText::new(EncodingMode::Pattern);
        let log = "2024-01-15 10:30:45 INFO User john logged in from 192.168.1.100";

        let compressed = alice.compress(log).unwrap();
        let decompressed = alice.decompress(&compressed).unwrap();

        assert_eq!(log, decompressed);
    }

    #[test]
    fn test_multiline_log() {
        let mut alice = ALICEText::new(EncodingMode::Pattern);
        let logs = "2024-01-15 10:30:45 INFO Server started\n\
                    2024-01-15 10:30:46 INFO Listening on port 8080\n\
                    2024-01-15 10:31:00 WARN High memory usage";

        let compressed = alice.compress(logs).unwrap();
        let decompressed = alice.decompress(&compressed).unwrap();

        assert_eq!(logs, decompressed);
    }

    #[test]
    fn test_compression_stats() {
        let mut alice = ALICEText::new(EncodingMode::Pattern);
        let text = "Test message ".repeat(100);

        alice.compress(&text).unwrap();
        let stats = alice.last_stats().unwrap();

        assert!(stats.original_size > 0);
        assert!(stats.compressed_size > 0);
    }

    #[test]
    fn test_empty_text() {
        let mut alice = ALICEText::new(EncodingMode::Pattern);

        let compressed = alice.compress("").unwrap();
        let decompressed = alice.decompress(&compressed).unwrap();

        assert_eq!("", decompressed);
    }
}
