//! 語の一覧と、それが文に現れるかの照合 (Unicode の compatibility caseless match、D146)
//!
//! 語と文は同じ key に揃えてから比べる: key(x) = NFKD(fold(NFKD(fold(NFD(x))))) (Unicode §3.13 D146) 全角・半角・
//! 合字・大文字小文字の違いでは語は隠れない (例: "ＢＡＤ" / "ﾊﾞｶ" / "ﬁne" / "STRASSE" は "bad" / "バカ" / "fine" /
//! "straße" に当たる) 正規化と folding は [`crate::unicode_norm`] (Unicode 16.0.0 の生成表) だけを使う
//!
//! 既定 ([`WordList::new`]) では、語は文の **語の境界** (UAX #29、[`crate::word_break`]) で始まり境界で終わる時だけ
//! 当たる ("ass" は "class" に当たらず "an ass." に当たる、"e" は "café" に当たらない、空白を含む語は境界から
//! 境界までの区間に当たる、片仮名の連続は 1 語 (WB13) なので "バカ" は "バカヤロー" に当たらない)
//! [`WordList::substring`] は境界を見ない部分文字列の照合 (語は長い語の中にも、分解した文字の一部にも当たる)
//!
//! 照合は全語の key から作った Aho-Corasick の automaton で、文の key を 1 度なめる 遷移は code point の順序で
//! 並べるので、同じ語の一覧はどの端末でも同じ automaton と同じ答えになる
//!
//! Author: Moroya Sakamoto

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use crate::unicode_norm::{case_fold, to_nfd, to_nfkd};
use crate::word_break::boundaries;

/// The compatibility caseless key of `s` (D146)
fn key(s: &str) -> Vec<char> {
    to_nfkd(&case_fold(&to_nfkd(&case_fold(&to_nfd(s)))))
        .chars()
        .collect()
}

/// Why a word list was not made
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordListError {
    /// The word at this index is empty (it would match every text)
    EmptyWord(usize),
}

impl fmt::Display for WordListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyWord(i) => write!(f, "word {i} is empty"),
        }
    }
}

impl std::error::Error for WordListError {}

/// One state of the automaton
#[derive(Debug, Clone, Default)]
struct Node {
    next: BTreeMap<char, usize>,
    fail: usize,
    /// the words that end here, by index, including those reached through the failure links
    out: Vec<usize>,
}

/// A list of words and the automaton that finds them
#[derive(Debug, Clone)]
pub struct WordList {
    words: usize,
    nodes: Vec<Node>,
    /// each word's key length in chars
    lengths: Vec<usize>,
    /// whether a match must start and end at word boundaries
    bounded: bool,
}

impl WordList {
    /// The list of `words` (each compared by its caseless key), each found only from a word boundary of the
    /// text to a word boundary
    ///
    /// # Errors
    /// [`WordListError::EmptyWord`] for the first empty word.
    pub fn new(words: &[&str]) -> Result<Self, WordListError> {
        Self::build(words, true)
    }

    /// The list of `words`, each found wherever its key occurs in the text's key (no word or grapheme boundary
    /// is looked at)
    ///
    /// # Errors
    /// [`WordListError::EmptyWord`] for the first empty word.
    pub fn substring(words: &[&str]) -> Result<Self, WordListError> {
        Self::build(words, false)
    }

    fn build(words: &[&str], bounded: bool) -> Result<Self, WordListError> {
        let mut nodes = vec![Node::default()];
        let mut lengths = Vec::with_capacity(words.len());
        for (i, w) in words.iter().enumerate() {
            let k = key(w);
            if k.is_empty() {
                return Err(WordListError::EmptyWord(i));
            }
            lengths.push(k.len());
            let mut at = 0;
            for c in k {
                at = if let Some(&n) = nodes[at].next.get(&c) {
                    n
                } else {
                    nodes.push(Node::default());
                    let n = nodes.len() - 1;
                    nodes[at].next.insert(c, n);
                    n
                };
            }
            nodes[at].out.push(i);
        }
        // failure links, breadth first (a node's link is the longest proper suffix that is also a prefix)
        let mut queue: VecDeque<usize> = nodes[0].next.values().copied().collect();
        while let Some(u) = queue.pop_front() {
            let edges: Vec<(char, usize)> = nodes[u].next.iter().map(|(&c, &v)| (c, v)).collect();
            for (c, v) in edges {
                let mut f = nodes[u].fail;
                let link = loop {
                    if let Some(&n) = nodes[f].next.get(&c) {
                        if n != v {
                            break n;
                        }
                    }
                    if f == 0 {
                        break 0;
                    }
                    f = nodes[f].fail;
                };
                nodes[v].fail = link;
                let inherited = nodes[link].out.clone();
                nodes[v].out.extend(inherited);
                queue.push_back(v);
            }
        }
        Ok(Self {
            words: words.len(),
            nodes,
            lengths,
            bounded,
        })
    }

    /// The number of words in the list
    #[must_use]
    pub const fn len(&self) -> usize {
        self.words
    }

    /// Whether the list has no words
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.words == 0
    }

    /// The indices of the words found in `text` (at word boundaries, or anywhere for a [`Self::substring`]
    /// list), ascending, each once
    #[must_use]
    pub fn matches(&self, text: &str) -> Vec<usize> {
        // the text's key, and which char positions of it are word boundaries of the text (each segment keyed on
        // its own: a segment never starts with a combining mark (WB4), so the keys concatenate)
        let (keyed, at_boundary) = if self.bounded {
            let b = boundaries(text);
            let mut k = Vec::with_capacity(text.len());
            let mut mark = vec![true];
            for w in b.windows(2) {
                k.extend(key(&text[w[0]..w[1]]));
                mark.resize(k.len(), false);
                mark.push(true);
            }
            (k, mark)
        } else {
            (key(text), Vec::new())
        };
        let mut seen = vec![false; self.words];
        let mut at = 0;
        for (i, &c) in keyed.iter().enumerate() {
            loop {
                if let Some(&n) = self.nodes[at].next.get(&c) {
                    at = n;
                    break;
                }
                if at == 0 {
                    break;
                }
                at = self.nodes[at].fail;
            }
            for &w in &self.nodes[at].out {
                let end = i + 1;
                if !self.bounded || (at_boundary[end - self.lengths[w]] && at_boundary[end]) {
                    seen[w] = true;
                }
            }
        }
        (0..self.words).filter(|&w| seen[w]).collect()
    }
}
