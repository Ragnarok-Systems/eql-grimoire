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

/// The game's own name: the subject of every question, so it names no page. It matched 7 of 9
/// unanswerable questions in the eval.
const GAME_PAGES: &[&str] = &["Monsters & Memories"];
/// Pages of general prose, as opposed to records of one zone, NPC, item, quest, spell and so on.
const GENERAL: &[&str] = &["article", "guide"];
/// Words that shape a question rather than say what it is about.
const SHAPE: &[&str] = &[
    "level", "levels", "range", "find", "gives", "give", "quest", "drops", "drop", "zones", "zone",
    "border", "borders", "class", "gets", "their", "my", "there", "monsters", "memories", "mnm",
];
/// Below this share of its own words found in any source, a question naming no page is not answered.
const MIN_COVERAGE: f64 = 0.5;
/// Question words that shape the ask rather than name its subject ("where can I find", "what level").
const KEY_SHAPE: &[&str] = &[
    "level",
    "levels",
    "range",
    "find",
    "found",
    "where",
    "gives",
    "give",
    "quest",
    "quests",
    "drops",
    "drop",
    "dropped",
    "zones",
    "zone",
    "border",
    "borders",
    "class",
    "classes",
    "gets",
    "get",
    "spell",
    "spells",
    "their",
    "my",
    "there",
    "monsters",
    "memories",
    "mnm",
    "can",
    "could",
    "does",
    "do",
    "did",
    "is",
    "are",
    "was",
    "were",
    "what",
    "which",
    "who",
    "whom",
    "whose",
    "how",
    "when",
    "why",
    "much",
    "many",
    "tell",
    "me",
    "about",
    "anyone",
    "know",
    "good",
    "use",
    /* Opinion and place words: a wiki states facts, never rankings, and never says "spot". Measured
     * 2026-10-13: "what zones are the best for farming tin?" had the right source (Tin Ore drops in
     * Sungreet Strand) and was blocked because "best" is on no page. */
    "best",
    "better",
    "great",
    "top",
    "fastest",
    "quickest",
    "easiest",
    "easy",
    "ideal",
    "recommended",
    "recommend",
    "optimal",
    "worth",
    "most",
    "least",
    "should",
    "place",
    "places",
    "spot",
    "spots",
    "area",
    "areas",
    /* Greetings, filler and asking words: how people talk, never what they ask about. Measured
     * 2026-10-13: "Hey guys were is thug camp is at in sgs?" had the right answer drafted and was
     * blocked because "hey" and "guys" are on no page. */
    "hey",
    "hi",
    "hello",
    "yo",
    "sup",
    "guys",
    "guy",
    "all",
    "everyone",
    "anyone",
    "anybody",
    "someone",
    "somebody",
    "folks",
    "yall",
    "ya",
    "you",
    "u",
    "show",
    "tell",
    "help",
    "see",
    "point",
    "please",
    "pls",
    "plz",
    "quick",
    "question",
    "lol",
    "lmao",
    "ok",
    "okay",
    "so",
    "um",
    "umm",
    "ummm",
    "uh",
    "just",
    "really",
    "actually",
    "again",
    "also",
    "still",
];
/// The wiki's words for what a player asks: "buy" is answered by a page that says "sold".
const KEY_SYNONYMS: &[(&str, &[&str])] = &[
    (
        "buy",
        &["sell", "sold", "merchant", "vendor", "cost", "price"],
    ),
    ("purchase", &["sell", "sold", "merchant", "vendor"]),
    ("sell", &["buy", "merchant", "vendor", "sold"]),
    ("sells", &["merchant", "vendor", "sold", "sell"]),
    /* Farming or grinding something is asking where it drops. */
    ("farm", &["drop", "dropped", "drops"]),
    ("farming", &["drop", "dropped", "drops"]),
    ("grind", &["drop", "dropped"]),
    ("grinding", &["drop", "dropped"]),
    ("located", &["found", "location", "zone"]),
    ("location", &["found", "zone", "located"]),
    ("obtain", &["drop", "dropped", "reward", "sold", "quest"]),
    ("earn", &["reward", "quest"]),
    ("first", &["1", "tier"]),
    ("pet", &["minion", "servant", "summon"]),
];

/// A crude stem, the same as `keyStem` in `mnm-lore/src/retrieve.js`: one suffix off, six letters.
fn key_stem(t: &str) -> String {
    let t = ["ing", "ed", "es", "s", "e"]
        .iter()
        .find_map(|s| t.strip_suffix(s))
        .unwrap_or(t);
    t.chars().take(6).collect()
}

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
    /// `vectors.i8`: passages x `dims` int8, value = round(x * 127), in passage order. Empty when
    /// the bundle carries none, and then meaning search is simply not a vote.
    vectors: Vec<i8>,
    dims: usize,
    /// What a query is prefixed with before it is embedded (`meta.embedding.query_prefix`).
    pub query_prefix: String,
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
            vectors: Vec::new(),
            dims: 0,
            query_prefix: String::new(),
        }
    }

    /// Attach the bundle's passage vectors. Refused (left keyword-only) when the sizes disagree.
    pub fn with_vectors(mut self, vectors: Vec<i8>, dims: usize, query_prefix: String) -> Index {
        if dims > 0 && vectors.len() == self.passages.len() * dims {
            self.vectors = vectors;
            self.dims = dims;
            self.query_prefix = query_prefix;
        }
        self
    }

    pub fn has_vectors(&self) -> bool {
        self.dims > 0
    }

    /// Cosine of a unit query vector against every passage vector, best first.
    pub fn vector_search(&self, q: &[f32], limit: usize) -> Vec<Hit> {
        if self.dims == 0 || q.len() != self.dims {
            return Vec::new();
        }
        let mut hits: Vec<Hit> = self
            .vectors
            .chunks_exact(self.dims)
            .enumerate()
            .map(|(i, row)| Hit {
                i,
                score: row
                    .iter()
                    .zip(q)
                    .map(|(a, b)| *a as f64 * *b as f64)
                    .sum::<f64>()
                    / 127.0,
            })
            .filter(|h| h.score > 0.0)
            .collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.i.cmp(&b.i)));
        hits.truncate(limit);
        hits
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
        let idx = Index::build(passages, redirects);
        /* The vectors are optional: a bundle without them, or with a meta that does not say how
         * they were made, still answers on keywords and named pages. */
        let meta: Option<serde_json::Value> = std::fs::read_to_string(root.join("meta.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        let dims = meta
            .as_ref()
            .and_then(|m| m.pointer("/embedding/dims"))
            .and_then(|d| d.as_u64())
            .unwrap_or(0) as usize;
        let prefix = meta
            .as_ref()
            .and_then(|m| m.pointer("/embedding/query_prefix"))
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_owned();
        match std::fs::read(root.join("vectors.i8")) {
            Ok(bytes) => {
                Ok(idx.with_vectors(bytes.into_iter().map(|b| b as i8).collect(), dims, prefix))
            }
            Err(_) => Ok(idx),
        }
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
            /* A longer name that is the shorter one with "a", "an" or "the" in front does not
             * hide it: in "how does a necromancer get ..." both the NPC "A necromancer" and the
             * class "Necromancer" are fair readings (measured 2026-10-08). */
            let article = |long: &str, short: &str| {
                ["a", "an", "the"].iter().any(|a| {
                    long.strip_prefix(&format!(" {a}"))
                        .is_some_and(|rest| rest == short)
                })
            };
            if kept
                .iter()
                .any(|k| k.0.contains(n.0.as_str()) && !article(&k.0, &n.0))
            {
                continue;
            }
            kept.push(n);
        }
        let mut pages: Vec<String> = Vec::new();
        for (_, page) in kept {
            if !pages.contains(page) && !GAME_PAGES.contains(&page.as_str()) {
                pages.push(page.clone());
            }
        }
        /* A named record drops the general pages named beside it. Measured: "which zones border
         * Ail'Vorith?" named the article "Zones", which outranked the zone's own facts. */
        let general = |p: &String| {
            self.first_of_page
                .get(p)
                .and_then(|&i| self.passages[i].kind.as_deref())
                .is_some_and(|k| GENERAL.contains(&k))
        };
        if pages.iter().any(|p| !general(p)) {
            pages.retain(|p| !general(p));
        }
        pages
    }

    /// Whether the sources could answer the question at all, decided before the model sees it.
    ///
    /// A question naming a page always goes to the model. One naming none goes only when at least
    /// half its own words (shape words aside) appear in some source. Measured 2026-10-08 on the
    /// eval: every answerable question named a page or scored 1.00; the unanswerable ones naming no
    /// page scored 0.40 or less. Word overlap misses synonyms ("buy" against "sold"), which is why a
    /// named page bypasses it. The same rule is `answerable` in `mnm-lore/src/retrieve.js`.
    /// The question's key terms that the answer's cited sources never mention. Non-empty means the
    /// wiki does not say what was asked, whatever the model wrote.
    ///
    /// Measured 2026-10-09 on 83 answered questions: it blocked 5 of 6 invented answers ("tame a
    /// dragon", "best guild", "legendary sword", "PvP zone", "raid loot drop rate", where the model
    /// answered with true but unrelated facts) and 1 of 77 real ones, which was itself a non-answer.
    /// With it, 14 of 14 unanswerable questions were declined. Same as `unsupportedTerms` in
    /// `mnm-lore/src/retrieve.js`.
    pub fn unsupported_terms(
        &self,
        question: &str,
        passages: &[&Passage],
        cited: &[usize],
    ) -> Vec<String> {
        let name_tok: HashSet<String> = self
            .named_pages(question)
            .iter()
            .flat_map(|n| tokens(n))
            .map(|t| key_stem(&t))
            .collect();
        let mut key: Vec<String> = Vec::new();
        for t in tokens(question) {
            if !KEY_SHAPE.contains(&t.as_str())
                && !name_tok.contains(&key_stem(&t))
                && !key.contains(&t)
            {
                key.push(t);
            }
        }
        let used: Vec<&Passage> = if cited.is_empty() {
            passages.to_vec()
        } else {
            cited
                .iter()
                .filter_map(|n| passages.get(n.wrapping_sub(1)).copied())
                .collect()
        };
        let text: String = used
            .iter()
            .map(|p| {
                format!(
                    "{} {} {} ",
                    p.page,
                    p.section.as_deref().unwrap_or(""),
                    p.text
                )
            })
            .collect();
        let have: HashSet<String> = tokens(&text).iter().map(|t| key_stem(t)).collect();
        key.into_iter()
            .filter(|t| {
                !have.contains(&key_stem(t))
                    && !KEY_SYNONYMS
                        .iter()
                        .find(|(k, _)| k == t)
                        .is_some_and(|(_, syn)| syn.iter().any(|s| have.contains(&key_stem(s))))
            })
            .collect()
    }

    pub fn answerable(&self, question: &str, passages: &[&Passage]) -> bool {
        if !self.named_pages(question).is_empty() {
            return true;
        }
        let mut want: Vec<String> = tokens(question)
            .into_iter()
            .filter(|t| !SHAPE.contains(&t.as_str()))
            .collect();
        want.sort();
        want.dedup();
        if want.is_empty() {
            return true;
        }
        let best = passages
            .iter()
            .map(|p| {
                let have: HashSet<String> = tokens(&format!(
                    "{} {} {}",
                    p.page,
                    p.section.as_deref().unwrap_or(""),
                    p.text
                ))
                .into_iter()
                .collect();
                want.iter().filter(|t| have.contains(*t)).count() as f64 / want.len() as f64
            })
            .fold(0.0, f64::max);
        best >= MIN_COVERAGE
    }

    /// The passages for a question on keywords and named pages alone.
    pub fn retrieve(&self, question: &str, limit: usize, per_page: usize) -> Vec<&Passage> {
        self.retrieve_with(question, None, limit, per_page)
    }

    /// The passages for a question: named pages, keyword hits and (given the question's vector)
    /// meaning hits, fused by reciprocal rank, at most `per_page` from any one page so an answer
    /// draws on several sources.
    pub fn retrieve_with(
        &self,
        question: &str,
        qvec: Option<&[f32]>,
        limit: usize,
        per_page: usize,
    ) -> Vec<&Passage> {
        let named: Vec<Hit> = self
            .named_pages(question)
            .iter()
            .filter_map(|p| self.first_of_page.get(p))
            .map(|&i| Hit { i, score: 1.0 })
            .collect();
        let meaning = qvec.map(|q| self.vector_search(q, 50)).unwrap_or_default();
        let fused = fuse(&[named, self.search(question, 50), meaning]);
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

    fn kp(page: &str, kind: &str, text: &str) -> Passage {
        Passage {
            page: page.into(),
            kind: Some(kind.into()),
            section: None,
            url: None,
            text: text.into(),
        }
    }

    /// DEFECTS THESE PREVENT, both measured 2026-10-08: "which zones border Ail'Vorith?" retrieved
    /// the article "Zones" ahead of the zone, and questions naming only the game ("phone number for
    /// Monsters & Memories support") went to the model, which invented an answer.
    #[test]
    fn greetings_and_chat_filler_are_never_required() {
        let idx = Index::build(
            vec![kp(
                "Community note: Thug camp",
                "community-note",
                "Thug camp (Sungreet Strand): on the west bank of the river.",
            )],
            vec![],
        );
        let note = vec![&idx.passages[0]];
        assert!(idx
            .unsupported_terms(
                "Hey guys were is thug camp is at in Sungreet Strand?",
                &note,
                &[1]
            )
            .is_empty());
        assert!(idx
            .unsupported_terms(
                "Can you show me were the thug camp is in Sungreet Strand?",
                &note,
                &[1]
            )
            .is_empty());
        assert_eq!(
            idx.unsupported_terms(
                "hey guys where is the dragon camp in Sungreet Strand?",
                &note,
                &[1]
            ),
            vec!["dragon"]
        );
    }

    #[test]
    fn opinion_words_are_never_required_and_farming_means_drops() {
        let idx = Index::build(
            vec![kp(
                "Tin Ore",
                "item",
                "Tin Ore is dropped by Tin Vein and Rich Tin Vein in Sungreet Strand.",
            )],
            vec![],
        );
        let tin = vec![&idx.passages[0]];
        assert!(idx
            .unsupported_terms("what zones are the best for farming tin?", &tin, &[1])
            .is_empty());
        assert!(idx
            .unsupported_terms("fastest place to grind tin ore", &tin, &[1])
            .is_empty());
        assert_eq!(
            idx.unsupported_terms("best zone for farming mithril?", &tin, &[1]),
            vec!["mithril"]
        );
    }

    #[test]
    fn an_answer_whose_sources_never_mention_what_was_asked_is_not_an_answer() {
        let idx = Index::build(
            vec![
                kp(
                    "Mother Gummer",
                    "npc",
                    "Mother Gummer is a Slime, level 12, found in Blind Midden.",
                ),
                kp(
                    "Fishing Pole",
                    "item",
                    "Sold by Old Brannoc, a merchant in Night Harbor.",
                ),
                kp("Fallen Pass", "zone", "Fallen Pass is for levels 19 to 44."),
            ],
            vec![],
        );
        let gummer = vec![&idx.passages[0]];
        assert_eq!(
            idx.unsupported_terms("Does Mother Gummer drop a legendary sword?", &gummer, &[1]),
            vec!["legendary", "sword"]
        );
        let pole = vec![&idx.passages[1]];
        assert!(
            idx.unsupported_terms("Where can I buy a Fishing Pole?", &pole, &[1])
                .is_empty(),
            "\"buy\" is answered by \"sold\""
        );
        let zone = vec![&idx.passages[2]];
        assert!(idx
            .unsupported_terms("What level range is Fallen Pass?", &zone, &[1])
            .is_empty());
    }

    #[test]
    fn a_named_record_beats_a_general_page_and_an_unrelated_question_is_not_answered() {
        let idx = Index::build(
            vec![
                kp("Zones", "article", "A list of zones: SZUUR, Calafrey."),
                kp(
                    "Ail'Vorith",
                    "zone",
                    "Ail'Vorith borders Great Cavern Sea and Blind Midden.",
                ),
                kp("Monsters & Memories", "article", "An MMORPG."),
                kp("Fishing Pole", "item", "Sold by Old Brannoc."),
            ],
            vec![],
        );
        assert_eq!(
            idx.named_pages("Which zones border Ail'Vorith?"),
            vec!["Ail'Vorith"]
        );
        assert_eq!(idx.named_pages("What are the zones?"), vec!["Zones"]);
        let nec = Index::build(
            vec![
                kp(
                    "A necromancer",
                    "npc",
                    "A necromancer stands in Underdocks.",
                ),
                kp("Necromancer", "class", "Necromancers raise the dead."),
                kp("Aashir's Journal", "item", "A journal."),
                kp("Journal", "item", "A book."),
            ],
            vec![],
        );
        assert_eq!(
            nec.named_pages("how does a necromancer get their first pet?"),
            vec!["A necromancer", "Necromancer"],
            "a leading article does not hide the shorter name"
        );
        assert_eq!(
            nec.named_pages("where is aashir's journal?"),
            vec!["Aashir's Journal"]
        );
        let q = "What is the phone number for Monsters & Memories customer support?";
        assert!(idx.named_pages(q).is_empty());
        assert!(!idx.answerable(q, &idx.retrieve(q, 5, 2)));
        let buy = "Where can I buy a Fishing Pole?";
        assert!(
            idx.answerable(buy, &idx.retrieve(buy, 5, 2)),
            "a named page always goes on"
        );
        let near = "who sold the brannoc pole";
        assert!(idx.answerable(near, &idx.retrieve(near, 5, 2)));
    }

    /// The meaning vote finds a passage that shares no word with the question, and a vector file
    /// whose size disagrees with the passages is refused rather than misread.
    #[test]
    fn meaning_search_is_a_third_vote_and_mismatched_vectors_are_refused() {
        let ps = vec![
            kp("Bone Chips", "item", "Dropped by skeletons."),
            kp("Lift Bones", "spell", "Raises a zombie servant."),
        ];
        let vectors: Vec<i8> = vec![127, 0, 0, 127];
        let idx = Index::build(ps.clone(), vec![]).with_vectors(vectors, 2, "q: ".into());
        assert!(idx.has_vectors());
        let undead_pet = [0.0f32, 1.0];
        let got: Vec<&str> = idx
            .retrieve_with("how do I get an undead pet", Some(&undead_pet), 2, 2)
            .iter()
            .map(|p| p.page.as_str())
            .collect();
        assert_eq!(got.first(), Some(&"Lift Bones"));
        assert!(
            idx.retrieve("how do I get an undead pet", 2, 2).is_empty(),
            "no keyword hit"
        );
        let bad = Index::build(ps, vec![]).with_vectors(vec![1, 2, 3], 2, String::new());
        assert!(!bad.has_vectors());
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
