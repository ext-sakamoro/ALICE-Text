//! Pattern learning module for ALICE-Text
//!
//! Automatically extracts and learns patterns from text/logs.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashMap;

/// Types of patterns that can be detected
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PatternType {
    /// ISO timestamp (2024-01-15 10:30:45)
    Timestamp,
    /// Date only (2024-01-15)
    Date,
    /// Time only (10:30:45)
    Time,
    /// IPv4 address (192.168.1.100)
    IPv4,
    /// IPv6 address
    IPv6,
    /// UUID (550e8400-e29b-41d4-a716-446655440000)
    UUID,
    /// Log level (INFO, WARN, ERROR, DEBUG)
    LogLevel,
    /// File path (/var/log/app.log)
    Path,
    /// URL (https://example.com)
    URL,
    /// Numeric value
    Number,
    /// Hexadecimal value
    Hex,
    /// Email address
    Email,
    /// Custom pattern
    Custom,
}

impl PatternType {
    /// Get the regex pattern for this type
    pub fn regex_pattern(&self) -> &'static str {
        match self {
            PatternType::Timestamp => r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?",
            PatternType::Date => r"\b\d{4}-\d{2}-\d{2}\b",
            PatternType::Time => r"\b\d{2}:\d{2}:\d{2}(?:\.\d+)?\b",
            PatternType::IPv4 => r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b",
            PatternType::IPv6 => r"\b(?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}\b",
            PatternType::UUID => r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b",
            PatternType::LogLevel => r"\b(?:DEBUG|INFO|WARN(?:ING)?|ERROR|FATAL|TRACE|CRITICAL)\b",
            PatternType::Path => r"(?:/[a-zA-Z0-9._-]+)+/?",
            PatternType::URL => r#"https?://[^\s<>"']+"#,
            PatternType::Number => r"\b\d+(?:\.\d+)?\b",
            PatternType::Hex => r"\b0x[0-9a-fA-F]+\b",
            PatternType::Email => r"\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}\b",
            PatternType::Custom => r".*",
        }
    }

    /// Get priority for pattern matching (higher = matched first)
    pub fn priority(&self) -> u8 {
        match self {
            PatternType::Timestamp => 100,
            PatternType::UUID => 95,
            PatternType::Email => 90,
            PatternType::URL => 85,
            PatternType::IPv6 => 80,
            PatternType::IPv4 => 75,
            PatternType::Date => 70,
            PatternType::Time => 65,
            PatternType::Path => 60,
            PatternType::Hex => 55,
            PatternType::LogLevel => 50,
            PatternType::Number => 40,
            PatternType::Custom => 10,
        }
    }
}

/// A match found in text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternMatch {
    /// Type of pattern matched
    pub pattern_type: PatternType,
    /// Start position in text
    pub start: usize,
    /// End position in text
    pub end: usize,
    /// Matched text
    pub matched_text: String,
    /// Pattern index in database
    pub pattern_index: usize,
}

/// A learned pattern with statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnedPattern {
    /// Pattern type
    pub pattern_type: PatternType,
    /// Regex pattern string
    pub pattern: String,
    /// Number of occurrences
    pub count: usize,
    /// Example values
    pub examples: Vec<String>,
}

impl LearnedPattern {
    pub fn new(pattern_type: PatternType) -> Self {
        Self {
            pattern_type,
            pattern: pattern_type.regex_pattern().to_string(),
            count: 0,
            examples: Vec::new(),
        }
    }

    pub fn add_example(&mut self, example: &str) {
        self.count += 1;
        if self.examples.len() < 5 && !self.examples.contains(&example.to_string()) {
            self.examples.push(example.to_string());
        }
    }
}

/// Database of learned patterns
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PatternDatabase {
    /// Learned patterns by type
    pub patterns: HashMap<PatternType, LearnedPattern>,
    /// Custom patterns
    pub custom_patterns: Vec<LearnedPattern>,
    /// Total matches found
    pub total_matches: usize,
}

impl PatternDatabase {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a pattern match to the database
    pub fn add_match(&mut self, pattern_type: PatternType, matched_text: &str) {
        self.total_matches += 1;
        self.patterns
            .entry(pattern_type)
            .or_insert_with(|| LearnedPattern::new(pattern_type))
            .add_example(matched_text);
    }

    /// Get the most common pattern types
    pub fn top_patterns(&self, n: usize) -> Vec<(&PatternType, &LearnedPattern)> {
        let mut patterns: Vec<_> = self.patterns.iter().collect();
        patterns.sort_by(|a, b| b.1.count.cmp(&a.1.count));
        patterns.into_iter().take(n).collect()
    }

    /// Serialize to JSON bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserialize from JSON bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }
}

/// Pattern learner for extracting patterns from text
pub struct PatternLearner {
    /// Compiled regex patterns
    patterns: Vec<(PatternType, Regex)>,
}

impl PatternLearner {
    /// Create a new pattern learner with default patterns
    pub fn new() -> Self {
        let mut pattern_types: Vec<PatternType> = vec![
            PatternType::Timestamp,
            PatternType::UUID,
            PatternType::Email,
            PatternType::URL,
            PatternType::IPv6,
            PatternType::IPv4,
            PatternType::Date,
            PatternType::Time,
            PatternType::Path,
            PatternType::Hex,
            PatternType::LogLevel,
            PatternType::Number,
        ];

        // Sort by priority (highest first)
        pattern_types.sort_by_key(|b| Reverse(b.priority()));

        let patterns = pattern_types
            .into_iter()
            .filter_map(|pt| {
                Regex::new(pt.regex_pattern())
                    .ok()
                    .map(|re| (pt, re))
            })
            .collect();

        Self { patterns }
    }

    /// Learn patterns from text
    pub fn learn(&self, text: &str) -> PatternDatabase {
        let mut db = PatternDatabase::new();
        let mut covered: Vec<bool> = vec![false; text.len()];

        // Find all pattern matches, prioritizing by pattern type
        for (pattern_type, regex) in &self.patterns {
            for mat in regex.find_iter(text) {
                let start = mat.start();
                let end = mat.end();

                // Check if this region is already covered
                if covered[start..end].iter().any(|&c| c) {
                    continue;
                }

                // Mark region as covered
                covered[start..end].iter_mut().for_each(|c| *c = true);

                db.add_match(*pattern_type, mat.as_str());
            }
        }

        db
    }

    /// Find all pattern matches in text
    pub fn find_matches(&self, text: &str) -> Vec<PatternMatch> {
        let mut matches = Vec::new();
        let mut covered: Vec<bool> = vec![false; text.len()];
        let mut pattern_index = 0;

        for (pattern_type, regex) in &self.patterns {
            for mat in regex.find_iter(text) {
                let start = mat.start();
                let end = mat.end();

                // Check if this region is already covered
                if covered[start..end].iter().any(|&c| c) {
                    continue;
                }

                // Mark region as covered
                covered[start..end].iter_mut().for_each(|c| *c = true);

                matches.push(PatternMatch {
                    pattern_type: *pattern_type,
                    start,
                    end,
                    matched_text: mat.as_str().to_string(),
                    pattern_index,
                });
                pattern_index += 1;
            }
        }

        // Sort by position
        matches.sort_by_key(|m| m.start);
        matches
    }

    /// Replace patterns with placeholders
    pub fn replace_patterns(&self, text: &str) -> (String, Vec<PatternMatch>) {
        let matches = self.find_matches(text);
        let mut result = String::with_capacity(text.len());
        let mut last_end = 0;

        for (i, mat) in matches.iter().enumerate() {
            // Add text before this match
            result.push_str(&text[last_end..mat.start]);
            // Add placeholder
            result.push_str(&format!("{{P{}}}", i));
            last_end = mat.end;
        }

        // Add remaining text
        result.push_str(&text[last_end..]);

        (result, matches)
    }

    /// Restore patterns from placeholders
    pub fn restore_patterns(&self, text: &str, matches: &[PatternMatch]) -> String {
        let mut result = text.to_string();

        // Replace in reverse order to maintain positions
        for (i, mat) in matches.iter().enumerate().rev() {
            let placeholder = format!("{{P{}}}", i);
            result = result.replace(&placeholder, &mat.matched_text);
        }

        result
    }
}

impl Default for PatternLearner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_detection() {
        let learner = PatternLearner::new();
        let text = "2024-01-15 10:30:45 INFO User logged in from 192.168.1.100";

        let matches = learner.find_matches(text);

        assert!(!matches.is_empty());

        let types: Vec<_> = matches.iter().map(|m| m.pattern_type).collect();
        assert!(types.contains(&PatternType::Timestamp) || types.contains(&PatternType::Date));
        assert!(types.contains(&PatternType::IPv4));
        assert!(types.contains(&PatternType::LogLevel));
    }

    #[test]
    fn test_pattern_replacement() {
        let learner = PatternLearner::new();
        let text = "IP: 192.168.1.100 at 10:30:45";

        let (replaced, matches) = learner.replace_patterns(text);
        let restored = learner.restore_patterns(&replaced, &matches);

        assert_eq!(text, restored);
    }

    #[test]
    fn test_pattern_learning() {
        let learner = PatternLearner::new();
        let text = "2024-01-15 INFO test\n2024-01-16 WARN test\n2024-01-17 ERROR test";

        let db = learner.learn(text);

        assert!(db.patterns.contains_key(&PatternType::Date));
        assert!(db.patterns.contains_key(&PatternType::LogLevel));
    }

    #[test]
    fn test_uuid_detection() {
        let learner = PatternLearner::new();
        let text = "Request ID: 550e8400-e29b-41d4-a716-446655440000";

        let matches = learner.find_matches(text);
        let types: Vec<_> = matches.iter().map(|m| m.pattern_type).collect();

        assert!(types.contains(&PatternType::UUID));
    }

    #[test]
    fn test_url_detection() {
        let learner = PatternLearner::new();
        let text = "Visit https://example.com/path?query=1";

        let matches = learner.find_matches(text);
        let types: Vec<_> = matches.iter().map(|m| m.pattern_type).collect();

        assert!(types.contains(&PatternType::URL));
    }
}
