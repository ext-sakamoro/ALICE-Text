//! Exception decoder module for ALICE-Text
//!
//! Decodes compressed data back to original text.

use crate::exception_encoder::ExceptionHeader;
use crate::pattern_learner::{PatternLearner, PatternMatch};
use crate::{ALICETextError, Result, ALICE_TEXT_MAGIC, ALICE_TEXT_VERSION};
use lzma_rs::lzma_decompress;

/// Exception decoder
pub struct ExceptionDecoder {
    /// Pattern learner for restoration
    pattern_learner: PatternLearner,
}

impl ExceptionDecoder {
    /// Create a new decoder
    pub fn new() -> Self {
        Self {
            pattern_learner: PatternLearner::new(),
        }
    }

    /// Decode bytes to text
    pub fn decode_from_bytes(&self, data: &[u8]) -> Result<String> {
        // Minimum size: magic (8) + version (2) + header (32) = 42
        if data.len() < 42 {
            return Err(ALICETextError::DecompressionError(
                "Data too short".to_string(),
            ));
        }

        // Verify magic
        if &data[0..8] != ALICE_TEXT_MAGIC {
            return Err(ALICETextError::InvalidMagic);
        }

        // Verify version
        let version = (data[8], data[9]);
        if version.0 > ALICE_TEXT_VERSION.0 {
            return Err(ALICETextError::InvalidVersion(version.0, version.1));
        }

        // Parse header
        let header = ExceptionHeader::from_bytes(&data[10..42])?;

        // Get compressed data
        let compressed_data = &data[42..];

        // Decompress
        let decompressed = self.decompress_lzma(compressed_data)?;

        // Check if pattern mode was used (pattern_db_length field used as flag)
        let use_pattern_mode = header.pattern_db_length == 1;

        // Decode based on mode
        if use_pattern_mode {
            self.decode_pattern(&decompressed)
        } else {
            // Direct decompression - data is just the original text
            String::from_utf8(decompressed)
                .map_err(|e| ALICETextError::DecompressionError(format!("UTF-8 error: {}", e)))
        }
    }

    /// Decode pattern-based encoding
    fn decode_pattern(&self, data: &[u8]) -> Result<String> {
        // Parse binary format
        let (pattern_matches, processed_text) = self.parse_binary_payload(data)?;

        // Restore patterns
        let text = self
            .pattern_learner
            .restore_patterns(&processed_text, &pattern_matches);

        Ok(text)
    }

    /// Parse binary payload format
    fn parse_binary_payload(&self, data: &[u8]) -> Result<(Vec<PatternMatch>, String)> {
        use crate::pattern_learner::PatternType;

        let slice_err = || ALICETextError::DecompressionError("Payload slice error".to_string());
        let mut pos = 0;

        // Read match count
        if data.len() < 4 {
            return Err(ALICETextError::DecompressionError("Payload too short".to_string()));
        }
        let match_count = u32::from_le_bytes(data[pos..pos + 4].try_into().map_err(|_| slice_err())?) as usize;
        pos += 4;

        // Read matches
        let mut pattern_matches = Vec::with_capacity(match_count);
        for i in 0..match_count {
            if pos + 11 > data.len() {
                return Err(ALICETextError::DecompressionError("Truncated match data".to_string()));
            }

            // Pattern type
            let pt = match data[pos] {
                0 => PatternType::Timestamp,
                1 => PatternType::Date,
                2 => PatternType::Time,
                3 => PatternType::IPv4,
                4 => PatternType::IPv6,
                5 => PatternType::UUID,
                6 => PatternType::LogLevel,
                7 => PatternType::Path,
                8 => PatternType::URL,
                9 => PatternType::Number,
                10 => PatternType::Hex,
                11 => PatternType::Email,
                _ => PatternType::Custom,
            };
            pos += 1;

            // Start and end
            let start = u32::from_le_bytes(data[pos..pos + 4].try_into().map_err(|_| slice_err())?) as usize;
            pos += 4;
            let end = u32::from_le_bytes(data[pos..pos + 4].try_into().map_err(|_| slice_err())?) as usize;
            pos += 4;

            // Text length and text
            let text_len = u16::from_le_bytes(data[pos..pos + 2].try_into().map_err(|_| slice_err())?) as usize;
            pos += 2;

            if pos + text_len > data.len() {
                return Err(ALICETextError::DecompressionError("Truncated text data".to_string()));
            }
            let matched_text = String::from_utf8_lossy(&data[pos..pos + text_len]).to_string();
            pos += text_len;

            pattern_matches.push(PatternMatch {
                pattern_type: pt,
                start,
                end,
                matched_text,
                pattern_index: i,
            });
        }

        // Read processed text
        if pos + 4 > data.len() {
            return Err(ALICETextError::DecompressionError("Missing processed text length".to_string()));
        }
        let text_len = u32::from_le_bytes(data[pos..pos + 4].try_into().map_err(|_| slice_err())?) as usize;
        pos += 4;

        if pos + text_len > data.len() {
            return Err(ALICETextError::DecompressionError("Truncated processed text".to_string()));
        }
        let processed_text = String::from_utf8_lossy(&data[pos..pos + text_len]).to_string();

        Ok((pattern_matches, processed_text))
    }

    /// Decompress LZMA data
    fn decompress_lzma(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut decompressed = Vec::new();
        lzma_decompress(&mut std::io::Cursor::new(data), &mut decompressed).map_err(|e| {
            ALICETextError::DecompressionError(format!("LZMA decompression failed: {}", e))
        })?;
        Ok(decompressed)
    }

    /// Get header from compressed data
    pub fn read_header(&self, data: &[u8]) -> Result<ExceptionHeader> {
        if data.len() < 42 {
            return Err(ALICETextError::DecompressionError(
                "Data too short".to_string(),
            ));
        }

        // Verify magic
        if &data[0..8] != ALICE_TEXT_MAGIC {
            return Err(ALICETextError::InvalidMagic);
        }

        ExceptionHeader::from_bytes(&data[10..42])
    }

    /// Verify data integrity without full decompression
    pub fn verify(&self, data: &[u8]) -> Result<bool> {
        // Check minimum size
        if data.len() < 42 {
            return Ok(false);
        }

        // Verify magic
        if &data[0..8] != ALICE_TEXT_MAGIC {
            return Ok(false);
        }

        // Verify version
        let version = (data[8], data[9]);
        if version.0 > ALICE_TEXT_VERSION.0 {
            return Ok(false);
        }

        // Try to parse header
        ExceptionHeader::from_bytes(&data[10..42])?;

        Ok(true)
    }
}

impl Default for ExceptionDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exception_encoder::{EncodingMode, ExceptionEncoder};

    #[test]
    fn test_decode_pattern_roundtrip() {
        let encoder = ExceptionEncoder::new(EncodingMode::Pattern);
        let decoder = ExceptionDecoder::new();
        let text = "2024-01-15 10:30:45 INFO User logged in from 192.168.1.100";

        let compressed = encoder.encode_to_bytes(text).unwrap();
        let decompressed = decoder.decode_from_bytes(&compressed).unwrap();

        assert_eq!(text, decompressed);
    }

    #[test]
    fn test_decode_ngram_roundtrip() {
        let encoder = ExceptionEncoder::new(EncodingMode::NGram);
        let decoder = ExceptionDecoder::new();
        let text = "Hello, World! This is a test message.";

        let compressed = encoder.encode_to_bytes(text).unwrap();
        let decompressed = decoder.decode_from_bytes(&compressed).unwrap();

        assert_eq!(text, decompressed);
    }

    #[test]
    fn test_verify_valid_data() {
        let encoder = ExceptionEncoder::new(EncodingMode::Pattern);
        let decoder = ExceptionDecoder::new();
        let text = "Test data";

        let compressed = encoder.encode_to_bytes(text).unwrap();
        assert!(decoder.verify(&compressed).unwrap());
    }

    #[test]
    fn test_verify_invalid_magic() {
        let decoder = ExceptionDecoder::new();
        let invalid_data = vec![0u8; 50]; // Invalid magic

        assert!(!decoder.verify(&invalid_data).unwrap());
    }

    #[test]
    fn test_verify_too_short() {
        let decoder = ExceptionDecoder::new();
        let short_data = vec![0u8; 10];

        assert!(!decoder.verify(&short_data).unwrap());
    }

    #[test]
    fn test_read_header() {
        let encoder = ExceptionEncoder::new(EncodingMode::Pattern);
        let decoder = ExceptionDecoder::new();
        let text = "Test message";

        let compressed = encoder.encode_to_bytes(text).unwrap();
        let header = decoder.read_header(&compressed).unwrap();

        assert_eq!(header.mode, EncodingMode::Pattern);
        assert_eq!(header.original_length, text.len() as u32);
    }

    #[test]
    fn test_empty_text_roundtrip() {
        let encoder = ExceptionEncoder::new(EncodingMode::Pattern);
        let decoder = ExceptionDecoder::new();
        let text = "";

        let compressed = encoder.encode_to_bytes(text).unwrap();
        let decompressed = decoder.decode_from_bytes(&compressed).unwrap();

        assert_eq!(text, decompressed);
    }

    #[test]
    fn test_multiline_roundtrip() {
        let encoder = ExceptionEncoder::new(EncodingMode::Pattern);
        let decoder = ExceptionDecoder::new();
        let text = "Line 1\nLine 2\nLine 3";

        let compressed = encoder.encode_to_bytes(text).unwrap();
        let decompressed = decoder.decode_from_bytes(&compressed).unwrap();

        assert_eq!(text, decompressed);
    }

    #[test]
    fn test_log_multiline_roundtrip() {
        let encoder = ExceptionEncoder::new(EncodingMode::Pattern);
        let decoder = ExceptionDecoder::new();
        let text = "2024-01-15 10:30:45 INFO Server started\n\
                    2024-01-15 10:30:46 INFO Listening on port 8080\n\
                    2024-01-15 10:31:00 WARN High memory usage";

        let compressed = encoder.encode_to_bytes(text).unwrap();
        let decompressed = decoder.decode_from_bytes(&compressed).unwrap();

        assert_eq!(text, decompressed);
    }
}
