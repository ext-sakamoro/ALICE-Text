//! Arithmetic coding module for ALICE-Text
//!
//! Provides entropy-optimal encoding for symbol sequences.

use std::collections::HashMap;

/// Precision bits for arithmetic coding
const PRECISION_BITS: u32 = 31; // Use 31 bits to avoid overflow issues
const WHOLE: u64 = 1 << PRECISION_BITS;
const HALF: u64 = WHOLE / 2;
const QUARTER: u64 = WHOLE / 4;
const THREE_QUARTERS: u64 = 3 * QUARTER;

/// Frequency model for symbols
#[derive(Debug, Clone)]
pub struct FrequencyModel {
    /// Symbol frequencies
    frequencies: HashMap<u8, u64>,
    /// Total frequency count
    total: u64,
    /// Cumulative frequencies (for encoding)
    cumulative: Vec<(u8, u64, u64)>, // (symbol, low, high)
}

impl FrequencyModel {
    /// Create a new frequency model
    pub fn new() -> Self {
        Self {
            frequencies: HashMap::new(),
            total: 0,
            cumulative: Vec::new(),
        }
    }

    /// Create from byte data
    pub fn from_data(data: &[u8]) -> Self {
        let mut model = Self::new();
        for &byte in data {
            model.add_symbol(byte);
        }
        model.build_cumulative();
        model
    }

    /// Add a symbol occurrence
    pub fn add_symbol(&mut self, symbol: u8) {
        *self.frequencies.entry(symbol).or_insert(0) += 1;
        self.total += 1;
    }

    /// Build cumulative frequency table
    pub fn build_cumulative(&mut self) {
        self.cumulative.clear();
        let mut cumsum: u64 = 0;

        // Sort by symbol for deterministic ordering
        let mut symbols: Vec<_> = self.frequencies.iter().collect();
        symbols.sort_by_key(|(k, _)| *k);

        for (&symbol, &freq) in symbols {
            let low = cumsum;
            cumsum += freq;
            let high = cumsum;
            self.cumulative.push((symbol, low, high));
        }
    }

    /// Get probability range for a symbol
    pub fn get_range(&self, symbol: u8) -> Option<(u64, u64, u64)> {
        for &(s, low, high) in &self.cumulative {
            if s == symbol {
                return Some((low, high, self.total));
            }
        }
        None
    }

    /// Get symbol from cumulative frequency value
    pub fn get_symbol(&self, value: u64) -> Option<u8> {
        for &(symbol, low, high) in &self.cumulative {
            if value >= low && value < high {
                return Some(symbol);
            }
        }
        // Handle edge case where value equals total
        if let Some(&(symbol, _, high)) = self.cumulative.last() {
            if value == high {
                return Some(symbol);
            }
        }
        None
    }

    /// Get total frequency
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Check if model is empty
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }
}

impl Default for FrequencyModel {
    fn default() -> Self {
        Self::new()
    }
}

/// Arithmetic encoder
pub struct ArithmeticEncoder {
    /// Low bound
    low: u64,
    /// High bound
    high: u64,
    /// Pending bits for output
    pending_bits: u32,
    /// Output buffer
    output: Vec<u8>,
    /// Current output byte
    current_byte: u8,
    /// Bits written to current byte
    bits_in_byte: u8,
}

impl ArithmeticEncoder {
    /// Create a new encoder
    pub fn new() -> Self {
        Self {
            low: 0,
            high: WHOLE - 1,
            pending_bits: 0,
            output: Vec::new(),
            current_byte: 0,
            bits_in_byte: 0,
        }
    }

    /// Encode a symbol using u128 for intermediate calculations
    #[inline(always)]
    pub fn encode_symbol(&mut self, symbol: u8, model: &FrequencyModel) {
        if let Some((sym_low, sym_high, total)) = model.get_range(symbol) {
            let range = (self.high - self.low + 1) as u128;
            // Pre-compute total once to avoid repeated division setup
            let total128 = total as u128;

            // Use u128 to avoid overflow
            self.high = self.low + ((range * sym_high as u128 / total128) as u64) - 1;
            self.low += (range * sym_low as u128 / total128) as u64;

            self.normalize();
        }
    }

    /// Encode data using a frequency model
    pub fn encode(&mut self, data: &[u8], model: &FrequencyModel) {
        for &byte in data {
            self.encode_symbol(byte, model);
        }
    }

    /// Normalize and output bits
    #[inline(always)]
    fn normalize(&mut self) {
        loop {
            if self.high < HALF {
                // Output 0 and pending 1s
                self.output_bit(0);
                while self.pending_bits > 0 {
                    self.output_bit(1);
                    self.pending_bits -= 1;
                }
            } else if self.low >= HALF {
                // Output 1 and pending 0s
                self.output_bit(1);
                while self.pending_bits > 0 {
                    self.output_bit(0);
                    self.pending_bits -= 1;
                }
                self.low -= HALF;
                self.high -= HALF;
            } else if self.low >= QUARTER && self.high < THREE_QUARTERS {
                // Middle case - increment pending
                self.pending_bits += 1;
                self.low -= QUARTER;
                self.high -= QUARTER;
            } else {
                break;
            }

            // Scale up
            self.low *= 2;
            self.high = self.high * 2 + 1;
        }
    }

    /// Output a single bit
    #[inline(always)]
    fn output_bit(&mut self, bit: u8) {
        self.current_byte = (self.current_byte << 1) | (bit & 1);
        self.bits_in_byte += 1;

        if self.bits_in_byte == 8 {
            self.output.push(self.current_byte);
            self.current_byte = 0;
            self.bits_in_byte = 0;
        }
    }

    /// Finish encoding and get output
    pub fn finish(mut self) -> Vec<u8> {
        // Output final bits to distinguish the interval
        self.pending_bits += 1;
        if self.low < QUARTER {
            self.output_bit(0);
            while self.pending_bits > 0 {
                self.output_bit(1);
                self.pending_bits -= 1;
            }
        } else {
            self.output_bit(1);
            while self.pending_bits > 0 {
                self.output_bit(0);
                self.pending_bits -= 1;
            }
        }

        // Flush remaining bits with padding
        if self.bits_in_byte > 0 {
            self.current_byte <<= 8 - self.bits_in_byte;
            self.output.push(self.current_byte);
        }

        self.output
    }

    /// Get current encoded size
    pub fn encoded_size(&self) -> usize {
        self.output.len()
    }
}

impl Default for ArithmeticEncoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Arithmetic decoder
pub struct ArithmeticDecoder {
    /// Low bound
    low: u64,
    /// High bound
    high: u64,
    /// Current code value
    code: u64,
    /// Input data
    input: Vec<u8>,
    /// Current byte position in input
    byte_pos: usize,
    /// Current bit position in byte (0-7)
    bit_pos: u8,
}

impl ArithmeticDecoder {
    /// Create a new decoder
    pub fn new(data: Vec<u8>) -> Self {
        let mut decoder = Self {
            low: 0,
            high: WHOLE - 1,
            code: 0,
            input: data,
            byte_pos: 0,
            bit_pos: 0,
        };

        // Read initial code value
        for _ in 0..PRECISION_BITS {
            decoder.code = (decoder.code << 1) | decoder.read_bit() as u64;
        }

        decoder
    }

    /// Read a single bit from input
    #[inline(always)]
    fn read_bit(&mut self) -> u8 {
        if self.byte_pos >= self.input.len() {
            return 0; // Pad with zeros
        }

        let bit = (self.input[self.byte_pos] >> (7 - self.bit_pos)) & 1;
        self.bit_pos += 1;

        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }

        bit
    }

    /// Decode a symbol using u128 for intermediate calculations
    #[inline(always)]
    pub fn decode_symbol(&mut self, model: &FrequencyModel) -> Option<u8> {
        if model.is_empty() {
            return None;
        }

        let range = (self.high - self.low + 1) as u128;
        // Pre-compute total once to avoid repeated division setup
        let total128 = model.total() as u128;

        // Calculate the cumulative frequency value
        // value = ((code - low + 1) * total - 1) / range
        let code_offset = (self.code - self.low) as u128;
        let value = ((code_offset + 1) * total128 - 1) / range;

        // Find symbol for this value
        let symbol = model.get_symbol(value as u64)?;
        let (sym_low, sym_high, _) = model.get_range(symbol)?;

        // Update interval using u128
        self.high = self.low + ((range * sym_high as u128 / total128) as u64) - 1;
        self.low += (range * sym_low as u128 / total128) as u64;

        // Normalize
        self.normalize();

        Some(symbol)
    }

    /// Decode n symbols
    pub fn decode(&mut self, model: &FrequencyModel, count: usize) -> Vec<u8> {
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            if let Some(symbol) = self.decode_symbol(model) {
                result.push(symbol);
            } else {
                break;
            }
        }
        result
    }

    /// Normalize decoder state
    #[inline(always)]
    fn normalize(&mut self) {
        loop {
            if self.high < HALF {
                // Do nothing special, just scale
            } else if self.low >= HALF {
                self.code -= HALF;
                self.low -= HALF;
                self.high -= HALF;
            } else if self.low >= QUARTER && self.high < THREE_QUARTERS {
                self.code -= QUARTER;
                self.low -= QUARTER;
                self.high -= QUARTER;
            } else {
                break;
            }

            // Scale up
            self.low *= 2;
            self.high = self.high * 2 + 1;
            self.code = self.code * 2 + self.read_bit() as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frequency_model() {
        let data = b"hello world";
        let model = FrequencyModel::from_data(data);

        assert_eq!(model.total(), 11);
        assert!(model.get_range(b'l').is_some());
    }

    #[test]
    fn test_arithmetic_coding_roundtrip() {
        let data = b"hello";
        let model = FrequencyModel::from_data(data);

        let mut encoder = ArithmeticEncoder::new();
        encoder.encode(data, &model);
        let encoded = encoder.finish();

        let mut decoder = ArithmeticDecoder::new(encoded);
        let decoded = decoder.decode(&model, data.len());

        assert_eq!(data.to_vec(), decoded);
    }

    #[test]
    fn test_longer_text() {
        let data = b"The quick brown fox jumps over the lazy dog.";
        let model = FrequencyModel::from_data(data);

        let mut encoder = ArithmeticEncoder::new();
        encoder.encode(data, &model);
        let encoded = encoder.finish();

        let mut decoder = ArithmeticDecoder::new(encoded);
        let decoded = decoder.decode(&model, data.len());

        assert_eq!(data.to_vec(), decoded);
    }

    #[test]
    fn test_repetitive_data() {
        let data = b"aaaaaaaaaa";
        let model = FrequencyModel::from_data(data);

        let mut encoder = ArithmeticEncoder::new();
        encoder.encode(data, &model);
        let encoded = encoder.finish();

        // Repetitive data should compress well
        assert!(encoded.len() < data.len());

        let mut decoder = ArithmeticDecoder::new(encoded);
        let decoded = decoder.decode(&model, data.len());

        assert_eq!(data.to_vec(), decoded);
    }

    #[test]
    fn test_empty_data() {
        let _model = FrequencyModel::new();
        let encoder = ArithmeticEncoder::new();
        let encoded = encoder.finish();

        assert!(!encoded.is_empty()); // Has termination bits
    }

    #[test]
    fn test_single_byte() {
        let data = b"x";
        let model = FrequencyModel::from_data(data);

        let mut encoder = ArithmeticEncoder::new();
        encoder.encode(data, &model);
        let encoded = encoder.finish();

        let mut decoder = ArithmeticDecoder::new(encoded);
        let decoded = decoder.decode(&model, data.len());

        assert_eq!(data.to_vec(), decoded);
    }

    #[test]
    fn test_binary_data() {
        let data: Vec<u8> = (0..=255).collect();
        let model = FrequencyModel::from_data(&data);

        let mut encoder = ArithmeticEncoder::new();
        encoder.encode(&data, &model);
        let encoded = encoder.finish();

        let mut decoder = ArithmeticDecoder::new(encoded);
        let decoded = decoder.decode(&model, data.len());

        assert_eq!(data, decoded);
    }

    #[test]
    fn test_compression_ratio() {
        // Data with skewed distribution should compress well
        let mut data = Vec::new();
        for _ in 0..100 {
            data.push(b'a');
        }
        for _ in 0..10 {
            data.push(b'b');
        }
        data.push(b'c');

        let model = FrequencyModel::from_data(&data);

        let mut encoder = ArithmeticEncoder::new();
        encoder.encode(&data, &model);
        let encoded = encoder.finish();

        // Should achieve good compression
        assert!(encoded.len() < data.len() / 2);

        let mut decoder = ArithmeticDecoder::new(encoded);
        let decoded = decoder.decode(&model, data.len());

        assert_eq!(data, decoded);
    }

    #[test]
    fn test_frequency_model_empty() {
        let model = FrequencyModel::new();
        assert!(model.is_empty());
        assert_eq!(model.total(), 0);
        assert!(model.get_range(b'a').is_none());
        assert!(model.get_symbol(0).is_none());
    }

    #[test]
    fn test_frequency_model_single_symbol() {
        let mut model = FrequencyModel::new();
        model.add_symbol(b'z');
        model.build_cumulative();
        assert_eq!(model.total(), 1);
        assert!(!model.is_empty());
        let (low, high, total) = model.get_range(b'z').unwrap();
        assert_eq!(low, 0);
        assert_eq!(high, 1);
        assert_eq!(total, 1);
        assert_eq!(model.get_symbol(0), Some(b'z'));
    }

    #[test]
    fn test_frequency_model_deterministic_ordering() {
        let data = b"dcba";
        let model = FrequencyModel::from_data(data);
        // Cumulative should be sorted by symbol value (a < b < c < d)
        let (a_low, a_high, _) = model.get_range(b'a').unwrap();
        let (b_low, b_high, _) = model.get_range(b'b').unwrap();
        let (c_low, c_high, _) = model.get_range(b'c').unwrap();
        let (d_low, _, _) = model.get_range(b'd').unwrap();
        assert_eq!(a_low, 0);
        assert_eq!(a_high, b_low);
        assert_eq!(b_high, c_low);
        assert_eq!(c_high, d_low);
    }

    #[test]
    fn test_roundtrip_two_distinct_symbols() {
        let data = b"ababab";
        let model = FrequencyModel::from_data(data);
        let mut encoder = ArithmeticEncoder::new();
        encoder.encode(data, &model);
        let encoded = encoder.finish();
        let mut decoder = ArithmeticDecoder::new(encoded);
        let decoded = decoder.decode(&model, data.len());
        assert_eq!(data.to_vec(), decoded);
    }

    #[test]
    fn test_encoded_size_tracks_output() {
        let data = b"hello world test data";
        let model = FrequencyModel::from_data(data);
        let mut encoder = ArithmeticEncoder::new();
        let initial_size = encoder.encoded_size();
        assert_eq!(initial_size, 0);
        encoder.encode(data, &model);
        // After encoding, some bytes may have been output
        let encoded = encoder.finish();
        assert!(encoded.len() > 0);
    }

    #[test]
    fn test_decode_with_empty_model_returns_none() {
        let model = FrequencyModel::new();
        let mut decoder = ArithmeticDecoder::new(vec![0xAA, 0xBB, 0xCC, 0xDD]);
        let result = decoder.decode_symbol(&model);
        assert!(result.is_none());
    }

    #[test]
    fn test_roundtrip_long_repetitive_sequence() {
        // 1000 bytes of the same character
        let data = vec![b'X'; 1000];
        let model = FrequencyModel::from_data(&data);
        let mut encoder = ArithmeticEncoder::new();
        encoder.encode(&data, &model);
        let encoded = encoder.finish();
        // Should compress very well for single-symbol data
        assert!(
            encoded.len() < 10,
            "Single symbol 1000x should compress to very few bytes, got {}",
            encoded.len()
        );
        let mut decoder = ArithmeticDecoder::new(encoded);
        let decoded = decoder.decode(&model, data.len());
        assert_eq!(data, decoded);
    }
}
