//! Entropy estimation module for ALICE-Text
//!
//! Estimates compression efficiency without full compression.

use crate::pattern_learner::PatternLearner;
use std::collections::HashMap;

/// Entropy estimation result
#[derive(Debug, Clone)]
pub struct EntropyEstimate {
    /// Shannon entropy (bits per byte)
    pub shannon_entropy: f64,
    /// Estimated compression ratio (compressed/original)
    pub estimated_ratio: f64,
    /// Estimated compressed size in bytes
    pub estimated_size: usize,
    /// Original size in bytes
    pub original_size: usize,
    /// Estimated space savings (0.0 to 1.0)
    pub space_savings: f64,
    /// Pattern coverage (fraction of text matched by patterns)
    pub pattern_coverage: f64,
    /// Unique byte count
    pub unique_bytes: usize,
    /// Repetition score (higher = more repetitive)
    pub repetition_score: f64,
}

impl EntropyEstimate {
    /// Check if compression is likely beneficial
    pub fn is_compressible(&self) -> bool {
        self.estimated_ratio < 0.9
    }

    /// Get human-readable compression quality
    pub fn quality(&self) -> &'static str {
        match self.estimated_ratio {
            r if r < 0.1 => "Excellent",
            r if r < 0.3 => "Very Good",
            r if r < 0.5 => "Good",
            r if r < 0.7 => "Fair",
            r if r < 0.9 => "Poor",
            _ => "Not Recommended",
        }
    }
}

/// Entropy estimator for predicting compression efficiency
pub struct EntropyEstimator {
    /// Pattern learner for pattern detection
    pattern_learner: PatternLearner,
    /// Fixed overhead for headers (magic + version + header)
    header_overhead: usize,
}

impl EntropyEstimator {
    /// Header overhead: magic (8) + version (2) + header (32) = 42 bytes
    const HEADER_OVERHEAD: usize = 42;

    /// Create a new estimator
    pub fn new() -> Self {
        Self {
            pattern_learner: PatternLearner::new(),
            header_overhead: Self::HEADER_OVERHEAD,
        }
    }

    /// Estimate compression for text
    pub fn estimate(&self, text: &str) -> EntropyEstimate {
        let bytes = text.as_bytes();
        let original_size = bytes.len();

        if original_size == 0 {
            return EntropyEstimate {
                shannon_entropy: 0.0,
                estimated_ratio: 1.0,
                estimated_size: self.header_overhead,
                original_size: 0,
                space_savings: 0.0,
                pattern_coverage: 0.0,
                unique_bytes: 0,
                repetition_score: 0.0,
            };
        }

        // Calculate Shannon entropy
        let shannon_entropy = self.calculate_shannon_entropy(bytes);

        // Calculate pattern coverage
        let pattern_coverage = self.calculate_pattern_coverage(text);

        // Calculate repetition score
        let (repetition_score, unique_bytes) = self.calculate_repetition(bytes);

        // Estimate compression ratio
        let estimated_ratio = self.estimate_ratio(
            original_size,
            shannon_entropy,
            pattern_coverage,
            repetition_score,
        );

        // Calculate estimated size
        let estimated_size =
            (original_size as f64 * estimated_ratio).ceil() as usize + self.header_overhead;

        let inv_original = 1.0 / original_size as f64;
        let actual_ratio = estimated_size as f64 * inv_original;
        let space_savings = (1.0 - actual_ratio).max(0.0);

        EntropyEstimate {
            shannon_entropy,
            estimated_ratio: actual_ratio,
            estimated_size,
            original_size,
            space_savings,
            pattern_coverage,
            unique_bytes,
            repetition_score,
        }
    }

    /// Calculate Shannon entropy in bits per byte
    fn calculate_shannon_entropy(&self, data: &[u8]) -> f64 {
        if data.is_empty() {
            return 0.0;
        }

        let mut freq: HashMap<u8, usize> = HashMap::new();
        for &byte in data {
            *freq.entry(byte).or_insert(0) += 1;
        }

        let len = data.len() as f64;
        let inv_len = 1.0 / len;
        let mut entropy = 0.0;

        for &count in freq.values() {
            if count > 0 {
                let p = count as f64 * inv_len;
                entropy -= p * p.log2();
            }
        }

        entropy
    }

    /// Calculate pattern coverage
    fn calculate_pattern_coverage(&self, text: &str) -> f64 {
        if text.is_empty() {
            return 0.0;
        }

        let matches = self.pattern_learner.find_matches(text);
        let covered_chars: usize = matches.iter().map(|m| m.end - m.start).sum();

        let inv_len = 1.0 / text.len() as f64;
        covered_chars as f64 * inv_len
    }

    /// Calculate repetition score and unique byte count
    fn calculate_repetition(&self, data: &[u8]) -> (f64, usize) {
        if data.is_empty() {
            return (0.0, 0);
        }

        let mut freq: HashMap<u8, usize> = HashMap::new();
        for &byte in data {
            *freq.entry(byte).or_insert(0) += 1;
        }

        let unique_bytes = freq.len();
        let max_unique = 256.min(data.len());

        // Repetition score: lower unique bytes = higher repetition
        let inv_max_unique = 1.0 / max_unique as f64;
        let repetition_score = 1.0 - (unique_bytes as f64 * inv_max_unique);

        // Also factor in how repeated the most common bytes are
        let max_freq = *freq.values().max().unwrap_or(&0);
        let inv_data_len = 1.0 / data.len() as f64;
        let freq_factor = max_freq as f64 * inv_data_len;

        let combined_score = (repetition_score + freq_factor) * 0.5;

        (combined_score, unique_bytes)
    }

    /// Estimate compression ratio based on various factors
    fn estimate_ratio(
        &self,
        original_size: usize,
        shannon_entropy: f64,
        pattern_coverage: f64,
        repetition_score: f64,
    ) -> f64 {
        // Base ratio from Shannon entropy (theoretical minimum)
        // 8 bits per byte, so ratio = entropy * (1/8)
        const RCP_8: f64 = 1.0 / 8.0;
        let entropy_ratio = shannon_entropy * RCP_8;

        // Pattern bonus (patterns compress well)
        let pattern_factor = 1.0 - (pattern_coverage * 0.3);

        // Repetition bonus
        let repetition_factor = 1.0 - (repetition_score * 0.2);

        // LZMA typically achieves better than theoretical
        // but has overhead for small files
        let lzma_efficiency = if original_size < 100 {
            1.5 // Overhead dominates for small files
        } else if original_size < 1000 {
            0.9 // Some overhead
        } else {
            0.7 // Good compression for larger files
        };

        // Combined estimate
        let estimated = entropy_ratio * pattern_factor * repetition_factor * lzma_efficiency;

        // Clamp to reasonable range
        estimated.clamp(0.05, 1.5)
    }

    /// Quick entropy check without full estimation
    pub fn quick_entropy(&self, data: &[u8]) -> f64 {
        self.calculate_shannon_entropy(data)
    }

    /// Check if data is likely compressible
    pub fn is_compressible(&self, text: &str) -> bool {
        let estimate = self.estimate(text);
        estimate.is_compressible()
    }
}

impl Default for EntropyEstimator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entropy_estimation() {
        let estimator = EntropyEstimator::new();

        // Highly repetitive text should have low entropy
        let repetitive = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let estimate = estimator.estimate(repetitive);
        assert!(estimate.shannon_entropy < 1.0);
        assert!(estimate.repetition_score > 0.5);

        // Random-ish text should have higher entropy
        let varied = "The quick brown fox jumps over the lazy dog 1234567890!@#$%";
        let estimate2 = estimator.estimate(varied);
        assert!(estimate2.shannon_entropy > estimate.shannon_entropy);
    }

    #[test]
    fn test_pattern_coverage() {
        let estimator = EntropyEstimator::new();

        // Text with many patterns
        let log_line = "2024-01-15 10:30:45 INFO 192.168.1.100 user@example.com";
        let estimate = estimator.estimate(log_line);
        assert!(estimate.pattern_coverage > 0.3);

        // Text with few patterns
        let plain_text = "hello world how are you";
        let estimate2 = estimator.estimate(plain_text);
        assert!(estimate2.pattern_coverage < estimate.pattern_coverage);
    }

    #[test]
    fn test_empty_text() {
        let estimator = EntropyEstimator::new();
        let estimate = estimator.estimate("");

        assert_eq!(estimate.original_size, 0);
        assert_eq!(estimate.shannon_entropy, 0.0);
    }

    #[test]
    fn test_compression_quality() {
        let estimator = EntropyEstimator::new();

        // Repetitive log data should be "Good" or better
        let logs = "2024-01-15 INFO test\n".repeat(100);
        let estimate = estimator.estimate(&logs);
        assert!(["Excellent", "Very Good", "Good"].contains(&estimate.quality()));
    }

    #[test]
    fn test_is_compressible() {
        let estimator = EntropyEstimator::new();

        // Repetitive data should be compressible
        let repetitive = "test ".repeat(100);
        assert!(estimator.is_compressible(&repetitive));
    }

    #[test]
    fn test_small_file_overhead() {
        let estimator = EntropyEstimator::new();

        // Small files have header overhead
        let small = "Hi";
        let estimate = estimator.estimate(small);

        // Estimated size includes header overhead
        assert!(estimate.estimated_size >= EntropyEstimator::HEADER_OVERHEAD);
    }

    #[test]
    fn test_quick_entropy() {
        let estimator = EntropyEstimator::new();

        let data = b"hello world";
        let entropy = estimator.quick_entropy(data);

        assert!(entropy > 0.0);
        assert!(entropy <= 8.0); // Max 8 bits per byte
    }

    #[test]
    fn test_quick_entropy_empty_data() {
        let estimator = EntropyEstimator::new();
        let entropy = estimator.quick_entropy(b"");
        assert_eq!(entropy, 0.0);
    }

    #[test]
    fn test_quick_entropy_single_byte() {
        let estimator = EntropyEstimator::new();
        let entropy = estimator.quick_entropy(b"A");
        // Single byte has zero entropy (only one symbol)
        assert_eq!(entropy, 0.0);
    }

    #[test]
    fn test_entropy_estimate_quality_labels() {
        let estimator = EntropyEstimator::new();

        // Single char repeated many times should be excellent compression
        let repetitive = "a".repeat(10000);
        let estimate = estimator.estimate(&repetitive);
        // Shannon entropy should be 0 for single-character text
        assert_eq!(estimate.shannon_entropy, 0.0);
        assert!(estimate.repetition_score > 0.5);

        // Unicode text
        let text = "Hello World! How are you today?";
        let estimate2 = estimator.estimate(text);
        assert!(estimate2.unique_bytes > 5);
    }

    #[test]
    fn test_is_compressible_short_text() {
        let estimator = EntropyEstimator::new();
        // Very short text typically not compressible due to header overhead
        let short = "Hi";
        let estimate = estimator.estimate(short);
        // With header overhead, estimated_size >> original_size, so ratio > 0.9
        assert!(!estimate.is_compressible() || estimate.estimated_ratio >= 0.9);
    }

    #[test]
    fn test_entropy_estimate_space_savings_non_negative() {
        let estimator = EntropyEstimator::new();
        // For any text, space_savings should be >= 0.0
        for text in &["", "a", "Hello", "test ".repeat(100).as_str()] {
            let estimate = estimator.estimate(text);
            assert!(
                estimate.space_savings >= 0.0,
                "space_savings should be non-negative, got {} for text len {}",
                estimate.space_savings,
                text.len()
            );
        }
    }
}
