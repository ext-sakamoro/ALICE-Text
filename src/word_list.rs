//! 語の一覧と、それが文に現れるかの照合 (Unicode の compatibility caseless match、D146)
//!
//! 語と文は同じ key に揃えてから比べる: key(x) = NFKD(fold(NFKD(fold(NFD(x))))) (Unicode §3.13 D146) 全角・半角・
//! 合字・大文字小文字の違いでは語は隠れない (例: "ＢＡＤ" / "ﾊﾞｶ" / "ﬁne" / "STRASSE" は "bad" / "バカ" / "fine" /
//! "straße" に当たる) 正規化と folding は [`crate::unicode_norm`] (Unicode 16.0.0 の生成表) だけを使う
//!
//! 照合は全語の key から作った Aho-Corasick の automaton で、文の key を 1 度なめる 遷移は code point の順序で
//! 並べるので、同じ語の一覧はどの端末でも同じ automaton と同じ答えになる
//!
//! Author: Moroya Sakamoto

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use crate::unicode_norm::{case_fold, to_nfd, to_nfkd};

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
}

impl WordList {
    /// The list of `words` (each compared by its caseless key)
    ///
    /// # Errors
    /// [`WordListError::EmptyWord`] for the first empty word.
    pub fn new(words: &[&str]) -> Result<Self, WordListError> {
        let mut nodes = vec![Node::default()];
        for (i, w) in words.iter().enumerate() {
            let k = key(w);
            if k.is_empty() {
                return Err(WordListError::EmptyWord(i));
            }
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

    /// The indices of the words that occur in `text`, ascending, each once
    #[must_use]
    pub fn matches(&self, text: &str) -> Vec<usize> {
        let mut seen = vec![false; self.words];
        let mut at = 0;
        for c in key(text) {
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
                seen[w] = true;
            }
        }
        (0..self.words).filter(|&w| seen[w]).collect()
    }
}
