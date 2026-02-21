//! Exception encoder module for ALICE-Text
//!
//! Encodes text by storing only the "exceptions" - tokens that differ from predictions.

use crate::pattern_learner::{PatternDatabase, PatternLearner, PatternMatch};
use crate::{ALICETextError, Result, ALICE_TEXT_MAGIC, ALICE_TEXT_VERSION};
use lzma_rs::lzma_compress;
use serde::{Deserialize, Serialize};

/// Encoding mode for compression
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EncodingMode {
    /// Pattern-based encoding (lightweight, good for structured logs)
    #[default]
    Pattern,
    /// N-gram based encoding (better compression, more CPU)
    NGram,
}

/// Header for encoded data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExceptionHeader {
    /// Encoding mode used
    pub mode: EncodingMode,
    /// Original text length
    pub original_length: u32,
    /// Number of tokens
    pub token_count: u32,
    /// Number of exceptions
    pub exception_count: u32,
    /// Pattern database length (if pattern mode)
    pub pattern_db_length: u32,
    /// Compressed data length
    pub compressed_length: u32,
}

impl ExceptionHeader {
    /// Header size in bytes
    pub const SIZE: usize = 32;

    /// Create a new header
    pub fn new(mode: EncodingMode) -> Self {
        Self {
            mode,
            original_length: 0,
            token_count: 0,
            exception_count: 0,
            pattern_db_length: 0,
            compressed_length: 0,
        }
    }

    /// Serialize header to bytes
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];

        // Mode (1 byte)
        bytes[0] = match self.mode {
            EncodingMode::Pattern => 0,
            EncodingMode::NGram => 1,
        };

        // Reserved (3 bytes)
        bytes[1..4].copy_from_slice(&[0, 0, 0]);

        // Original length (4 bytes, little-endian)
        bytes[4..8].copy_from_slice(&self.original_length.to_le_bytes());

        // Token count (4 bytes)
        bytes[8..12].copy_from_slice(&self.token_count.to_le_bytes());

        // Exception count (4 bytes)
        bytes[12..16].copy_from_slice(&self.exception_count.to_le_bytes());

        // Pattern DB length (4 bytes)
        bytes[16..20].copy_from_slice(&self.pattern_db_length.to_le_bytes());

        // Compressed length (4 bytes)
        bytes[20..24].copy_from_slice(&self.compressed_length.to_le_bytes());

        // Reserved (8 bytes)
        bytes[24..32].copy_from_slice(&[0; 8]);

        bytes
    }

    /// Deserialize header from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < Self::SIZE {
            return Err(ALICETextError::DecompressionError(
                "Header too short".to_string(),
            ));
        }

        let mode = match bytes[0] {
            0 => EncodingMode::Pattern,
            1 => EncodingMode::NGram,
            _ => {
                return Err(ALICETextError::DecompressionError(
                    "Invalid mode".to_string(),
                ))
            }
        };

        let to_err = || ALICETextError::DecompressionError("Header slice error".to_string());
        Ok(Self {
            mode,
            original_length: u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| to_err())?),
            token_count: u32::from_le_bytes(bytes[8..12].try_into().map_err(|_| to_err())?),
            exception_count: u32::from_le_bytes(bytes[12..16].try_into().map_err(|_| to_err())?),
            pattern_db_length: u32::from_le_bytes(bytes[16..20].try_into().map_err(|_| to_err())?),
            compressed_length: u32::from_le_bytes(bytes[20..24].try_into().map_err(|_| to_err())?),
        })
    }
}

/// Encoded text result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodedText {
    /// Header information
    pub header: ExceptionHeader,
    /// Pattern database (if pattern mode)
    pub pattern_db: Option<PatternDatabase>,
    /// Pattern matches for restoration
    pub pattern_matches: Vec<PatternMatch>,
    /// Processed text with patterns replaced
    pub processed_text: String,
    /// Original text (for direct compression mode)
    pub original_text: String,
    /// Token count
    pub token_count: usize,
    /// Exception count
    pub exception_count: usize,
}

/// Exception encoder
pub struct ExceptionEncoder {
    /// Encoding mode
    mode: EncodingMode,
    /// Pattern learner
    pattern_learner: PatternLearner,
}

impl ExceptionEncoder {
    /// Create a new encoder
    pub fn new(mode: EncodingMode) -> Self {
        Self {
            mode,
            pattern_learner: PatternLearner::new(),
        }
    }

    /// Encode text to EncodedText structure
    pub fn encode(&self, text: &str) -> Result<EncodedText> {
        match self.mode {
            EncodingMode::Pattern => self.encode_pattern(text),
            EncodingMode::NGram => self.encode_ngram(text),
        }
    }

    /// Pattern-based encoding
    fn encode_pattern(&self, text: &str) -> Result<EncodedText> {
        // Learn patterns
        let pattern_db = self.pattern_learner.learn(text);

        // Replace patterns with placeholders
        let (processed_text, pattern_matches) = self.pattern_learner.replace_patterns(text);

        // Count tokens (simple whitespace tokenization for stats)
        let token_count = text.split_whitespace().count();
        let exception_count = pattern_matches.len();

        let mut header = ExceptionHeader::new(self.mode);
        header.original_length = text.len() as u32;
        header.token_count = token_count as u32;
        header.exception_count = exception_count as u32;

        Ok(EncodedText {
            header,
            pattern_db: Some(pattern_db),
            pattern_matches,
            processed_text,
            original_text: text.to_string(),
            token_count,
            exception_count,
        })
    }

    /// N-gram based encoding (simplified - stores compressed text)
    fn encode_ngram(&self, text: &str) -> Result<EncodedText> {
        let token_count = text.split_whitespace().count();

        let mut header = ExceptionHeader::new(self.mode);
        header.original_length = text.len() as u32;
        header.token_count = token_count as u32;
        header.exception_count = token_count as u32; // All tokens are exceptions in simplified mode

        Ok(EncodedText {
            header,
            pattern_db: None,
            pattern_matches: Vec::new(),
            processed_text: text.to_string(),
            original_text: text.to_string(),
            token_count,
            exception_count: token_count,
        })
    }

    /// Encode text to bytes
    pub fn encode_to_bytes(&self, text: &str) -> Result<Vec<u8>> {
        let encoded = self.encode(text)?;
        self.to_bytes(&encoded)
    }

    /// Convert EncodedText to bytes
    pub fn to_bytes(&self, encoded: &EncodedText) -> Result<Vec<u8>> {
        let mut result = Vec::new();

        // Write magic
        result.extend_from_slice(ALICE_TEXT_MAGIC);

        // Write version
        result.push(ALICE_TEXT_VERSION.0);
        result.push(ALICE_TEXT_VERSION.1);

        // For optimal compression, just LZMA compress the original text
        // LZMA handles repetitive patterns (timestamps, IPs, etc.) very well
        let text_bytes = encoded.original_text.as_bytes();

        // Try pattern-based approach: store pattern values separately
        // This can help when patterns are highly repetitive
        let (payload, use_pattern_mode) = if self.mode == EncodingMode::Pattern
            && !encoded.pattern_matches.is_empty()
        {
            // Calculate overhead of pattern storage vs direct LZMA
            let pattern_payload = self.create_payload(encoded)?;
            let direct_compressed = self.compress_lzma(text_bytes)?;
            let pattern_compressed = self.compress_lzma(&pattern_payload)?;

            // Use pattern mode only if it results in smaller output
            if pattern_compressed.len() < direct_compressed.len() {
                (pattern_compressed, true)
            } else {
                (direct_compressed, false)
            }
        } else {
            // Direct compression
            (self.compress_lzma(text_bytes)?, false)
        };

        // Update header with sizes and mode flag
        let mut header = encoded.header.clone();
        header.compressed_length = payload.len() as u32;

        // Use pattern_db_length field as mode flag (0 = direct, 1 = pattern)
        header.pattern_db_length = if use_pattern_mode { 1 } else { 0 };

        // Write header
        result.extend_from_slice(&header.to_bytes());

        // Write compressed data
        result.extend_from_slice(&payload);

        Ok(result)
    }

    /// Create payload for compression
    fn create_payload(&self, encoded: &EncodedText) -> Result<Vec<u8>> {
        // Use a compact binary format:
        // - For pattern mode: store pattern_matches and processed_text
        // - For ngram mode: store just the original text
        //
        // Format:
        // [match_count: u32]
        // [matches: variable]
        //   - [pattern_type: u8]
        //   - [start: u32]
        //   - [end: u32]
        //   - [text_len: u16]
        //   - [text: bytes]
        // [processed_text_len: u32]
        // [processed_text: bytes]

        let mut payload = Vec::new();

        // Write match count
        let match_count = encoded.pattern_matches.len() as u32;
        payload.extend_from_slice(&match_count.to_le_bytes());

        // Write matches
        for mat in &encoded.pattern_matches {
            // Pattern type as u8
            let pt = match mat.pattern_type {
                crate::pattern_learner::PatternType::Timestamp => 0u8,
                crate::pattern_learner::PatternType::Date => 1,
                crate::pattern_learner::PatternType::Time => 2,
                crate::pattern_learner::PatternType::IPv4 => 3,
                crate::pattern_learner::PatternType::IPv6 => 4,
                crate::pattern_learner::PatternType::UUID => 5,
                crate::pattern_learner::PatternType::LogLevel => 6,
                crate::pattern_learner::PatternType::Path => 7,
                crate::pattern_learner::PatternType::URL => 8,
                crate::pattern_learner::PatternType::Number => 9,
                crate::pattern_learner::PatternType::Hex => 10,
                crate::pattern_learner::PatternType::Email => 11,
                crate::pattern_learner::PatternType::Custom => 12,
            };
            payload.push(pt);

            // Start and end positions
            payload.extend_from_slice(&(mat.start as u32).to_le_bytes());
            payload.extend_from_slice(&(mat.end as u32).to_le_bytes());

            // Matched text
            let text_bytes = mat.matched_text.as_bytes();
            payload.extend_from_slice(&(text_bytes.len() as u16).to_le_bytes());
            payload.extend_from_slice(text_bytes);
        }

        // Write processed text
        let text_bytes = encoded.processed_text.as_bytes();
        payload.extend_from_slice(&(text_bytes.len() as u32).to_le_bytes());
        payload.extend_from_slice(text_bytes);

        Ok(payload)
    }

    /// Compress data with LZMA
    fn compress_lzma(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut compressed = Vec::new();
        lzma_compress(&mut std::io::Cursor::new(data), &mut compressed)
            .map_err(|e| ALICETextError::EncodingError(format!("LZMA compression failed: {}", e)))?;
        Ok(compressed)
    }
}

impl Default for ExceptionEncoder {
    fn default() -> Self {
        Self::new(EncodingMode::Pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_serialization() {
        let mut header = ExceptionHeader::new(EncodingMode::Pattern);
        header.original_length = 1000;
        header.token_count = 100;
        header.exception_count = 50;

        let bytes = header.to_bytes();
        let restored = ExceptionHeader::from_bytes(&bytes).unwrap();

        assert_eq!(header.mode, restored.mode);
        assert_eq!(header.original_length, restored.original_length);
        assert_eq!(header.token_count, restored.token_count);
        assert_eq!(header.exception_count, restored.exception_count);
    }

    #[test]
    fn test_pattern_encoding() {
        let encoder = ExceptionEncoder::new(EncodingMode::Pattern);
        let text = "2024-01-15 INFO Test message from 192.168.1.100";

        let encoded = encoder.encode(text).unwrap();

        assert!(encoded.pattern_db.is_some());
        assert!(!encoded.pattern_matches.is_empty());
        assert!(encoded.header.original_length > 0);
    }

    #[test]
    fn test_encode_to_bytes() {
        let encoder = ExceptionEncoder::new(EncodingMode::Pattern);
        let text = "Hello, World!";

        let bytes = encoder.encode_to_bytes(text).unwrap();

        // Check magic
        assert_eq!(&bytes[0..8], ALICE_TEXT_MAGIC);

        // Check version
        assert_eq!(bytes[8], ALICE_TEXT_VERSION.0);
        assert_eq!(bytes[9], ALICE_TEXT_VERSION.1);
    }

    #[test]
    fn test_ngram_encoding() {
        let encoder = ExceptionEncoder::new(EncodingMode::NGram);
        let text = "Test message for n-gram encoding";

        let encoded = encoder.encode(text).unwrap();

        assert!(encoded.pattern_db.is_none());
        assert_eq!(encoded.processed_text, text);
    }
}
