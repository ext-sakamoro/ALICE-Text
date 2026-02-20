//! ALICE-Text × ALICE-ML Bridge
//!
//! Ternary neural inference for next-token prediction in text compression.
//! Uses 1.58-bit weights for ultra-fast context → prediction mapping.

use alice_ml::{TernaryWeight, ternary_matvec};

/// ML-accelerated text predictor.
///
/// Uses ternary weights to predict the next byte/token distribution
/// from a context window, improving exception-based compression.
pub struct TextPredictor {
    /// Context → logits weights.
    weights: TernaryWeight,
    /// Context window size.
    context_size: usize,
    /// Vocabulary size (output classes).
    vocab_size: usize,
}

impl TextPredictor {
    /// Create a text predictor from pre-trained ternary weights.
    ///
    /// - `weights`: ternary values (vocab_size × context_size).
    /// - `context_size`: number of input context features.
    /// - `vocab_size`: number of output prediction classes.
    pub fn new(weights: &[i8], context_size: usize, vocab_size: usize) -> Self {
        Self {
            weights: TernaryWeight::from_ternary(weights, vocab_size, context_size),
            context_size,
            vocab_size,
        }
    }

    /// Predict next-token logits from context features.
    ///
    /// Zero-allocation: writes logits directly to `output`.
    pub fn predict_logits(&self, context: &[f32], output: &mut [f32]) {
        debug_assert_eq!(context.len(), self.context_size);
        debug_assert!(output.len() >= self.vocab_size);
        ternary_matvec(context, &self.weights, &mut output[..self.vocab_size]);
    }

    /// Predict next-token probabilities (softmax over logits).
    pub fn predict_probs(&self, context: &[f32], output: &mut [f32]) {
        self.predict_logits(context, output);
        // Inline softmax to avoid Tensor API dependency
        let slice = &mut output[..self.vocab_size];
        let max = slice.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mut sum = 0.0f32;
        for v in slice.iter_mut() {
            *v = (*v - max).exp();
            sum += *v;
        }
        if sum > 0.0 {
            for v in slice.iter_mut() { *v /= sum; }
        }
    }

    /// Find the most likely next token.
    pub fn predict_top(&self, context: &[f32]) -> (usize, f32) {
        let mut logits = vec![0.0f32; self.vocab_size];
        self.predict_logits(context, &mut logits);
        let (idx, &val) = logits.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)).unwrap();
        (idx, val)
    }

    /// Context window size.
    pub fn context_size(&self) -> usize { self.context_size }
    /// Vocabulary size.
    pub fn vocab_size(&self) -> usize { self.vocab_size }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_predictor() {
        // 3 context features → 2 vocab classes
        let weights = [1i8, -1, 0, 0, 1, 1]; // 2×3

        let predictor = TextPredictor::new(&weights, 3, 2);

        let context = [1.0f32, 2.0, 3.0];
        let mut logits = [0.0f32; 2];
        predictor.predict_logits(&context, &mut logits);

        // Row 0: [1,-1,0] · [1,2,3] = 1-2+0 = -1
        // Row 1: [0,1,1] · [1,2,3] = 0+2+3 = 5
        assert!((logits[0] - (-1.0)).abs() < 1e-6);
        assert!((logits[1] - 5.0).abs() < 1e-6);

        let (top_idx, _) = predictor.predict_top(&context);
        assert_eq!(top_idx, 1); // class 1 has higher logit
    }
}
