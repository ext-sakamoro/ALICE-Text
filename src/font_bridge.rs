//! Font bridge — ALICE-Text ↔ ALICE-Font pipeline connection
//!
//! Feature-gated: `#[cfg(feature = "font")]`
//!
//! Connects compressed text data from ALICE-Text to the parametric
//! font renderer in ALICE-Font. Provides:
//! - Character set extraction from compressed text
//! - Text shaping with ALICE-Font parameters
//! - Dialogue table charset extraction for atlas preloading
//!
//! License: BSL 1.1
//! Author: Moroya Sakamoto

use alice_font::atlas::SdfAtlas;
use alice_font::param::MetaFontParams;
use alice_font::shaper::{ShapedLine, TextShaper};

use crate::dialogue::{DialogueTable, LocalizationTable};

// ── FNV-1a (file-local) ───────────────────────────────────────
#[inline(always)]
fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ── Character Set ──────────────────────────────────────────────

/// Sorted, deduplicated character set extracted from text data
#[derive(Debug, Clone)]
pub struct CharacterSet {
    /// Sorted unique characters
    pub chars: Vec<char>,
    /// Content hash
    pub content_hash: u64,
}

impl CharacterSet {
    /// Create from raw text
    pub fn from_text(text: &str) -> Self {
        let mut chars: Vec<char> = text.chars().collect();
        chars.sort_unstable();
        chars.dedup();
        let hash = fnv1a(text.as_bytes());
        Self {
            chars,
            content_hash: hash,
        }
    }

    /// Create from compressed ALICE-Text data (decompress first)
    pub fn from_compressed(data: &[u8]) -> crate::Result<Self> {
        let alice = crate::ALICEText::default();
        let text = alice.decompress(data)?;
        Ok(Self::from_text(&text))
    }

    /// Check if a character is in the set (binary search, O(log n))
    pub fn contains(&self, ch: char) -> bool {
        self.chars.binary_search(&ch).is_ok()
    }

    /// Number of unique characters
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// Preload characters into an SDF atlas
    pub fn preload_atlas(&self, atlas: &mut SdfAtlas) {
        atlas.preload(&self.chars);
    }

    /// Merge with another character set
    pub fn merge(&self, other: &Self) -> Self {
        let mut chars = self.chars.clone();
        chars.extend_from_slice(&other.chars);
        chars.sort_unstable();
        chars.dedup();
        let mut buf = Vec::new();
        for ch in &chars {
            let mut b = [0u8; 4];
            let s = ch.encode_utf8(&mut b);
            buf.extend_from_slice(s.as_bytes());
        }
        Self {
            chars,
            content_hash: fnv1a(&buf),
        }
    }
}

// ── Shaping Results ────────────────────────────────────────────

/// Text shaping result from the font pipeline
#[derive(Debug, Clone)]
pub struct ShapedTextResult {
    /// Shaped lines
    pub lines: Vec<ShapedLine>,
    /// Total bounding width
    pub total_width: f32,
    /// Total bounding height
    pub total_height: f32,
    /// Total glyph count
    pub glyph_count: usize,
    /// Content hash
    pub content_hash: u64,
}

// ── Pipeline Configuration ─────────────────────────────────────

/// Font rendering pipeline configuration
#[derive(Debug, Clone)]
pub struct FontPipelineConfig {
    /// Font parameters
    pub font_params: MetaFontParams,
    /// Maximum line width for wrapping (0 = no wrap)
    pub max_line_width: f32,
    /// Atlas grid dimension
    pub atlas_dim: usize,
    /// Additional letter spacing (em units)
    pub letter_spacing: f32,
    /// Line height multiplier
    pub line_height: f32,
}

impl Default for FontPipelineConfig {
    fn default() -> Self {
        Self {
            font_params: MetaFontParams::sans_regular(),
            max_line_width: 0.0,
            atlas_dim: 8,
            letter_spacing: 0.0,
            line_height: 1.2,
        }
    }
}

// ── Pipeline Functions ─────────────────────────────────────────

/// Extract character set from compressed ALICE-Text data
pub fn extract_charset_from_compressed(data: &[u8]) -> crate::Result<CharacterSet> {
    CharacterSet::from_compressed(data)
}

/// Shape compressed text using the font pipeline
pub fn shape_compressed_text(
    data: &[u8],
    config: &FontPipelineConfig,
) -> crate::Result<ShapedTextResult> {
    let alice = crate::ALICEText::default();
    let text = alice.decompress(data)?;
    Ok(shape_text_with_config(&text, config))
}

/// Shape plain text using the font pipeline configuration
pub fn shape_text_with_config(text: &str, config: &FontPipelineConfig) -> ShapedTextResult {
    let mut atlas = SdfAtlas::new(config.atlas_dim, config.font_params);
    let mut shaper = TextShaper::new(config.font_params);
    shaper.set_letter_spacing(config.letter_spacing);
    shaper.set_line_height(config.line_height);

    let lines = shaper.shape_text(text, &mut atlas, config.max_line_width);

    let mut total_width: f32 = 0.0;
    let mut glyph_count = 0;
    for line in &lines {
        if line.width > total_width {
            total_width = line.width;
        }
        glyph_count += line.glyphs.len();
    }

    let line_step =
        config.line_height * (config.font_params.ascender + config.font_params.descender);
    let total_height = lines.len() as f32 * line_step;

    let content_hash = fnv1a(text.as_bytes());

    ShapedTextResult {
        lines,
        total_width,
        total_height,
        glyph_count,
        content_hash,
    }
}

/// Extract character set from a dialogue table (for atlas preloading)
pub fn dialogue_charset(table: &DialogueTable) -> CharacterSet {
    let chars = table.unique_chars();
    let mut buf = Vec::new();
    for ch in &chars {
        let mut b = [0u8; 4];
        let s = ch.encode_utf8(&mut b);
        buf.extend_from_slice(s.as_bytes());
    }
    CharacterSet {
        chars,
        content_hash: fnv1a(&buf),
    }
}

/// Extract character set from all locales in a localization table
pub fn localization_charset(table: &LocalizationTable) -> CharacterSet {
    let chars = table.all_unique_chars();
    let mut buf = Vec::new();
    for ch in &chars {
        let mut b = [0u8; 4];
        let s = ch.encode_utf8(&mut b);
        buf.extend_from_slice(s.as_bytes());
    }
    CharacterSet {
        chars,
        content_hash: fnv1a(&buf),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue::{DialogueEntry, LocaleId};

    #[test]
    fn test_charset_from_text() {
        let cs = CharacterSet::from_text("hello world");
        assert!(cs.contains('h'));
        assert!(cs.contains(' '));
        assert!(!cs.contains('z'));
    }

    #[test]
    fn test_charset_dedup() {
        let cs = CharacterSet::from_text("aabbcc");
        assert_eq!(cs.len(), 3);
        assert_eq!(cs.chars, vec!['a', 'b', 'c']);
    }

    #[test]
    fn test_charset_binary_search() {
        let cs = CharacterSet::from_text("abcdefghijklmnop");
        assert!(cs.contains('a'));
        assert!(cs.contains('p'));
        assert!(!cs.contains('z'));
    }

    #[test]
    fn test_charset_hash_determinism() {
        let cs1 = CharacterSet::from_text("hello");
        let cs2 = CharacterSet::from_text("hello");
        assert_eq!(cs1.content_hash, cs2.content_hash);
        assert_ne!(cs1.content_hash, 0);
    }

    #[test]
    fn test_charset_merge() {
        let cs1 = CharacterSet::from_text("abc");
        let cs2 = CharacterSet::from_text("cde");
        let merged = cs1.merge(&cs2);
        assert_eq!(merged.len(), 5); // a,b,c,d,e
        assert!(merged.contains('a'));
        assert!(merged.contains('e'));
    }

    #[test]
    fn test_shape_text_basic() {
        let config = FontPipelineConfig::default();
        let result = shape_text_with_config("Hello", &config);
        assert!(result.total_width > 0.0);
        assert_eq!(result.glyph_count, 5);
        assert_eq!(result.lines.len(), 1);
    }

    #[test]
    fn test_shape_text_multiline() {
        let config = FontPipelineConfig::default();
        let result = shape_text_with_config("Hello\nWorld", &config);
        assert_eq!(result.lines.len(), 2);
        assert!(result.total_height > 0.0);
    }

    #[test]
    fn test_pipeline_config_default() {
        let config = FontPipelineConfig::default();
        assert!((config.max_line_width - 0.0).abs() < 0.001);
        assert_eq!(config.atlas_dim, 8);
        assert!((config.line_height - 1.2).abs() < 0.01);
    }

    #[test]
    fn test_dialogue_charset_extraction() {
        let mut table = crate::dialogue::DialogueTable::new();
        table.add(DialogueEntry {
            id: 0,
            speaker: 0,
            text: "abc".to_string(),
            ruby: None,
        });
        table.add(DialogueEntry {
            id: 1,
            speaker: 0,
            text: "cde".to_string(),
            ruby: None,
        });
        let cs = dialogue_charset(&table);
        assert_eq!(cs.len(), 5); // a,b,c,d,e
    }

    #[test]
    fn test_localization_charset_extraction() {
        let mut loc = LocalizationTable::new(LocaleId::JA);
        loc.base_table.add(DialogueEntry {
            id: 0,
            speaker: 0,
            text: "abc".to_string(),
            ruby: None,
        });
        loc.add_delta(
            LocaleId::EN,
            DialogueEntry {
                id: 0,
                speaker: 0,
                text: "xyz".to_string(),
                ruby: None,
            },
        );
        let cs = localization_charset(&loc);
        assert!(cs.contains('a'));
        assert!(cs.contains('x'));
        assert_eq!(cs.len(), 6); // a,b,c,x,y,z
    }
}
