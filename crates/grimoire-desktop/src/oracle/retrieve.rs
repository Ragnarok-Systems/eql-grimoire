//! Oracle's retrieval: which passages of the wiki bundle a question is about.
//!
//! A PORT OF `mnm-lore/src/retrieve.js`, and it has to stay one. The bundle's retrieval check was
//! measured there (15/16 pages in the top 5 with vectors, 14/16 with keyword and named-page votes
//! alone, which is what this file does), so the tokeniser, the BM25 constants, the title boost, the
//! named-page rule and the reciprocal-rank fusion are the same numbers as that file. Meaning search
//! (the vectors) is not here yet: it needs the embedding model at runtime.

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// One passage of `passages.jsonl`.
#[derive(Clone, Debug, Deserialize)]
pub struct Passage {
    pub page: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub section: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    pub text: String,
}

#[derive(Deserialize)]
struct Redirect {
    from: String,
    to: String,
}

const STOP: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "do", "does", "for", "from", "get",
    "how", "i", "in", "is", "it", "of", "on", "or", "the", "to", "what", "when", "where", "which",
    "who", "why", "with", "you", "your",
];

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\''
}

/// The search words of a text: lower case, possessive `'s` dropped, stop words and single letters
/// out. Numbers stay even when one digit long: "levels 3 to 15" needs the 3.
pub fn tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !word_char(c))
        .filter(|w| !w.is_empty())
        .map(|w| w.strip_suffix("'s").unwrap_or(w).to_owned())
        .filter(|t| {
            (t.chars().count() > 1 || t.chars().any(|c| c.is_numeric()))
                && !STOP.contains(&t.as_str())
        })
        .collect()
}

/// Lower case, every run of other characters one space, padded so `contains` matches whole words.
fn fold(s: &str) -> String {
    let lower = s.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !word_char(c))
        .filter(|w| !w.is_empty())
        .collect();
    format!(" {} ", words.join(" "))
}

/// A hit: passage index and score, best first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub i: usize,
    pub score: f64,
}

/// The loaded passages and their keyword index.
pub struct Index {
    pub passages: Vec<Passage>,
    /// term -> (passage, weighted term frequency)
    postings: HashMap<String, Vec<(u32, f32)>>,
    lens: Vec<f32>,
    avg: f64,
    /// Every page name (folded) a question can name, with the page it means.
    names: Vec<(String, String)>,
    first_of_page: HashMap<String, usize>,
}

const K1: f64 = 1.2;
const B: f64 = 0.75;
const TITLE_BOOST: f32 = 3.0;

impl Index {
    pub fn build(passages: Vec<Passage>, redirects: Vec<(String, String)>) -> Index {
        let mut postings: HashMap<String, Vec<(u32, f32)>> = HashMap::new();
        let mut lens = Vec::with_capacity(passages.len());
        let mut first_of_page = HashMap::new();
        for (i, p) in passages.iter().enumerate() {
            first_of_page.entry(p.page.clone()).or_insert(i);
            let mut tf: HashMap<String, f32> = HashMap::new();
            for t in tokens(&p.text) {
                *tf.entry(t).or_default() += 1.0;
            }
            let title = format!("{} {}", p.page, p.section.as_deref().unwrap_or(""));
            for t in tokens(&title) {
                *tf.entry(t).or_default() += TITLE_BOOST;
            }
            lens.push(tf.values().sum());
            for (t, f) in tf {
                postings.entry(t).or_default().push((i as u32, f));
            }
        }
        let avg = lens.iter().map(|l| *l as f64).sum::<f64>() / lens.len().max(1) as f64;
        let mut seen = HashSet::new();
        let mut names = Vec::new();
        for p in &passages {
            if seen.insert(p.page.clone()) {
                names.push((p.page.clone(), p.page.clone()));
            }
        }
        names.extend(redirects);
        let names = names
            .into_iter()
            .filter(|(name, _)| name.chars().count() >= 4)
            .map(|(name, page)| (fold(&name), page))
            .collect();
        Index {
            passages,
            postings,
            lens,
            avg,
            names,
            first_of_page,
        }
    }

    /// Read `passages.jsonl` and `redirects.json` from a bundle folder.
    pub fn load(root: &Path) -> Result<Index, String> {
        let text = std::fs::read_to_string(root.join("passages.jsonl"))
            .map_err(|e| format!("could not read passages.jsonl: {e}"))?;
        let mut passages = Vec::new();
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            passages.push(
                serde_json::from_str::<Passage>(line)
                    .map_err(|e| format!("passages.jsonl line {}: {e}", n + 1))?,
            );
        }
        let redirects = match std::fs::read_to_string(root.join("redirects.json")) {
            Ok(t) => serde_json::from_str::<Vec<Redirect>>(&t)
                .map_err(|e| format!("redirects.json: {e}"))?
                .into_iter()
                .map(|r| (r.from, r.to))
                .collect(),
            Err(_) => Vec::new(),
        };
        Ok(Index::build(passages, redirects))
    }

    /// BM25 over passage text, title words counted three times.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let n = self.passages.len() as f64;
        let mut scores: HashMap<u32, f64> = HashMap::new();
        let q: HashSet<String> = tokens(query).into_iter().collect();
        for t in &q {
            let Some(list) = self.postings.get(t) else {
                continue;
            };
            let df = list.len() as f64;
            let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
            for &(i, f) in list {
                let f = f as f64;
                let len = self.lens[i as usize] as f64;
                *scores.entry(i).or_default() +=
                    idf * (f * (K1 + 1.0)) / (f + K1 * (1.0 - B + B * len / self.avg));
            }
        }
        let mut hits: Vec<Hit> = scores
            .into_iter()
            .filter(|(_, s)| *s > 0.0)
            .map(|(i, score)| Hit {
                i: i as usize,
                score,
            })
            .collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.i.cmp(&b.i)));
        hits.truncate(limit);
        hits
    }

    /// Page names said in the question, longest first, ignoring a name inside a longer one that
    /// also matched ("Aashir's Journal" is named; "Journal" alone is not). Redirects count.
    pub fn named_pages(&self, question: &str) -> Vec<String> {
        let q = fold(question);
        let mut found: Vec<&(String, String)> = self
            .names
            .iter()
            .filter(|(name, _)| q.contains(name.as_str()))
            .collect();
        found.sort_by_key(|n| std::cmp::Reverse(n.0.len()));
        let mut kept: Vec<&(String, String)> = Vec::new();
        for n in found {
            if kept.iter().any(|k| k.0.contains(n.0.as_str())) {
                continue;
            }
            kept.push(n);
        }
        let mut pages = Vec::new();
        for (_, page) in kept {
            if !pages.contains(page) {
                pages.push(page.clone());
            }
        }
        pages
    }

    /// The passages for a question: named pages and keyword hits fused by reciprocal rank, at most
    /// `per_page` from any one page so an answer draws on several sources.
    pub fn retrieve(&self, question: &str, limit: usize, per_page: usize) -> Vec<&Passage> {
        let named: Vec<Hit> = self
            .named_pages(question)
            .iter()
            .filter_map(|p| self.first_of_page.get(p))
            .map(|&i| Hit { i, score: 1.0 })
            .collect();
        let fused = fuse(&[named, self.search(question, 50)]);
        let mut out = Vec::new();
        let mut used: HashMap<&str, usize> = HashMap::new();
        for h in fused {
            let p = &self.passages[h.i];
            let n = used.entry(p.page.as_str()).or_default();
            if *n >= per_page {
                continue;
            }
            *n += 1;
            out.push(p);
            if out.len() == limit {
                break;
            }
        }
        out
    }
}

/// Reciprocal rank fusion, k = 60: robust to lists that score on different scales.
pub fn fuse(lists: &[Vec<Hit>]) -> Vec<Hit> {
    let mut score: HashMap<usize, f64> = HashMap::new();
    let mut order: Vec<usize> = Vec::new();
    for list in lists {
        for (rank, h) in list.iter().enumerate() {
            let e = score.entry(h.i).or_insert_with(|| {
                order.push(h.i);
                0.0
            });
            *e += 1.0 / (60.0 + rank as f64 + 1.0);
        }
    }
    /* Ties keep first-seen order, as the JS Map insertion order does. */
    let mut out: Vec<Hit> = order
        .into_iter()
        .map(|i| Hit {
            i,
            score: score[&i],
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(page: &str, text: &str) -> Passage {
        Passage {
            page: page.into(),
            kind: None,
            section: None,
            url: None,
            text: text.into(),
        }
    }

    #[test]
    fn tokens_match_the_js_tokeniser() {
        assert_eq!(
            tokens("What level is Fallen Pass?"),
            vec!["level", "fallen", "pass"]
        );
        assert_eq!(tokens("levels 3 to 15"), vec!["levels", "3", "15"]);
        assert_eq!(tokens("Aashir's Journal"), vec!["aashir", "journal"]);
        assert_eq!(fold("Ail'Vorith, the city!"), " ail'vorith the city ");
    }

    #[test]
    fn a_named_page_wins_a_vote_and_the_longer_name_hides_the_shorter() {
        let idx = Index::build(
            vec![
                p("Journal", "a book"),
                p(
                    "Aashir's Journal",
                    "Aashir wrote in it. Found in Fallen Pass.",
                ),
                p("Fallen Pass", "Fallen Pass is a zone for levels 10 to 20."),
            ],
            vec![("The Pass".into(), "Fallen Pass".into())],
        );
        assert_eq!(
            idx.named_pages("where is aashir's journal?"),
            vec!["Aashir's Journal"]
        );
        assert_eq!(
            idx.named_pages("what level is the pass"),
            vec!["Fallen Pass"]
        );
        let got: Vec<&str> = idx
            .retrieve("what level is Fallen Pass?", 2, 2)
            .iter()
            .map(|p| p.page.as_str())
            .collect();
        assert_eq!(got[0], "Fallen Pass");
    }

    #[test]
    fn fusion_rewards_agreement_and_per_page_caps_hold() {
        let a = vec![Hit { i: 1, score: 9.0 }, Hit { i: 2, score: 8.0 }];
        let b = vec![Hit { i: 2, score: 1.0 }, Hit { i: 3, score: 0.5 }];
        assert_eq!(fuse(&[a, b])[0].i, 2);
        let idx = Index::build(
            (0..5)
                .map(|n| p("Same", &format!("fallen pass part {n}")))
                .collect(),
            vec![],
        );
        assert_eq!(idx.retrieve("fallen pass", 5, 2).len(), 2);
    }
}
