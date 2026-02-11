//! ALICE-Text × ALICE-Search bridge
//!
//! FM-Index search on compressed text — O(|pattern|) search without full decompression.
//!
//! Author: Moroya Sakamoto

use alice_search::FmIndex;
use crate::ALICEText;

/// Search hit with offset and context range
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub offset: usize,
    pub context_start: usize,
    pub context_end: usize,
}

/// FM-Index built from ALICE-Text compressed data
pub struct CompressedSearchIndex {
    index: FmIndex,
    text_len: usize,
}

impl CompressedSearchIndex {
    /// Build FM-Index from compressed ALICE-Text data.
    /// Decompresses once to build the index, then search is O(|pattern|).
    pub fn build_from_compressed(compressed: &[u8]) -> Result<Self, String> {
        let decompressor = ALICEText::new();
        let text = decompressor
            .decompress(compressed)
            .map_err(|e| format!("Decompress error: {}", e))?;
        let text_len = text.len();
        let index = FmIndex::build(text.as_bytes());
        Ok(Self { index, text_len })
    }

    /// Build FM-Index from raw text
    pub fn build_from_text(text: &str) -> Self {
        let text_len = text.len();
        let index = FmIndex::build(text.as_bytes());
        Self { index, text_len }
    }

    /// Count occurrences of a pattern (O(|pattern|) via backward search)
    pub fn search_count(&self, query: &str) -> usize {
        self.index.count(query.as_bytes())
    }

    /// Find all occurrences with context windows
    pub fn search(&self, query: &str, context_radius: usize) -> Vec<SearchHit> {
        let offsets = self.index.locate(query.as_bytes());
        offsets
            .into_iter()
            .map(|offset| {
                let start = offset.saturating_sub(context_radius);
                let end = (offset + query.len() + context_radius).min(self.text_len);
                SearchHit { offset, context_start: start, context_end: end }
            })
            .collect()
    }

    pub fn text_len(&self) -> usize {
        self.text_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_and_search() {
        let text = "ERROR: connection failed\nINFO: retry success\nERROR: timeout";
        let index = CompressedSearchIndex::build_from_text(text);
        assert_eq!(index.search_count("ERROR"), 2);
        assert_eq!(index.search_count("INFO"), 1);
        assert_eq!(index.search_count("MISSING"), 0);
    }

    #[test]
    fn test_search_with_context() {
        let text = "the quick brown fox jumps over the lazy dog";
        let index = CompressedSearchIndex::build_from_text(text);
        let hits = index.search("fox", 5);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].context_start < hits[0].offset);
    }

    #[test]
    fn test_empty_text() {
        let index = CompressedSearchIndex::build_from_text("");
        assert_eq!(index.search_count("anything"), 0);
        assert_eq!(index.text_len(), 0);
    }
}
