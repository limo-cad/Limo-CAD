//! Shared help catalog + in-process BM25 search for MCP `cad_help` and desktop Help.
//!
//! The corpus is every markdown file under `knowledge/**`, embedded by `build.rs`
//! and listed by [`knowledge_files`]. The MCP server serves those same embeds as
//! `limo-cad://knowledge/...` resources; BM25 search covers the Concept pages
//! filtered through [`parse_markdown`]. [`SearchIndex`] supplies the ranking
//! behind the shared help-tool schema.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Default search hit count.
pub const SEARCH_DEFAULT_LIMIT: usize = 5;
/// Hard max search hits.
pub const SEARCH_MAX_LIMIT: usize = 10;
/// Snippet character budget (title/topics accompany the snippet in MCP).
pub const SNIPPET_CHARS: usize = 280;
/// Max UTF-8 bytes returned by [`HelpStore::get`].
pub const GET_MAX_BYTES: usize = 12 * 1024;
/// Topics listing page size.
pub const TOPICS_PAGE_SIZE: usize = 50;

/// One authored help page (frontmatter + body).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub topics: Vec<String>,
    pub keywords: Vec<String>,
    pub description: String,
    pub body: String,
    pub related_recipes: Vec<String>,
    pub status: String,
}

/// Search hit with a short snippet (never the full body).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub topics: Vec<String>,
    pub snippet: String,
    pub score: f64,
    pub related_recipes: Vec<String>,
    pub status: String,
}

/// Result of [`HelpStore::get`] with optional truncation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GetResult {
    pub id: String,
    pub title: String,
    pub topics: Vec<String>,
    pub keywords: Vec<String>,
    pub description: String,
    pub body: String,
    pub related_recipes: Vec<String>,
    pub status: String,
    pub truncated: bool,
}

/// Pluggable full-text backend. v1 = BM25; scale path = Tantivy behind same trait.
pub trait SearchIndex: Send + Sync {
    fn search(&self, catalog: &Catalog, query: &str, limit: usize) -> Vec<SearchHit>;
}

/// Id → page catalog loaded from embedded markdown.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pages: BTreeMap<String, Page>,
    topic_index: BTreeMap<String, Vec<String>>,
}

impl Catalog {
    pub fn from_pages(pages: impl IntoIterator<Item = Page>) -> Self {
        let mut pages_map = BTreeMap::new();
        let mut topic_index: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for page in pages {
            for topic in &page.topics {
                topic_index
                    .entry(topic.to_ascii_lowercase())
                    .or_default()
                    .push(page.id.clone());
            }
            pages_map.insert(page.id.clone(), page);
        }
        for ids in topic_index.values_mut() {
            ids.sort();
            ids.dedup();
        }
        Self {
            pages: pages_map,
            topic_index,
        }
    }

    pub fn get(&self, id: &str) -> Option<&Page> {
        self.pages.get(id)
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.pages.keys().map(String::as_str)
    }

    pub fn pages(&self) -> impl Iterator<Item = &Page> {
        self.pages.values()
    }

    pub fn topics_page(&self, offset: usize, limit: usize) -> (Vec<(String, usize)>, usize) {
        let total = self.topic_index.len();
        let page: Vec<(String, usize)> = self
            .topic_index
            .iter()
            .skip(offset)
            .take(limit)
            .map(|(topic, ids)| (topic.clone(), ids.len()))
            .collect();
        (page, total)
    }

    pub fn ids_for_topic(&self, topic: &str) -> Option<&[String]> {
        self.topic_index
            .get(&topic.to_ascii_lowercase())
            .map(Vec::as_slice)
    }
}

/// Fielded BM25 over an in-memory catalog (title > keywords/topics > description > body).
#[derive(Debug, Default)]
pub struct Bm25Index {
    /// Precomputed avg field lengths for BM25 length normalization.
    avg: [f64; FIELD_COUNT],
    n_docs: usize,
    /// term → document frequency
    df: HashMap<String, usize>,
    /// Per-page term statistics in catalog (id) order, so a query never
    /// re-tokenizes the corpus.
    docs: Vec<DocStats>,
}

/// Scored fields: title, keywords, topics, description, body.
const FIELD_COUNT: usize = 5;
const FIELD_WEIGHTS: [f64; FIELD_COUNT] = [4.0, 3.0, 3.0, 2.0, 1.0];

#[derive(Debug, Default)]
struct DocStats {
    id: String,
    /// term → term frequency per field
    tf: HashMap<String, [u32; FIELD_COUNT]>,
    /// token count per field
    len: [f64; FIELD_COUNT],
}

impl Bm25Index {
    const K1: f64 = 1.2;
    const B: f64 = 0.75;

    pub fn build(catalog: &Catalog) -> Self {
        let mut df: HashMap<String, usize> = HashMap::new();
        let mut sums = [0.0f64; FIELD_COUNT];
        let mut docs = Vec::with_capacity(catalog.len());
        let n = catalog.len().max(1);

        for page in catalog.pages() {
            let fields = [
                tokenize(&page.title),
                tokenize(&page.keywords.join(" ")),
                tokenize(&page.topics.join(" ")),
                tokenize(&page.description),
                tokenize(&page.body),
            ];
            let mut tf: HashMap<String, [u32; FIELD_COUNT]> = HashMap::new();
            let mut len = [0.0f64; FIELD_COUNT];
            for (field, tokens) in fields.into_iter().enumerate() {
                len[field] = tokens.len() as f64;
                sums[field] += len[field];
                for token in tokens {
                    tf.entry(token).or_default()[field] += 1;
                }
            }
            for term in tf.keys() {
                *df.entry(term.clone()).or_default() += 1;
            }
            docs.push(DocStats {
                id: page.id.clone(),
                tf,
                len,
            });
        }

        let n_f = n as f64;
        Self {
            avg: sums.map(|sum| sum / n_f),
            n_docs: n,
            df,
            docs,
        }
    }

    fn idf(&self, term: &str) -> f64 {
        let df = *self.df.get(term).unwrap_or(&0) as f64;
        let n = self.n_docs as f64;
        ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
    }

    fn field_score(&self, tf: f64, avgdl: f64, dl: f64, idf: f64) -> f64 {
        if tf <= 0.0 || idf <= 0.0 {
            return 0.0;
        }
        let avg = avgdl.max(1.0);
        let denom = tf + Self::K1 * (1.0 - Self::B + Self::B * (dl / avg));
        idf * (tf * (Self::K1 + 1.0)) / denom
    }
}

impl SearchIndex for Bm25Index {
    fn search(&self, catalog: &Catalog, query: &str, limit: usize) -> Vec<SearchHit> {
        let q_terms = tokenize(query);
        if q_terms.is_empty() || limit == 0 {
            return Vec::new();
        }
        let weighted: Vec<(&str, f64)> = q_terms
            .iter()
            .map(|term| (term.as_str(), self.idf(term)))
            .collect();

        let mut scored: Vec<(f64, &Page)> = Vec::new();
        for doc in &self.docs {
            let Some(page) = catalog.get(&doc.id) else {
                continue;
            };
            let mut score = 0.0;
            for &(term, idf) in &weighted {
                let Some(tf) = doc.tf.get(term) else {
                    continue;
                };
                for (field, (&weight, &avg)) in FIELD_WEIGHTS.iter().zip(&self.avg).enumerate() {
                    score +=
                        weight * self.field_score(f64::from(tf[field]), avg, doc.len[field], idf);
                }
            }
            if score > 0.0 {
                scored.push((score, page));
            }
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
            .into_iter()
            .map(|(score, page)| SearchHit {
                id: page.id.clone(),
                title: page.title.clone(),
                topics: page.topics.clone(),
                snippet: make_snippet(&page.body, &q_terms, SNIPPET_CHARS),
                score,
                related_recipes: page.related_recipes.clone(),
                status: page.status.clone(),
            })
            .collect()
    }
}

/// Help store: catalog + search index.
pub struct HelpStore {
    catalog: Catalog,
    index: Box<dyn SearchIndex>,
}

impl HelpStore {
    pub fn new(catalog: Catalog, index: Box<dyn SearchIndex>) -> Self {
        Self { catalog, index }
    }

    /// Load the embedded knowledge corpus with BM25.
    pub fn bundled() -> Self {
        let catalog = Catalog::from_pages(embedded_pages());
        let index = Box::new(Bm25Index::build(&catalog));
        Self { catalog, index }
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn search(&self, query: &str, limit: Option<usize>) -> Vec<SearchHit> {
        let limit = limit
            .unwrap_or(SEARCH_DEFAULT_LIMIT)
            .clamp(1, SEARCH_MAX_LIMIT);
        self.index.search(&self.catalog, query, limit)
    }

    /// Id-only lookup. Paths and traversal attempts fail closed.
    pub fn get(&self, id: &str) -> Result<GetResult, String> {
        if !is_safe_id(id) {
            return Err(format!(
                "invalid help id '{id}': id-only allowlist (no paths)"
            ));
        }
        let page = self
            .catalog
            .get(id)
            .ok_or_else(|| format!("unknown help id '{id}'"))?;
        let (body, truncated) = truncate_utf8(&page.body, GET_MAX_BYTES);
        Ok(GetResult {
            id: page.id.clone(),
            title: page.title.clone(),
            topics: page.topics.clone(),
            keywords: page.keywords.clone(),
            description: page.description.clone(),
            body,
            related_recipes: page.related_recipes.clone(),
            status: page.status.clone(),
            truncated,
        })
    }

    pub fn topics(&self, offset: Option<usize>) -> serde_json::Value {
        let offset = offset.unwrap_or(0);
        let (page, total) = self.catalog.topics_page(offset, TOPICS_PAGE_SIZE);
        serde_json::json!({
            "topics": page.iter().map(|(name, count)| serde_json::json!({
                "topic": name,
                "page_count": count,
            })).collect::<Vec<_>>(),
            "offset": offset,
            "limit": TOPICS_PAGE_SIZE,
            "total": total,
            "next_offset": if offset + TOPICS_PAGE_SIZE < total {
                Some(offset + TOPICS_PAGE_SIZE)
            } else {
                None
            },
        })
    }
}

fn is_safe_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 200 {
        return false;
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return false;
    }
    if id.starts_with('.') || id.contains('\0') {
        return false;
    }

    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            cur.push(c.to_ascii_lowercase());
        } else if !cur.is_empty() {
            if cur.len() > 1 || cur.chars().all(|ch| ch.is_ascii_digit()) {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if !cur.is_empty() && (cur.len() > 1 || cur.chars().all(|ch| ch.is_ascii_digit())) {
        out.push(cur);
    }
    out
}

fn make_snippet(body: &str, query_terms: &[String], limit: usize) -> String {
    let plain = body
        .lines()
        .filter(|line| {
            let t = line.trim();
            !t.starts_with('#') && !t.starts_with('>') && !t.is_empty()
        })
        .collect::<Vec<_>>()
        .join(" ");
    let plain = strip_md_noise(&plain);
    let lower = plain.to_ascii_lowercase();
    let mut start = 0usize;
    for term in query_terms {
        if let Some(pos) = lower.find(term.as_str()) {
            start = pos.saturating_sub(40);
            while !plain.is_char_boundary(start) {
                start -= 1;
            }
            break;
        }
    }
    let tail = &plain[start..];
    let mut snippet: String = tail.chars().take(limit).collect();
    if tail.chars().nth(limit).is_some() {
        snippet.push('…');
    }
    snippet.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_md_noise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            if let Some(close) = chars[i..].iter().position(|&c| c == ']') {
                let text: String = chars[i + 1..i + close].iter().collect();
                let after = i + close + 1;
                if after < chars.len() && chars[after] == '(' {
                    if let Some(paren) = chars[after..].iter().position(|&c| c == ')') {
                        out.push_str(&text);
                        i = after + paren + 1;
                        continue;
                    }
                }
            }
        }
        let c = chars[i];
        if c != '*' && c != '_' && c != '`' {
            out.push(c);
        }
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_utf8(s: &str, max_bytes: usize) -> (String, bool) {
    if s.len() <= max_bytes {
        return (s.to_owned(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (s[..end].to_owned(), true)
}

/// Parse YAML-ish frontmatter used by knowledge pages (flat key: value lines).
pub fn parse_markdown(rel_path: &str, raw: &str) -> Option<Page> {
    let raw = raw.replace("\r\n", "\n");
    let (fields, body) = split_frontmatter(&raw);
    if fields.get("searchable").map(String::as_str) == Some("false") {
        return None;
    }

    let page_type = fields.get("type").map(String::as_str).unwrap_or("");
    if !page_type.is_empty() && page_type != "Concept" {
        return None;
    }

    let file_name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    if file_name.eq_ignore_ascii_case("SOURCES.md")
        || file_name.eq_ignore_ascii_case("taxonomy.md")
        || file_name.eq_ignore_ascii_case("index.md")
        || file_name.eq_ignore_ascii_case("log.md")
    {
        return None;
    }

    let id = fields
        .get("id")
        .cloned()
        .unwrap_or_else(|| path_to_id(rel_path));
    if !is_safe_id(&id) {
        return None;
    }

    let title = fields
        .get("title")
        .cloned()
        .or_else(|| {
            body.lines()
                .find_map(|line| line.strip_prefix("# ").map(str::to_owned))
        })
        .unwrap_or_else(|| file_name.trim_end_matches(".md").to_owned());

    Some(Page {
        id,
        title,
        topics: csv_list(fields.get("topics").map(String::as_str).unwrap_or("")),
        keywords: csv_list(fields.get("keywords").map(String::as_str).unwrap_or("")),
        description: fields.get("description").cloned().unwrap_or_default(),
        body: body.trim().to_owned(),
        related_recipes: csv_list(
            fields
                .get("related_recipes")
                .map(String::as_str)
                .unwrap_or(""),
        ),
        status: fields
            .get("status")
            .cloned()
            .unwrap_or_else(|| "draft".into()),
    })
}

fn path_to_id(rel_path: &str) -> String {
    let without = rel_path
        .trim_start_matches("./")
        .trim_start_matches("knowledge/")
        .trim_end_matches(".md")
        .trim_end_matches(".MD");
    without.replace('/', ".")
}

fn csv_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| {
            s.trim()
                .trim_matches(|c| c == '`' || c == '"' || c == '\'')
                .to_owned()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn split_frontmatter(content: &str) -> (HashMap<String, String>, String) {
    if !content.starts_with("---\n") {
        return (HashMap::new(), content.to_owned());
    }
    let Some(end) = content[4..].find("\n---\n") else {
        return (HashMap::new(), content.to_owned());
    };
    let fm = &content[4..4 + end];
    let body = content[4 + end + 5..].to_owned();
    let mut fields = HashMap::new();
    for line in fm.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let Some(sep) = line.find(':') else {
            continue;
        };
        let key = line[..sep].trim().to_owned();
        let mut val = line[sep + 1..].trim().to_owned();
        if (val.starts_with('"') && val.ends_with('"'))
            || (val.starts_with('\'') && val.ends_with('\''))
        {
            val = val[1..val.len() - 1].to_owned();
        }
        if val.starts_with('[') && val.ends_with(']') {
            val = val[1..val.len() - 1].to_owned();
        }
        fields.insert(key, val);
    }
    (fields, body)
}

/// MCP / file-door URI prefix for bundled knowledge markdown.
pub const KNOWLEDGE_URI_PREFIX: &str = "limo-cad://knowledge/";

/// One embedded knowledge file available as an MCP resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnowledgeFile {
    /// Path relative to the `knowledge/` directory (POSIX separators).
    pub path: &'static str,
    /// Raw markdown bytes embedded at compile time.
    pub text: &'static str,
}

impl KnowledgeFile {
    /// Canonical MCP resource URI (`limo-cad://knowledge/...`).
    pub fn uri(&self) -> String {
        format!("{KNOWLEDGE_URI_PREFIX}{}", self.path)
    }
}

/// All embedded knowledge markdown exposed as MCP `resources/*`.
///
/// Includes the OKF index and provenance pages that are intentionally excluded
/// from BM25/`cad_help` search. Searchable Concept pages are the same embeds
/// filtered through [`parse_markdown`].
pub fn knowledge_files() -> &'static [KnowledgeFile] {
    KNOWLEDGE_FILES
}

/// Look up an embedded knowledge file by MCP URI.
pub fn knowledge_file_by_uri(uri: &str) -> Option<&'static KnowledgeFile> {
    let path = uri
        .strip_prefix(KNOWLEDGE_URI_PREFIX)
        .or_else(|| uri.strip_prefix("nbcad://knowledge/"))?;
    if path.is_empty() || path.contains("..") || path.starts_with('/') {
        return None;
    }
    knowledge_files().iter().find(|file| file.path == path)
}

include!(concat!(env!("OUT_DIR"), "/knowledge_bundle.rs"));

fn embedded_pages() -> Vec<Page> {
    knowledge_files()
        .iter()
        .filter_map(|file| parse_markdown(file.path, file.text))
        .collect()
}

/// Validate the bundled corpus (CI / `limo-cad-help check`).
pub fn check_corpus() -> Result<usize, Vec<String>> {
    let pages = embedded_pages();
    let mut errors = Vec::new();
    if pages.is_empty() {
        errors.push("no searchable help pages embedded".into());
    }
    let mut ids = HashSet::new();
    for page in &pages {
        if !is_safe_id(&page.id) {
            errors.push(format!("unsafe id '{}'", page.id));
        }
        if !ids.insert(page.id.clone()) {
            errors.push(format!("duplicate id '{}'", page.id));
        }
        if page.title.is_empty() {
            errors.push(format!("{}: empty title", page.id));
        }
    }
    if errors.is_empty() {
        Ok(pages.len())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearance_fit_hits_fits_page() {
        let store = HelpStore::bundled();
        let hits = store.search("clearance fit", Some(5));
        assert!(!hits.is_empty(), "expected hits for 'clearance fit'");
        let top_ids: Vec<_> = hits.iter().map(|h| h.id.as_str()).collect();
        assert!(
            top_ids
                .iter()
                .any(|id| id.contains("fits-clearances") || id.contains("fits")),
            "expected fits-clearances in hits, got {top_ids:?}"
        );
        assert!(hits[0].snippet.chars().count() <= SNIPPET_CHARS + 1);
    }

    #[test]
    fn draft_angle_hits_dfm_guidelines() {
        let store = HelpStore::bundled();
        let hits = store.search("draft angle", Some(5));
        assert!(!hits.is_empty(), "expected hits for 'draft angle'");
        let top_ids: Vec<_> = hits.iter().map(|h| h.id.as_str()).collect();
        assert!(
            top_ids.iter().any(|id| id.contains("dfm-process")
                || id.contains("dfm-overview")
                || hits
                    .iter()
                    .any(|h| h.snippet.to_ascii_lowercase().contains("draft"))),
            "expected DFM/draft hit, got {top_ids:?} snippets {:?}",
            hits.iter().map(|h| &h.snippet).collect::<Vec<_>>()
        );
    }

    #[test]
    fn get_by_id_works_and_path_fails() {
        let store = HelpStore::bundled();
        let id = store
            .catalog()
            .ids()
            .find(|id| id.contains("fits-clearances"))
            .expect("fits page")
            .to_owned();
        let got = store.get(&id).expect("get by id");
        assert_eq!(got.id, id);
        assert!(!got.body.is_empty());
        assert!(!got.truncated || got.body.len() <= GET_MAX_BYTES);

        for bad in [
            "../etc/passwd",
            "knowledge/machine-design/concepts/fits-clearances.md",
            "machine-design/concepts/fits-clearances",
            "../../secrets",
            "/absolute/path",
            "foo\\bar",
        ] {
            assert!(
                store.get(bad).is_err(),
                "path/traversal get must fail for {bad}"
            );
        }
    }

    #[test]
    fn search_respects_limit_cap() {
        let store = HelpStore::bundled();
        let hits = store.search("fit", Some(100));
        assert!(hits.len() <= SEARCH_MAX_LIMIT);
    }

    #[test]
    fn topics_lists_known_topics() {
        let store = HelpStore::bundled();
        let value = store.topics(Some(0));
        let total = value["total"].as_u64().unwrap();
        assert!(total > 0);
        let topics = value["topics"].as_array().unwrap();
        assert!(!topics.is_empty());
    }

    #[test]
    fn agent_doctrine_page_is_searchable() {
        let store = HelpStore::bundled();
        let hits = store.search("cad_help tenacity", Some(5));
        assert!(
            hits.iter().any(|h| h.id.contains("agent-mcp-workflow")
                || h.title.to_ascii_lowercase().contains("agent")),
            "agent doctrine should be searchable, got {:?}",
            hits.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn snap_fit_hits_am_snap_page() {
        let store = HelpStore::bundled();
        let hits = store.search("snap fit cantilever clip", Some(5));
        assert!(!hits.is_empty(), "expected hits for snap fit");
        assert!(
            hits.iter().any(|h| h.id.contains("am-snap-fit")),
            "expected am-snap-fit in hits, got {:?}",
            hits.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn living_hinge_hits_am_snap_page() {
        let store = HelpStore::bundled();
        let hits = store.search("living hinge", Some(5));
        assert!(
            hits.iter().any(|h| h.id.contains("am-snap-fit")),
            "expected am-snap-fit for living hinge, got {:?}",
            hits.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn thin_wall_fdm_hits_am_thin_walls() {
        let store = HelpStore::bundled();
        let hits = store.search("thin wall FDM min wall", Some(5));
        assert!(
            hits.iter()
                .any(|h| h.id.contains("am-thin-walls") || h.id.contains("am-snap-fit")),
            "expected am-thin-walls (or snap) for thin wall FDM, got {:?}",
            hits.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn research_before_commit_is_searchable() {
        let store = HelpStore::bundled();
        let hits = store.search("research before commit verify table", Some(5));
        assert!(
            hits.iter().any(|h| h.id.contains("research-before-commit")),
            "expected research-before-commit, got {:?}",
            hits.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn fillet_vs_chamfer_hits_dedicated_page() {
        let store = HelpStore::bundled();
        let hits = store.search("fillet vs chamfer when to use", Some(5));
        assert!(
            hits.iter().any(|h| h.id.contains("fillet-chamfer")),
            "expected fillet-chamfer, got {:?}",
            hits.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn interference_check_hits_assembly_interference_page() {
        let store = HelpStore::bundled();
        for query in ["interference check", "assembly clearance"] {
            let hits = store.search(query, Some(5));
            assert!(!hits.is_empty(), "expected hits for {query}");
            assert!(
                hits.iter().any(|h| h.id.contains("assembly-interference")),
                "expected assembly-interference for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn validate_before_show_shot_pack_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "validate before show shot pack",
            "blank frame camera inside solid",
            "review PNG section cutaway",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("validate-before-show")),
                "expected validate-before-show for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn adversarial_mesh_wall_probe_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "adversarial mesh audit manifold",
            "wall probe thin wall seat",
            "printable solid non-manifold export preflight",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("adversarial-mesh-audit")),
                "expected adversarial-mesh-audit for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn alignment_nubs_vs_pins_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "alignment nubs vs pins",
            "locating pin dowel locator sock",
            "wedding-cake nub lofted cap",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("alignment-nubs-pins")),
                "expected alignment-nubs-pins for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn clamshell_retainer_slide_detent_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "clamshell retainer slide fit",
            "slide fit then detent clamp face",
            "retainer clip retention bump",
            "clamshell teeth cutout",
            "matching cutouts clip",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("am-clamshell-retainer")),
                "expected am-clamshell-retainer for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }

        for query in ["clamshell teeth cutout", "matching cutouts clip"] {
            let hits = store.search(query, Some(3));
            assert!(
                hits.first()
                    .is_some_and(|h| h.id.contains("am-clamshell-retainer")),
                "expected am-clamshell-retainer top for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn heat_set_insert_boss_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "heat-set insert boss crush ribs",
            "brass threaded insert FDM boss",
            "melt insert pilot hole",
            "heat stake boss",
            "heat-set boss",
            "heat stake insert",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("am-heat-set-inserts")),
                "expected am-heat-set-inserts for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }

        for query in ["heat stake boss", "heat-set boss", "heat stake insert"] {
            let hits = store.search(query, Some(3));
            assert!(
                hits.first()
                    .is_some_and(|h| h.id.contains("am-heat-set-inserts")),
                "expected am-heat-set-inserts top for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn fasteners_joints_preload_torque_searchable() {
        let store = HelpStore::bundled();
        for query in ["fastener preload", "bolt torque friction", "clamp load"] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("fasteners-joints")),
                "expected fasteners-joints for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn fastener_clearance_counterbore_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "fastener clearance counterbore",
            "tap drill vs heat-set insert",
            "screw clearance hole head recess",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("fastener-clearance-counterbore")),
                "expected fastener-clearance-counterbore for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn ribs_gussets_draft_am_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "ribs gussets draft AM",
            "stiffener rib FDM even wall",
            "boss gusset brace",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("am-ribs-gussets-draft")),
                "expected am-ribs-gussets-draft for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn bearing_seat_geometry_ranks_hubs_seats() {
        let store = HelpStore::bundled();
        for query in [
            "bearing seat",
            "bearing bore",
            "hub seat",
            "press fit hub bearing seat",
            "bearing bore journal shoulder",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("bearings-hubs-seats")),
                "expected bearings-hubs-seats for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
            assert!(
                hits[0].id.contains("bearings-hubs-seats"),
                "expected bearings-hubs-seats top for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn purchased_bearing_sku_ranks_stacks() {
        let store = HelpStore::bundled();
        for query in [
            "purchased bearing designation SKU shield seal",
            "bearing SKU collar envelope axial retention",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("bearing-stacks")),
                "expected bearing-stacks for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn press_fit_hub_bearing_seat_searchable() {
        let store = HelpStore::bundled();
        let seat_hits = store.search("press fit hub bearing seat", Some(5));
        assert!(
            seat_hits
                .iter()
                .any(|h| h.id.contains("bearings-hubs-seats")),
            "expected bearings-hubs-seats, got {:?}",
            seat_hits.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
        let sku_hits = store.search("hub lead-in shaft shoulder collar", Some(5));
        assert!(
            sku_hits
                .iter()
                .any(|h| h.id.contains("bearing-stacks") || h.id.contains("bearings-hubs-seats")),
            "expected bearing page for hub lead-in, got {:?}",
            sku_hits.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn locating_scheme_overconstraint_dof_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "overconstraint DOF locating scheme",
            "pin and slot primary secondary locator",
            "3-2-1 kinematic location",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("locating-scheme-dof")),
                "expected locating-scheme-dof for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn tolerance_stackup_intro_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "tolerance stack-up dimensional loop",
            "worst case stackup RSS",
            "assembly tolerance accumulation",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("tolerance-stackup-intro")),
                "expected tolerance-stackup-intro for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn supports_bridging_overhangs_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "support strategy overhang bridging",
            "FDM overhang angle self-supporting",
            "bridge length support cleanup",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("am-supports-overhangs")),
                "expected am-supports-overhangs for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn technic_lego_envelope_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "Technic Lego pin beam envelope",
            "LEGO compatible pin diameter pitch",
            "technic beam hole pitch unofficial",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("technic-envelope")),
                "expected technic-envelope for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn hardware_pocket_servo_bolt_circle_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "servo horn spline bolt circle PCD",
            "hardware pocket research actuator mount",
            "purchased flange bolt pattern VERIFY",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("am-hardware-pocket-research")),
                "expected am-hardware-pocket-research for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn cable_exits_strain_relief_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "wire window cable exit strain relief",
            "grommet cord grip AM enclosure",
            "jacket clamp wire channel FDM",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("am-cable-exits-strain-relief")),
                "expected am-cable-exits-strain-relief for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn captive_nut_hex_trap_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "captive nut hex nut trap",
            "printed hex pocket anti-rotation nut",
            "drop-in nut trap FDM",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("captive-nut-hex-trap")),
                "expected captive-nut-hex-trap for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn cosmetic_threads_vs_clearance_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "cosmetic thread modeled clearance helix",
            "CAD visual thread vs tap drill",
            "display helix not drill size",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("cosmetic-threads-vs-clearance")),
                "expected cosmetic-threads-vs-clearance for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn enclosure_lid_gasket_labyrinth_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "enclosure lid gasket labyrinth seal",
            "tongue groove dust seal FDM lid",
            "O-ring groove gasket seat enclosure",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("am-enclosure-lid-gasket-labyrinth")),
                "expected am-enclosure-lid-gasket-labyrinth for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn ventilation_grille_finger_trap_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "ventilation grille finger trap",
            "FDM vent slots louvers finger guard",
            "grille bar pitch airflow opening",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("am-ventilation-grille-finger-trap")),
                "expected am-ventilation-grille-finger-trap for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn boss_standoff_patterns_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "boss-to-boss standoff pattern PCB",
            "FDM standoff height mounting boss grid",
            "PCB standoff boss pair screw roles",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("am-boss-standoff-patterns")),
                "expected am-boss-standoff-patterns for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn assembly_join_choice_when_not_to_snap_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "when not to snap glue screw assembly",
            "solvent weld vs screw vs snap FDM",
            "ultrasonic plastic join choice AM enclosure",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("am-assembly-join-choice")),
                "expected am-assembly-join-choice for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn warpage_cooling_flatness_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "warpage cooling flatness large plate",
            "FDM plate curl dish flatness",
            "large enclosure base warp residual stress",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("am-warpage-cooling-flatness")),
                "expected am-warpage-cooling-flatness for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn fit_coupons_recipes_map_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "fit coupons recipes map related_recipes",
            "turbine-fit-coupons recipe hub",
            "coupon map help recipes demos",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("fit-coupons-recipes-map")),
                "expected fit-coupons-recipes-map for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn interference_fit_hits_fits_not_only_assembly_check() {
        let store = HelpStore::bundled();
        for query in [
            "interference fit shaft hole",
            "clearance fit running sliding",
            "press fit class allowance",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("fits-clearances")),
                "expected fits-clearances for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
            if query.starts_with("interference fit") {
                assert!(
                    hits[0].id.contains("fits-clearances"),
                    "fits-clearances should top interference fit query, got {:?}",
                    hits.iter().map(|h| &h.id).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn datum_sketch_plane_mcp_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "datum sketch plane coordinate system MCP",
            "sketch plane choice origin XY",
            "datum_plane_create offset plane",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("datum-sketch-plane-choice")),
                "expected datum-sketch-plane-choice for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn hole_wizard_vs_modeled_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "hole wizard vs modeled hole",
            "solid_edit_hole hole pattern bolt circle",
            "simple hole pattern clearance positions",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("hole-wizard-vs-modeled")),
                "expected hole-wizard-vs-modeled for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn power_screws_lead_screws_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "power screws lead screws pitch backdrive",
            "lead screw printed wear nut vise",
            "Acme trapezoidal power screw CAD-time",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("power-screws-lead-screws")),
                "expected power-screws-lead-screws for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn shafts_keys_retaining_rings_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "shafts keys retaining rings circlip",
            "keyseat keyway parallel key shaft shoulder",
            "retaining ring groove axial retention",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("shafts-keys-retaining-rings")),
                "expected shafts-keys-retaining-rings for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn springs_couplings_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "springs couplings free length solid height",
            "jaw coupling Oldham misalignment hub bore",
            "compression spring seat OD clearance",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("springs-couplings")),
                "expected springs-couplings for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn drawing_vs_mbd_pmi_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "drawing vs MBD PMI model-based definition",
            "PMI annotations datum feature control frame Y14.41",
            "2D drawing notes vs semantic PMI manufacturing",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("drawing-vs-mbd-pmi")),
                "expected drawing-vs-mbd-pmi for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn dfam_fdm_overview_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "DFAM FDM design for additive",
            "FDM design additive manufacturing overview",
            "design for additive FDM golden path",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("dfam-fdm-overview")),
                "expected dfam-fdm-overview for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn am_fdm_holes_fit_allowances_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "printed hole shrink FDM",
            "FDM hole clearance locate press allowance",
            "XY shrink printed fit coupon hole",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("am-fdm-holes-fit-allowances")),
                "expected am-fdm-holes-fit-allowances for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn am_fdm_load_layers_infill_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "FDM load path layer orientation",
            "shells vs infill structural roles",
            "bed face tension in-plane layers anisotropy",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("am-fdm-load-layers-infill")),
                "expected am-fdm-load-layers-infill for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn mechanisms_overview_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "mechanisms overview motion class",
            "rotary to linear mechanism hub",
            "mechanism element family envelopes DOF",
            "navigate mechanism soft sync CAD owns",
            "CAD owns centers catalog tooth cam belt",
            "motion class element family purchased indexer",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("mechanisms-overview")),
                "expected mechanisms-overview for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn mechanisms_linkages_mobility_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "four bar linkage mobility",
            "slider-crank joints DOF Gruebler",
            "linkage revolute prismatic mobility",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("mechanisms-linkages-mobility")),
                "expected mechanisms-linkages-mobility for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn mechanisms_cams_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "cam follower rise dwell return",
            "base circle pressure angle cam",
            "roller follower plate cam",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("mechanisms-cams")),
                "expected mechanisms-cams for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn mechanisms_belts_pulleys_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "timing belt center distance",
            "belt pulley wrap idler tension",
            "GT2 HTD purchased belt profile",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("mechanisms-belts-pulleys")),
                "expected mechanisms-belts-pulleys for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn mechanisms_chains_sprockets_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "roller chain center distance",
            "chain sprocket wrap idler tension",
            "purchased chain pitch sprocket",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("mechanisms-chains-sprockets")),
                "expected mechanisms-chains-sprockets for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn am_printed_gears_dfam_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "printed gear FDM orientation tooth",
            "min tooth thickness nozzle backlash coupon",
            "DFAM printed spur gear layer load",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("am-printed-gears-dfam")),
                "expected am-printed-gears-dfam for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn mechanisms_intermittent_geneva_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "Geneva drive intermittent index dwell",
            "lock arc driver pin Geneva wheel",
            "purchased indexer intermittent motion",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("mechanisms-intermittent-geneva")),
                "expected mechanisms-intermittent-geneva for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn bearings_hubs_seats_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "bearing shaft housing seat preload",
            "L10 life load speed VERIFY catalog",
            "inner ring outer ring spacer stack fit roles",
            "bearing seat",
            "hub seat housing bore",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("bearings-hubs-seats")),
                "expected bearings-hubs-seats for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn materials_vocabulary_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "materials vocabulary E Sy Sut hardness",
            "print vs isotropic anisotropy filament allowable",
            "educational range not allowable materials datasheet",
            "elastic modulus yield ultimate CAD materials",
            "hardness not substitute for Sy grade temper",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("materials-vocabulary")),
                "expected materials-vocabulary for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn design_hygiene_requirements_bom_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "requirements embodiment BOM purchased",
            "purchased vs print BOM roles",
            "make vs buy COTS SKU freeze geometry",
            "reference designator BOM phantom",
            "VERIFY table freeze mates purchased",
            "standardize fastener kit BOM",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("design-hygiene-requirements-bom")),
                "expected design-hygiene-requirements-bom for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn inspection_metrology_bridge_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "inspection metrology bridge",
            "CMM gage characteristic datum scheme handoff",
            "FAIR balloon MBD to shop inspection",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("inspection-metrology-bridge")),
                "expected inspection-metrology-bridge for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn agent_mcp_workflow_ops_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "cad_help tenacity soft focus",
            "inspect between mutates solid_scene",
            "cad_attach headless sessions",
            "solid_edit fillet topology face_id",
            "unit systems mm default",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("agent-mcp-workflow")),
                "expected agent-mcp-workflow for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn export_print_3mf_stl_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "export preflight 3MF vs STL",
            "3MF print package mesh preflight",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("export-print")),
                "expected export-print for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn design_version_scripts_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "design VERSION JSONC script naming design_v",
            "VERSION DESIGN_VERSION design_vM_N.limo.jsonc",
            "prune prior design_v nbcad.jsonc VERSION metadata",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("design-version-scripts")),
                "expected design-version-scripts for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn geometry_naming_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "name bodies faces script",
            "geometry naming STEP",
            "JSONC body names",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter().any(|h| h.id.contains("geometry-naming")),
                "expected geometry-naming for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn shared_reference_geometry_searchable() {
        let store = HelpStore::bundled();
        for query in [
            "shared reference geometry",
            "surfaces follow param change",
            "offset from named plane",
        ] {
            let hits = store.search(query, Some(5));
            assert!(
                hits.iter()
                    .any(|h| h.id.contains("shared-reference-geometry")),
                "expected shared-reference-geometry for {query}, got {:?}",
                hits.iter().map(|h| &h.id).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn snippet_keeps_query_context_after_multibyte_text() {
        let mut body = "a".repeat(300);
        body.push('→');
        body.push_str(&"b".repeat(38));
        body.push_str(" clearance ");
        body.push_str(&"c".repeat(400));
        let snippet = make_snippet(&body, &["clearance".to_string()], SNIPPET_CHARS);
        assert!(snippet.contains("clearance"), "{snippet}");
        assert!(snippet.chars().count() <= SNIPPET_CHARS + 1);
    }

    #[test]
    fn embedded_files_match_every_repository_markdown_file() {
        fn collect(
            root: &std::path::Path,
            dir: &std::path::Path,
            out: &mut BTreeMap<String, String>,
        ) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    collect(root, &path, out);
                } else if path.extension().is_some_and(|ext| ext == "md") {
                    let relative = path
                        .strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .replace('\\', "/");
                    out.insert(relative, std::fs::read_to_string(&path).unwrap());
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../knowledge");
        let mut expected = BTreeMap::new();
        collect(&root, &root, &mut expected);
        let embedded: BTreeMap<String, String> = knowledge_files()
            .iter()
            .map(|file| (file.path.to_string(), file.text.to_string()))
            .collect();
        assert_eq!(embedded, expected);
    }

    #[test]
    fn check_corpus_ok() {
        let n = check_corpus().expect("corpus check");
        assert!(n >= 50);
    }

    #[test]
    fn knowledge_resources_include_index_and_concepts() {
        let files = knowledge_files();
        assert!(files.iter().any(|f| f.path == "index.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/fits-clearances.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-snap-fit.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "concepts/research-before-commit.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "concepts/assembly-interference.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "concepts/validate-before-show.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "concepts/adversarial-mesh-audit.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "concepts/design-version-scripts.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "concepts/geometry-naming.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "concepts/shared-reference-geometry.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/alignment-nubs-pins.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-clamshell-retainer.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-heat-set-inserts.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/fastener-clearance-counterbore.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-ribs-gussets-draft.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/locating-scheme-dof.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/tolerance-stackup-intro.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-supports-overhangs.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/technic-envelope.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-hardware-pocket-research.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-cable-exits-strain-relief.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/captive-nut-hex-trap.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/cosmetic-threads-vs-clearance.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-enclosure-lid-gasket-labyrinth.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-ventilation-grille-finger-trap.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-boss-standoff-patterns.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-assembly-join-choice.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-warpage-cooling-flatness.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/fit-coupons-recipes-map.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/datum-sketch-plane-choice.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/hole-wizard-vs-modeled.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/power-screws-lead-screws.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/shafts-keys-retaining-rings.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/springs-couplings.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/drawing-vs-mbd-pmi.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/dfam-fdm-overview.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-fdm-holes-fit-allowances.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-fdm-load-layers-infill.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/mechanisms-overview.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/mechanisms-linkages-mobility.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/mechanisms-cams.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/mechanisms-belts-pulleys.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/mechanisms-chains-sprockets.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/am-printed-gears-dfam.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/mechanisms-intermittent-geneva.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/design-hygiene-requirements-bom.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/bearings-hubs-seats.md"));
        assert!(files
            .iter()
            .any(|f| f.path == "machine-design/concepts/inspection-metrology-bridge.md"));
        let index = knowledge_file_by_uri("limo-cad://knowledge/index.md").expect("index uri");
        assert!(index.text.contains("Open Knowledge Format"));
        assert!(knowledge_file_by_uri("limo-cad://knowledge/../etc/passwd").is_none());
        assert!(knowledge_file_by_uri("limo-cad://other/index.md").is_none());

        let store = HelpStore::bundled();
        assert!(store.catalog().ids().all(|id| !id.ends_with("index")));
        assert!(store
            .catalog()
            .ids()
            .any(|id| id.contains("fits-clearances")));
    }
}
