//! ALICE-Text x ALICE-Voice Bridge
//!
//! Analyze compressed text for speech-like patterns and provide
//! phoneme/prosody hints to ALICE-Voice's parametric codec.
//!
//! # Concept
//!
//! Text compression statistics (exception rate, pattern density) correlate
//! with speech regularity. Low exception rate => regular speech patterns
//! => LPC parametric encoding works well. High exception rate => irregular
//! content => spectral encoding preserves fidelity better.
//!
//! ```text
//! Text Input
//!     |
//!     v
//! ALICE-Text compress() --> CompressionStats
//!     |                         |
//!     v                         v
//! compressed bytes        exception_rate, ratio
//!                               |
//!                               v
//!                         SpeechHints { mode, phonemes, ... }
//!                               |
//!                               v
//!                     ALICE-Voice VoiceCodec (select layer)
//! ```

use alice_voice::{VoiceQuality, VoiceCodecConfig};
use crate::{ALICEText, EncodingMode};

/// Speech encoding hints derived from text analysis.
///
/// These hints guide ALICE-Voice's codec selection: parametric (L2)
/// for regular speech patterns, spectral (L1) for irregular content.
#[derive(Clone, Debug)]
pub struct SpeechHints {
    /// Estimated phoneme count from text patterns.
    pub estimated_phonemes: usize,
    /// Text exception rate (higher = more irregular speech).
    pub exception_rate: f64,
    /// Suggested Voice codec layer based on text analysis.
    pub suggested_mode: VoiceMode,
    /// Compression ratio achieved by text encoder.
    pub text_compression_ratio: f64,
    /// Suggested voice quality based on text density.
    pub suggested_quality: VoiceQuality,
}

/// Suggested voice codec layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceMode {
    /// L2: Parametric (LPC) - for regular speech patterns.
    /// Achieves 100-600x compression.
    Parametric,
    /// L1: Spectral (FFT/DCT) - for irregular/musical content.
    /// Achieves 10-50x compression with higher fidelity.
    Spectral,
}

/// Analyze text to produce speech encoding hints.
///
/// Compresses the text with ALICE-Text and uses the resulting statistics
/// to recommend a Voice codec mode.
pub fn analyze_for_speech(text: &str) -> SpeechHints {
    let mut encoder = ALICEText::new(EncodingMode::Pattern);
    let _compressed = encoder.compress(text).unwrap_or_default();

    let (exception_rate, ratio) = match encoder.last_stats() {
        Some(s) => (s.exception_rate(), s.compression_ratio()),
        None => (1.0, 1.0),
    };

    // Estimate phonemes: ~1 phoneme per 3 characters for English speech
    let estimated_phonemes = (text.len() + 2) / 3;

    // Regular patterns => LPC parametric codec handles well
    // Irregular patterns => spectral codec preserves fidelity
    let suggested_mode = if exception_rate < 0.3 {
        VoiceMode::Parametric
    } else {
        VoiceMode::Spectral
    };

    // Dense text (high compression) suggests wideband; sparse suggests narrowband
    let suggested_quality = if ratio < 0.3 {
        VoiceQuality::High
    } else if ratio < 0.6 {
        VoiceQuality::Medium
    } else {
        VoiceQuality::Low
    };

    SpeechHints {
        estimated_phonemes,
        exception_rate,
        suggested_mode,
        text_compression_ratio: ratio,
        suggested_quality,
    }
}

/// Compress a text transcript and produce speech hints.
///
/// Returns the compressed transcript bytes alongside hints for the
/// Voice codec. This avoids double-compressing when both text and
/// audio channels are used.
pub fn compress_transcript(text: &str) -> (Vec<u8>, SpeechHints) {
    let mut encoder = ALICEText::new(EncodingMode::Pattern);
    let compressed = encoder.compress(text).unwrap_or_default();

    let (exception_rate, ratio) = match encoder.last_stats() {
        Some(s) => (s.exception_rate(), s.compression_ratio()),
        None => (1.0, 1.0),
    };

    let estimated_phonemes = (text.len() + 2) / 3;

    let suggested_mode = if exception_rate < 0.3 {
        VoiceMode::Parametric
    } else {
        VoiceMode::Spectral
    };

    let suggested_quality = if ratio < 0.3 {
        VoiceQuality::High
    } else if ratio < 0.6 {
        VoiceQuality::Medium
    } else {
        VoiceQuality::Low
    };

    let hints = SpeechHints {
        estimated_phonemes,
        exception_rate,
        suggested_mode,
        text_compression_ratio: ratio,
        suggested_quality,
    };

    (compressed, hints)
}

/// Build a `VoiceCodecConfig` from speech hints.
///
/// Convenience function that maps `SpeechHints` to an appropriate
/// ALICE-Voice codec configuration.
pub fn codec_config_from_hints(hints: &SpeechHints) -> VoiceCodecConfig {
    VoiceCodecConfig::for_quality(hints.suggested_quality)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_regular_speech() {
        // Repetitive log-like text => low exception rate => Parametric
        let text = "Hello hello hello hello hello hello hello hello";
        let hints = analyze_for_speech(text);

        assert!(hints.estimated_phonemes > 0);
        assert!(hints.exception_rate >= 0.0);
        assert!(hints.text_compression_ratio > 0.0);
    }

    #[test]
    fn test_analyze_empty() {
        let hints = analyze_for_speech("");
        assert_eq!(hints.estimated_phonemes, 0);
    }

    #[test]
    fn test_compress_transcript() {
        let text = "2024-01-15 INFO Server started on port 8080";
        let (compressed, hints) = compress_transcript(text);

        assert!(!compressed.is_empty());
        assert!(hints.estimated_phonemes > 0);
        assert!(hints.text_compression_ratio > 0.0);
    }

    #[test]
    fn test_voice_mode_variants() {
        assert_ne!(VoiceMode::Parametric, VoiceMode::Spectral);
        assert_eq!(VoiceMode::Parametric, VoiceMode::Parametric);
    }

    #[test]
    fn test_codec_config_from_hints() {
        let hints = SpeechHints {
            estimated_phonemes: 10,
            exception_rate: 0.1,
            suggested_mode: VoiceMode::Parametric,
            text_compression_ratio: 0.5,
            suggested_quality: VoiceQuality::Medium,
        };

        let config = codec_config_from_hints(&hints);
        assert_eq!(config.sample_rate, 16000);
        assert_eq!(config.lpc_order, 10);
    }
}
