//! Dialogue — game dialogue compression and localization
//!
//! Provides compressed storage for game dialogue text with:
//! - Speaker dictionary (name deduplication)
//! - Ruby/furigana annotations for CJK text
//! - O(1) dialogue lookup (contiguous ID) or O(log n) (sparse ID)
//! - Delta-based localization (store only differences from base locale)
//! - Bincode + Zstd compressed wire format
//!
//! License: BSL 1.1
//! Author: Moroya Sakamoto

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Magic bytes for dialogue archive format
pub const DIALOGUE_MAGIC: &[u8; 8] = b"ALICEDLG";

/// Dialogue format version
pub const DIALOGUE_VERSION: (u8, u8) = (1, 0);

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

// ── Locale ─────────────────────────────────────────────────────

/// Language identifier (newtype over u16)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocaleId(pub u16);

impl LocaleId {
    pub const JA: Self = Self(0);
    pub const EN: Self = Self(1);
    pub const ZH: Self = Self(2);
    pub const ZH_TW: Self = Self(3);
    pub const KO: Self = Self(4);
    pub const FR: Self = Self(5);
    pub const DE: Self = Self(6);
    pub const ES: Self = Self(7);
    pub const PT: Self = Self(8);
    pub const RU: Self = Self(9);
}

// ── Ruby Annotation ────────────────────────────────────────────

/// Ruby (furigana) annotation for CJK text
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RubyAnnotation {
    /// Start position in base text (character index, not byte)
    pub base_start: u16,
    /// Number of base characters covered
    pub base_len: u16,
    /// Ruby text (e.g., furigana reading)
    pub ruby_text: String,
}

// ── Dialogue Entry ─────────────────────────────────────────────

/// Single dialogue entry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DialogueEntry {
    /// Unique dialogue ID
    pub id: u32,
    /// Speaker index in SpeakerDictionary
    pub speaker: u16,
    /// Dialogue text
    pub text: String,
    /// Optional ruby annotations
    pub ruby: Option<Vec<RubyAnnotation>>,
}

// ── Speaker Dictionary ─────────────────────────────────────────

/// Speaker name dictionary with deduplication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerDictionary {
    names: Vec<String>,
    #[serde(skip)]
    index: HashMap<String, u16>,
}

impl SpeakerDictionary {
    pub fn new() -> Self {
        Self {
            names: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Insert a speaker name, returning its index.
    /// Deduplicates: returns existing index if already present.
    pub fn insert(&mut self, name: &str) -> u16 {
        if let Some(&idx) = self.index.get(name) {
            return idx;
        }
        let idx = self.names.len() as u16;
        self.names.push(name.to_string());
        self.index.insert(name.to_string(), idx);
        idx
    }

    /// Look up speaker name by index
    pub fn get(&self, idx: u16) -> Option<&str> {
        self.names.get(idx as usize).map(|s| s.as_str())
    }

    /// Number of unique speakers
    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Rebuild the index after deserialization
    pub fn rebuild_index(&mut self) {
        self.index.clear();
        for (i, name) in self.names.iter().enumerate() {
            self.index.insert(name.clone(), i as u16);
        }
    }
}

impl Default for SpeakerDictionary {
    fn default() -> Self {
        Self::new()
    }
}

// ── Dialogue Table ─────────────────────────────────────────────

/// Dialogue table with O(1) or O(log n) lookup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueTable {
    /// Speaker dictionary
    pub speakers: SpeakerDictionary,
    /// Dialogue entries (sorted by id)
    entries: Vec<DialogueEntry>,
    /// True if IDs are contiguous starting from 0 (enables O(1) direct index)
    contiguous: bool,
    /// Content hash
    pub content_hash: u64,
}

impl DialogueTable {
    pub fn new() -> Self {
        Self {
            speakers: SpeakerDictionary::new(),
            entries: Vec::new(),
            contiguous: true,
            content_hash: 0,
        }
    }

    /// Add a dialogue entry
    pub fn add(&mut self, entry: DialogueEntry) {
        // Check contiguity
        let expected_id = self.entries.len() as u32;
        if entry.id != expected_id {
            self.contiguous = false;
        }
        self.entries.push(entry);
        self.update_hash();
    }

    /// Look up dialogue by ID.
    /// O(1) if contiguous, O(log n) if sparse.
    pub fn get(&self, id: u32) -> Option<&DialogueEntry> {
        if self.contiguous {
            self.entries.get(id as usize)
        } else {
            self.entries
                .binary_search_by_key(&id, |e| e.id)
                .ok()
                .map(|idx| &self.entries[idx])
        }
    }

    /// Number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether IDs are contiguous (O(1) lookup)
    pub fn is_contiguous(&self) -> bool {
        self.contiguous
    }

    /// Collect all unique characters across all dialogue text.
    /// Useful for font atlas preloading.
    pub fn unique_chars(&self) -> Vec<char> {
        let mut chars: Vec<char> = self.entries.iter().flat_map(|e| e.text.chars()).collect();
        chars.sort_unstable();
        chars.dedup();
        chars
    }

    /// Iterator over entries
    pub fn iter(&self) -> impl Iterator<Item = &DialogueEntry> {
        self.entries.iter()
    }

    fn update_hash(&mut self) {
        let mut buf = Vec::new();
        for entry in &self.entries {
            buf.extend_from_slice(&entry.id.to_le_bytes());
            buf.extend_from_slice(entry.text.as_bytes());
        }
        self.content_hash = fnv1a(&buf);
    }
}

impl Default for DialogueTable {
    fn default() -> Self {
        Self::new()
    }
}

// ── Delta Table (Locale Variant) ───────────────────────────────

/// Delta table storing only entries that differ from the base locale
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaTable {
    /// Target locale
    pub locale: LocaleId,
    /// Overridden entries (keyed by dialogue ID)
    pub entries: HashMap<u32, DialogueEntry>,
    /// Content hash
    pub content_hash: u64,
}

impl DeltaTable {
    pub fn new(locale: LocaleId) -> Self {
        Self {
            locale,
            entries: HashMap::new(),
            content_hash: 0,
        }
    }

    /// Add an overriding entry for a specific dialogue ID
    pub fn add(&mut self, entry: DialogueEntry) {
        self.entries.insert(entry.id, entry);
        self.update_hash();
    }

    /// Look up an overriding entry
    pub fn get(&self, id: u32) -> Option<&DialogueEntry> {
        self.entries.get(&id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn update_hash(&mut self) {
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.locale.0.to_le_bytes());
        let mut ids: Vec<u32> = self.entries.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            if let Some(e) = self.entries.get(&id) {
                buf.extend_from_slice(&e.id.to_le_bytes());
                buf.extend_from_slice(e.text.as_bytes());
            }
        }
        self.content_hash = fnv1a(&buf);
    }
}

// ── Localization Table ─────────────────────────────────────────

/// Multi-locale dialogue table with delta compression
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalizationTable {
    /// Base locale identifier
    pub base_locale: LocaleId,
    /// Base dialogue table
    pub base_table: DialogueTable,
    /// Per-locale delta tables
    pub locale_deltas: HashMap<LocaleId, DeltaTable>,
}

impl LocalizationTable {
    pub fn new(base_locale: LocaleId) -> Self {
        Self {
            base_locale,
            base_table: DialogueTable::new(),
            locale_deltas: HashMap::new(),
        }
    }

    /// Get dialogue entry for a specific locale.
    /// Falls back to base locale if not overridden.
    pub fn get(&self, locale: LocaleId, id: u32) -> Option<&DialogueEntry> {
        // Check delta first
        if locale != self.base_locale {
            if let Some(delta) = self.locale_deltas.get(&locale) {
                if let Some(entry) = delta.get(id) {
                    return Some(entry);
                }
            }
        }
        // Fall back to base
        self.base_table.get(id)
    }

    /// Add a delta entry for a specific locale
    pub fn add_delta(&mut self, locale: LocaleId, entry: DialogueEntry) {
        self.locale_deltas
            .entry(locale)
            .or_insert_with(|| DeltaTable::new(locale))
            .add(entry);
    }

    /// Collect all unique characters across all locales.
    pub fn all_unique_chars(&self) -> Vec<char> {
        let mut chars: Vec<char> = self.base_table.unique_chars();
        for delta in self.locale_deltas.values() {
            for entry in delta.entries.values() {
                chars.extend(entry.text.chars());
            }
        }
        chars.sort_unstable();
        chars.dedup();
        chars
    }

    /// List available locales
    pub fn available_locales(&self) -> Vec<LocaleId> {
        let mut locales = vec![self.base_locale];
        locales.extend(self.locale_deltas.keys());
        locales
    }
}

// ── Compression ────────────────────────────────────────────────

/// Compression mode for dialogue data
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogueCompressionMode {
    /// Fast compression (Zstd level 1)
    Fast,
    /// Balanced (Zstd level 3)
    Balanced,
    /// Maximum compression (Zstd level 19)
    Maximum,
}

impl DialogueCompressionMode {
    fn zstd_level(self) -> i32 {
        match self {
            Self::Fast => 1,
            Self::Balanced => 3,
            Self::Maximum => 19,
        }
    }
}

/// Dialogue compressor (Bincode + Zstd)
pub struct DialogueCompressor {
    mode: DialogueCompressionMode,
}

impl DialogueCompressor {
    pub fn new(mode: DialogueCompressionMode) -> Self {
        Self { mode }
    }

    /// Compress a dialogue table to bytes
    pub fn compress_table(&self, table: &DialogueTable) -> crate::Result<Vec<u8>> {
        let serialized = bincode::serialize(table)
            .map_err(|e| crate::ALICETextError::EncodingError(e.to_string()))?;
        let mut output = Vec::with_capacity(serialized.len() + 12);
        output.extend_from_slice(DIALOGUE_MAGIC);
        output.push(DIALOGUE_VERSION.0);
        output.push(DIALOGUE_VERSION.1);
        output.push(0x01); // type: single table
        output.push(self.mode as u8);
        let compressed = zstd::encode_all(serialized.as_slice(), self.mode.zstd_level())
            .map_err(|e| crate::ALICETextError::EncodingError(e.to_string()))?;
        output.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        output.extend_from_slice(&compressed);
        Ok(output)
    }

    /// Decompress a dialogue table from bytes
    pub fn decompress_table(&self, data: &[u8]) -> crate::Result<DialogueTable> {
        if data.len() < 16 {
            return Err(crate::ALICETextError::DecompressionError(
                "Data too short".to_string(),
            ));
        }
        if &data[0..8] != DIALOGUE_MAGIC {
            return Err(crate::ALICETextError::InvalidMagic);
        }
        if data[10] != 0x01 {
            return Err(crate::ALICETextError::DecompressionError(
                "Not a single dialogue table".to_string(),
            ));
        }
        let compressed_len = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
        if data.len() < 16 + compressed_len {
            return Err(crate::ALICETextError::DecompressionError(
                "Truncated data".to_string(),
            ));
        }
        let decompressed = zstd::decode_all(&data[16..16 + compressed_len])
            .map_err(|e| crate::ALICETextError::DecompressionError(e.to_string()))?;
        let mut table: DialogueTable = bincode::deserialize(&decompressed)
            .map_err(|e| crate::ALICETextError::DecompressionError(e.to_string()))?;
        table.speakers.rebuild_index();
        Ok(table)
    }

    /// Compress a localization table (multi-locale) to bytes
    pub fn compress_localization(&self, table: &LocalizationTable) -> crate::Result<Vec<u8>> {
        let serialized = bincode::serialize(table)
            .map_err(|e| crate::ALICETextError::EncodingError(e.to_string()))?;
        let mut output = Vec::with_capacity(serialized.len() + 12);
        output.extend_from_slice(DIALOGUE_MAGIC);
        output.push(DIALOGUE_VERSION.0);
        output.push(DIALOGUE_VERSION.1);
        output.push(0x02); // type: localization table
        output.push(self.mode as u8);
        let compressed = zstd::encode_all(serialized.as_slice(), self.mode.zstd_level())
            .map_err(|e| crate::ALICETextError::EncodingError(e.to_string()))?;
        output.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        output.extend_from_slice(&compressed);
        Ok(output)
    }

    /// Decompress a localization table from bytes
    pub fn decompress_localization(&self, data: &[u8]) -> crate::Result<LocalizationTable> {
        if data.len() < 16 {
            return Err(crate::ALICETextError::DecompressionError(
                "Data too short".to_string(),
            ));
        }
        if &data[0..8] != DIALOGUE_MAGIC {
            return Err(crate::ALICETextError::InvalidMagic);
        }
        if data[10] != 0x02 {
            return Err(crate::ALICETextError::DecompressionError(
                "Not a localization table".to_string(),
            ));
        }
        let compressed_len = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
        if data.len() < 16 + compressed_len {
            return Err(crate::ALICETextError::DecompressionError(
                "Truncated data".to_string(),
            ));
        }
        let decompressed = zstd::decode_all(&data[16..16 + compressed_len])
            .map_err(|e| crate::ALICETextError::DecompressionError(e.to_string()))?;
        let mut table: LocalizationTable = bincode::deserialize(&decompressed)
            .map_err(|e| crate::ALICETextError::DecompressionError(e.to_string()))?;
        table.base_table.speakers.rebuild_index();
        Ok(table)
    }
}

impl Default for DialogueCompressor {
    fn default() -> Self {
        Self::new(DialogueCompressionMode::Balanced)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: u32, speaker: u16, text: &str) -> DialogueEntry {
        DialogueEntry {
            id,
            speaker,
            text: text.to_string(),
            ruby: None,
        }
    }

    #[test]
    fn test_speaker_dictionary_insert_dedup() {
        let mut dict = SpeakerDictionary::new();
        let i1 = dict.insert("Alice");
        let i2 = dict.insert("Bob");
        let i3 = dict.insert("Alice"); // duplicate
        assert_eq!(i1, 0);
        assert_eq!(i2, 1);
        assert_eq!(i3, 0); // same as first
        assert_eq!(dict.len(), 2);
    }

    #[test]
    fn test_speaker_dictionary_get() {
        let mut dict = SpeakerDictionary::new();
        dict.insert("Alice");
        dict.insert("Bob");
        assert_eq!(dict.get(0), Some("Alice"));
        assert_eq!(dict.get(1), Some("Bob"));
        assert_eq!(dict.get(2), None);
    }

    #[test]
    fn test_ruby_annotation() {
        let ruby = RubyAnnotation {
            base_start: 0,
            base_len: 2,
            ruby_text: "とうきょう".to_string(),
        };
        assert_eq!(ruby.base_start, 0);
        assert_eq!(ruby.ruby_text, "とうきょう");
    }

    #[test]
    fn test_dialogue_table_contiguous_lookup() {
        let mut table = DialogueTable::new();
        table.add(make_entry(0, 0, "Hello"));
        table.add(make_entry(1, 0, "World"));
        table.add(make_entry(2, 1, "Goodbye"));
        assert!(table.is_contiguous());
        assert_eq!(table.get(0).unwrap().text, "Hello");
        assert_eq!(table.get(2).unwrap().text, "Goodbye");
        assert!(table.get(3).is_none());
    }

    #[test]
    fn test_dialogue_table_sparse_lookup() {
        let mut table = DialogueTable::new();
        table.add(make_entry(0, 0, "First"));
        table.add(make_entry(5, 0, "Sparse")); // non-contiguous
        assert!(!table.is_contiguous());
        // Sparse lookup via binary search
        let found = table.get(5);
        assert!(found.is_some());
        assert_eq!(found.unwrap().text, "Sparse");
    }

    #[test]
    fn test_unique_chars() {
        let mut table = DialogueTable::new();
        table.add(make_entry(0, 0, "aab"));
        table.add(make_entry(1, 0, "bcc"));
        let chars = table.unique_chars();
        assert_eq!(chars, vec!['a', 'b', 'c']);
    }

    #[test]
    fn test_hash_determinism() {
        let mut t1 = DialogueTable::new();
        t1.add(make_entry(0, 0, "Hello"));
        let mut t2 = DialogueTable::new();
        t2.add(make_entry(0, 0, "Hello"));
        assert_eq!(t1.content_hash, t2.content_hash);
        assert_ne!(t1.content_hash, 0);
    }

    #[test]
    fn test_delta_table() {
        let mut delta = DeltaTable::new(LocaleId::EN);
        delta.add(make_entry(0, 0, "Hello"));
        delta.add(make_entry(5, 1, "Bye"));
        assert_eq!(delta.len(), 2);
        assert_eq!(delta.get(0).unwrap().text, "Hello");
        assert_eq!(delta.get(5).unwrap().text, "Bye");
        assert!(delta.get(1).is_none());
    }

    #[test]
    fn test_localization_fallback() {
        let mut loc = LocalizationTable::new(LocaleId::JA);
        loc.base_table.add(make_entry(0, 0, "こんにちは"));
        loc.base_table.add(make_entry(1, 0, "さようなら"));
        // Override only entry 0 for EN
        loc.add_delta(LocaleId::EN, make_entry(0, 0, "Hello"));
        // EN entry 0 → delta
        assert_eq!(loc.get(LocaleId::EN, 0).unwrap().text, "Hello");
        // EN entry 1 → fallback to JA base
        assert_eq!(loc.get(LocaleId::EN, 1).unwrap().text, "さようなら");
        // JA entry 0 → base
        assert_eq!(loc.get(LocaleId::JA, 0).unwrap().text, "こんにちは");
    }

    #[test]
    fn test_all_unique_chars_across_locales() {
        let mut loc = LocalizationTable::new(LocaleId::JA);
        loc.base_table.add(make_entry(0, 0, "abc"));
        loc.add_delta(LocaleId::EN, make_entry(0, 0, "xyz"));
        let chars = loc.all_unique_chars();
        assert!(chars.contains(&'a'));
        assert!(chars.contains(&'x'));
    }

    #[test]
    fn test_compress_decompress_table_roundtrip() {
        let mut table = DialogueTable::new();
        let speaker = table.speakers.insert("Alice");
        table.add(DialogueEntry {
            id: 0,
            speaker,
            text: "Hello, World!".to_string(),
            ruby: Some(vec![RubyAnnotation {
                base_start: 0,
                base_len: 5,
                ruby_text: "test".to_string(),
            }]),
        });
        table.add(make_entry(1, speaker, "Goodbye"));

        let compressor = DialogueCompressor::default();
        let compressed = compressor.compress_table(&table).unwrap();
        assert!(compressed.len() > 0);
        assert_eq!(&compressed[0..8], DIALOGUE_MAGIC);

        let decompressed = compressor.decompress_table(&compressed).unwrap();
        assert_eq!(decompressed.len(), 2);
        assert_eq!(decompressed.get(0).unwrap().text, "Hello, World!");
        assert_eq!(decompressed.get(1).unwrap().text, "Goodbye");
        assert_eq!(decompressed.speakers.get(0), Some("Alice"));
    }

    #[test]
    fn test_compress_decompress_localization_roundtrip() {
        let mut loc = LocalizationTable::new(LocaleId::JA);
        let speaker = loc.base_table.speakers.insert("NPC");
        loc.base_table.add(make_entry(0, speaker, "こんにちは"));
        loc.add_delta(LocaleId::EN, make_entry(0, 0, "Hello"));

        let compressor = DialogueCompressor::default();
        let compressed = compressor.compress_localization(&loc).unwrap();
        let decompressed = compressor.decompress_localization(&compressed).unwrap();
        assert_eq!(
            decompressed.get(LocaleId::JA, 0).unwrap().text,
            "こんにちは"
        );
        assert_eq!(decompressed.get(LocaleId::EN, 0).unwrap().text, "Hello");
    }
}
