//! Tuned Pattern Learner - Fused Regex Implementation
//!
//! Uses a single combined regex for O(N) pattern extraction instead of O(N×M).

use regex::Regex;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::borrow::Cow;
use std::collections::HashMap;

/// Pattern types (same as original, but optimized for u8 storage)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum PatternType {
    Timestamp = 0,
    Date = 1,
    Time = 2,
    IPv4 = 3,
    IPv6 = 4,
    UUID = 5,
    LogLevel = 6,
    Path = 7,
    URL = 8,
    Number = 9,
    Hex = 10,
    Email = 11,
    Custom = 12,
}

impl PatternType {
    /// Convert to u8 for compact storage
    #[inline]
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Convert from u8
    #[inline]
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Timestamp,
            1 => Self::Date,
            2 => Self::Time,
            3 => Self::IPv4,
            4 => Self::IPv6,
            5 => Self::UUID,
            6 => Self::LogLevel,
            7 => Self::Path,
            8 => Self::URL,
            9 => Self::Number,
            10 => Self::Hex,
            11 => Self::Email,
            _ => Self::Custom,
        }
    }
}

/// Zero-copy pattern match using Cow
#[derive(Debug, Clone)]
pub struct TunedMatch<'a> {
    pub pattern_type: PatternType,
    pub start: usize,
    pub end: usize,
    pub matched_text: Cow<'a, str>,
}

/// Owned version for serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedMatch {
    pub pattern_type: PatternType,
    pub start: usize,
    pub end: usize,
    pub matched_text: String,
}

impl<'a> From<TunedMatch<'a>> for OwnedMatch {
    fn from(m: TunedMatch<'a>) -> Self {
        Self {
            pattern_type: m.pattern_type,
            start: m.start,
            end: m.end,
            matched_text: m.matched_text.into_owned(),
        }
    }
}

/// Pattern definition with name and regex
struct PatternDef {
    name: &'static str,
    pattern: &'static str,
    pattern_type: PatternType,
}

/// All pattern definitions (ordered by priority - most specific first)
const PATTERNS: &[PatternDef] = &[
    PatternDef {
        name: "TIMESTAMP",
        pattern: r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?",
        pattern_type: PatternType::Timestamp,
    },
    PatternDef {
        name: "UUID",
        pattern: r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
        pattern_type: PatternType::UUID,
    },
    PatternDef {
        name: "EMAIL",
        pattern: r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}",
        pattern_type: PatternType::Email,
    },
    PatternDef {
        name: "URL",
        pattern: r#"https?://[^\s<>"']+"#,
        pattern_type: PatternType::URL,
    },
    PatternDef {
        name: "IPV6",
        pattern: r"(?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}",
        pattern_type: PatternType::IPv6,
    },
    PatternDef {
        name: "IPV4",
        pattern: r"(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)",
        pattern_type: PatternType::IPv4,
    },
    PatternDef {
        name: "DATE",
        pattern: r"\d{4}-\d{2}-\d{2}",
        pattern_type: PatternType::Date,
    },
    PatternDef {
        name: "TIME",
        pattern: r"\d{2}:\d{2}:\d{2}(?:\.\d+)?",
        pattern_type: PatternType::Time,
    },
    PatternDef {
        name: "PATH",
        pattern: r"(?:/[a-zA-Z0-9._-]+)+/?",
        pattern_type: PatternType::Path,
    },
    PatternDef {
        name: "HEX",
        pattern: r"0x[0-9a-fA-F]+",
        pattern_type: PatternType::Hex,
    },
    PatternDef {
        name: "LOGLEVEL",
        pattern: r"(?:DEBUG|INFO|WARN(?:ING)?|ERROR|FATAL|TRACE|CRITICAL)",
        pattern_type: PatternType::LogLevel,
    },
    PatternDef {
        name: "NUMBER",
        pattern: r"\d+(?:\.\d+)?",
        pattern_type: PatternType::Number,
    },
];

/// Tuned Pattern Learner with Fused Regex
///
/// Uses a single combined regex for O(N) pattern extraction.
pub struct TunedPatternLearner {
    /// Combined regex with named capture groups
    fused_regex: Regex,
    /// Mapping from group name to pattern type
    group_map: HashMap<&'static str, PatternType>,
    /// Group names in order (for iteration)
    group_names: Vec<&'static str>,
}

impl TunedPatternLearner {
    /// Create a new tuned pattern learner
    pub fn new() -> Self {
        // Build fused regex: (?P<TIMESTAMP>...)|(?P<UUID>...)|...
        let expr = PATTERNS
            .iter()
            .map(|p| format!("(?P<{}>{})", p.name, p.pattern))
            .collect::<Vec<_>>()
            .join("|");

        let fused_regex = Regex::new(&expr).expect("Invalid fused regex");

        let mut group_map = HashMap::new();
        let mut group_names = Vec::new();

        for p in PATTERNS {
            group_map.insert(p.name, p.pattern_type);
            group_names.push(p.name);
        }

        Self {
            fused_regex,
            group_map,
            group_names,
        }
    }

    /// Find all matches in text with zero-copy (single pass)
    ///
    /// Returns matches in a SmallVec to avoid heap allocation for small match counts.
    pub fn find_matches<'a>(&self, text: &'a str) -> SmallVec<[TunedMatch<'a>; 32]> {
        let mut matches = SmallVec::new();
        let mut covered = vec![false; text.len()];

        for caps in self.fused_regex.captures_iter(text) {
            // Find which named group matched
            for &name in &self.group_names {
                if let Some(m) = caps.name(name) {
                    let start = m.start();
                    let end = m.end();

                    // Skip if region already covered by higher-priority pattern
                    if covered[start..end].iter().any(|&c| c) {
                        continue;
                    }

                    // Mark as covered
                    for c in &mut covered[start..end] {
                        *c = true;
                    }

                    matches.push(TunedMatch {
                        pattern_type: self.group_map[name],
                        start,
                        end,
                        matched_text: Cow::Borrowed(m.as_str()),
                    });
                    break;
                }
            }
        }

        // Sort by position
        matches.sort_by_key(|m: &TunedMatch| m.start);
        matches
    }

    /// Create skeleton text with placeholders and extract matches
    ///
    /// Returns (skeleton, matches) where skeleton has {0}, {1}, etc. placeholders.
    pub fn extract_skeleton<'a>(&self, text: &'a str) -> (String, SmallVec<[TunedMatch<'a>; 32]>) {
        let matches = self.find_matches(text);

        if matches.is_empty() {
            return (text.to_string(), matches);
        }

        // Build skeleton with capacity hint
        let mut skeleton = String::with_capacity(text.len());
        let mut last_end = 0;

        for (i, m) in matches.iter().enumerate() {
            // Add text before match
            skeleton.push_str(&text[last_end..m.start]);
            // Add placeholder
            skeleton.push('{');
            // Efficient integer formatting for small numbers
            if i < 10 {
                skeleton.push((b'0' + i as u8) as char);
            } else {
                skeleton.push_str(&i.to_string());
            }
            skeleton.push('}');
            last_end = m.end;
        }

        // Add remaining text
        skeleton.push_str(&text[last_end..]);

        (skeleton, matches)
    }

    /// Restore text from skeleton and matches
    pub fn restore_text(&self, skeleton: &str, matches: &[OwnedMatch]) -> String {
        let mut result = skeleton.to_string();

        // Replace in reverse order to maintain positions
        for (i, m) in matches.iter().enumerate().rev() {
            let placeholder = if i < 10 {
                format!("{{{}}}", i)
            } else {
                format!("{{{}}}", i)
            };
            result = result.replace(&placeholder, &m.matched_text);
        }

        result
    }

    /// Get pattern statistics
    pub fn get_stats<'a>(&self, matches: &[TunedMatch<'a>]) -> HashMap<PatternType, usize> {
        let mut stats = HashMap::new();
        for m in matches {
            *stats.entry(m.pattern_type).or_insert(0) += 1;
        }
        stats
    }
}

impl Default for TunedPatternLearner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fused_regex_single_pass() {
        let learner = TunedPatternLearner::new();
        let text = "2024-01-15 10:30:45 INFO User john@example.com logged in from 192.168.1.100";

        let matches = learner.find_matches(text);

        // Should find: timestamp, log level, email, IP
        assert!(matches.len() >= 3);

        let types: Vec<_> = matches.iter().map(|m| m.pattern_type).collect();
        assert!(types.contains(&PatternType::Timestamp) || types.contains(&PatternType::Date));
        assert!(types.contains(&PatternType::IPv4));
        assert!(types.contains(&PatternType::Email));
    }

    #[test]
    fn test_skeleton_extraction() {
        let learner = TunedPatternLearner::new();
        let text = "IP: 192.168.1.100 at 10:30:45";

        let (skeleton, matches) = learner.extract_skeleton(text);

        // Skeleton should have placeholders
        assert!(skeleton.contains("{0}"));
        assert!(!skeleton.contains("192.168.1.100"));

        // Should be able to restore
        let owned: Vec<OwnedMatch> = matches.into_iter().map(|m| m.into()).collect();
        let restored = learner.restore_text(&skeleton, &owned);
        assert_eq!(text, restored);
    }

    #[test]
    fn test_zero_copy() {
        let learner = TunedPatternLearner::new();
        let text = "2024-01-15 INFO test";

        let matches = learner.find_matches(text);

        // Verify Cow is borrowed, not owned
        for m in &matches {
            assert!(matches!(m.matched_text, Cow::Borrowed(_)));
        }
    }

    #[test]
    fn test_pattern_priority() {
        let learner = TunedPatternLearner::new();
        // Timestamp should be detected as timestamp, not as date + time separately
        let text = "2024-01-15T10:30:45Z";

        let matches = learner.find_matches(text);

        // Should be one timestamp match, not date + time
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].pattern_type, PatternType::Timestamp);
    }

    #[test]
    fn test_large_text_performance() {
        let learner = TunedPatternLearner::new();

        // Generate large log
        let log_line = "2024-01-15 10:30:45 INFO User logged in from 192.168.1.100\n";
        let large_text = log_line.repeat(1000);

        let start = std::time::Instant::now();
        let matches = learner.find_matches(&large_text);
        let elapsed = start.elapsed();

        // Should complete quickly (< 100ms for 1000 lines)
        assert!(elapsed.as_millis() < 100, "Took too long: {:?}", elapsed);
        assert!(!matches.is_empty());
    }
}
