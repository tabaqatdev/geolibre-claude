//! A small BM25 index over catalog layer metadata.
//!
//! Dependency-light by design (the roadmap's default over embeddings): tokens are
//! normalized with [`crate::normalize`], so an Arabic query matches an Arabic layer
//! title without a translation model. Ranking is textbook BM25 (k1=1.5, b=0.75).
//! Embeddings remain an optional future add-on for cross-language matching when the
//! catalog's own text isn't multilingual.

use crate::normalize::normalize;
use std::collections::{HashMap, HashSet};

const K1: f64 = 1.5;
const B: f64 = 0.75;

/// One searchable catalog layer. `text` is whatever should be matched (title +
/// service path, later also field names/aliases); the rest is returned to the caller.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub service: String,
    pub layer: u32,
    pub title: String,
    pub geometry: String,
    pub text: String,
}

/// A layer that matched a query, with its BM25 score.
#[derive(Debug, Clone)]
pub struct ScoredEntry {
    pub entry: IndexEntry,
    pub score: f64,
}

pub struct CatalogIndex {
    entries: Vec<IndexEntry>,
    doc_tokens: Vec<Vec<String>>,
    df: HashMap<String, usize>,
    avgdl: f64,
    n: usize,
}

impl CatalogIndex {
    pub fn build(entries: Vec<IndexEntry>) -> Self {
        let doc_tokens: Vec<Vec<String>> = entries
            .iter()
            .map(|e| tokenize(&normalize(&e.text, None).text))
            .collect();

        let mut df: HashMap<String, usize> = HashMap::new();
        for toks in &doc_tokens {
            for t in toks.iter().collect::<HashSet<_>>() {
                *df.entry(t.clone()).or_insert(0) += 1;
            }
        }
        let n = entries.len();
        let total: usize = doc_tokens.iter().map(|t| t.len()).sum();
        let avgdl = if n > 0 { total as f64 / n as f64 } else { 0.0 };

        Self { entries, doc_tokens, df, avgdl, n }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Top `limit` layers for `query`, highest score first (score > 0 only).
    pub fn search(&self, query: &str, limit: usize) -> Vec<ScoredEntry> {
        let q = tokenize(&normalize(query, None).text);
        let avgdl = self.avgdl.max(1.0);

        let mut scored: Vec<(usize, f64)> = Vec::new();
        for (i, toks) in self.doc_tokens.iter().enumerate() {
            let dl = toks.len() as f64;
            let mut score = 0.0;
            for qt in &q {
                let f = toks.iter().filter(|t| *t == qt).count() as f64;
                if f == 0.0 {
                    continue;
                }
                let df = *self.df.get(qt).unwrap_or(&0) as f64;
                let idf = (((self.n as f64 - df + 0.5) / (df + 0.5)) + 1.0).ln();
                let denom = f + K1 * (1.0 - B + B * dl / avgdl);
                score += idf * (f * (K1 + 1.0)) / denom;
            }
            if score > 0.0 {
                scored.push((i, score));
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
            .into_iter()
            .map(|(i, s)| ScoredEntry { entry: self.entries[i].clone(), score: s })
            .collect()
    }
}

fn tokenize(s: &str) -> Vec<String> {
    // is_alphanumeric is Unicode-aware, so Arabic letters are kept as tokens.
    // Applied to both index and query, so the two sides stay consistent.
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(light_stem)
        .collect()
}

/// Light Arabic stemming for *search only* (kept out of `normalize`, which stays
/// literal for attribute WHERE matching): strip the leading definite article "ال"
/// so "مدارس" (schools) matches "المدارس" (the schools). Only for longer tokens, to
/// avoid mangling short words like "الله".
fn light_stem(tok: &str) -> String {
    let chars: Vec<char> = tok.chars().collect();
    if chars.len() > 4 && chars[0] == '\u{0627}' && chars[1] == '\u{0644}' {
        chars[2..].iter().collect()
    } else {
        tok.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(service: &str, layer: u32, title: &str) -> IndexEntry {
        IndexEntry {
            service: service.to_string(),
            layer,
            title: title.to_string(),
            geometry: "esriGeometryPoint".to_string(),
            text: format!("{title} {service}"),
        }
    }

    #[test]
    fn ranks_the_matching_layer_first() {
        let idx = CatalogIndex::build(vec![
            entry("Education/MapServer", 0, "Schools"),
            entry("Transport/MapServer", 3, "Highways"),
            entry("Health/MapServer", 1, "Hospitals"),
        ]);
        let hits = idx.search("schools", 5);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].entry.title, "Schools");
    }

    #[test]
    fn arabic_query_matches_arabic_title_after_normalization() {
        let idx = CatalogIndex::build(vec![
            entry("Education/MapServer", 0, "المدارس"),
            entry("Transport/MapServer", 3, "الطرق"),
        ]);
        // Query written with a different alef/hamza form still matches.
        let hits = idx.search("مدارس", 5);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].entry.title, "المدارس");
    }
}
