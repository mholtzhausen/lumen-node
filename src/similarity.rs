//! In-memory prompt-token similarity for “Similar in folder” browse.

use crate::metadata::ImageMetadata;
use crate::metadata_view::extract_seed_from_parameters;
use std::collections::{HashMap, HashSet};

/// Max similar paths returned (excluding the query path, which is always included).
pub const SIMILAR_TOP_N: usize = crate::config::DEFAULT_SIMILAR_TOP_N as usize;
/// Minimum Jaccard score (before seed boost) to keep a candidate. `> 0` requires overlap.
pub const SIMILAR_MIN_SCORE: f64 = 0.0;
/// Added when two images share the same extracted seed.
pub const SEED_SCORE_BOOST: f64 = 0.25;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptIndexEntry {
    pub tokens: HashSet<String>,
    pub seed: Option<String>,
}

/// Lowercase, split on non-alphanumeric, drop tokens shorter than 3 characters.
pub fn normalize_prompt_tokens(text: &str) -> HashSet<String> {
    let mut tokens = HashSet::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            if current.len() >= 3 {
                tokens.insert(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= 3 {
        tokens.insert(current);
    }
    tokens
}

pub fn jaccard_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count();
    let union = a.len() + b.len() - inter;
    if union == 0 {
        0.0
    } else {
        inter as f64 / union as f64
    }
}

fn prompt_or_parameters_text(meta: &ImageMetadata) -> Option<&str> {
    meta.prompt
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            meta.raw_parameters
                .as_deref()
                .filter(|s| !s.trim().is_empty())
        })
}

pub fn meta_has_similarity_source(meta: &ImageMetadata) -> bool {
    prompt_or_parameters_text(meta).is_some()
}

pub fn entry_from_meta(meta: &ImageMetadata) -> Option<PromptIndexEntry> {
    let text = prompt_or_parameters_text(meta)?;
    Some(PromptIndexEntry {
        tokens: normalize_prompt_tokens(text),
        seed: extract_seed_from_parameters(meta),
    })
}

/// Insert or remove a path in the similarity index from enriched metadata.
pub fn upsert_prompt_index(
    index: &mut HashMap<String, PromptIndexEntry>,
    path: &str,
    meta: &ImageMetadata,
) {
    match entry_from_meta(meta) {
        Some(entry) => {
            index.insert(path.to_string(), entry);
        }
        None => {
            index.remove(path);
        }
    }
}

/// Move an index entry from `old_path` to `new_path` (no-op if missing).
pub fn rekey_prompt_index(
    index: &mut HashMap<String, PromptIndexEntry>,
    old_path: &str,
    new_path: &str,
) {
    if old_path == new_path {
        return;
    }
    if let Some(entry) = index.remove(old_path) {
        index.insert(new_path.to_string(), entry);
    }
}

/// Rank other indexed images by Jaccard token overlap (plus seed boost).
/// Always includes `query_path` in the result set when the query is indexed.
pub fn find_similar_paths(
    index: &HashMap<String, PromptIndexEntry>,
    query_path: &str,
    top_n: usize,
    min_score: f64,
) -> Option<HashSet<String>> {
    let query = index.get(query_path)?;
    let mut scored: Vec<(String, f64)> = Vec::new();
    for (path, entry) in index {
        if path == query_path {
            continue;
        }
        let mut score = jaccard_similarity(&query.tokens, &entry.tokens);
        if query.seed.is_some() && query.seed == entry.seed {
            score += SEED_SCORE_BOOST;
        }
        if score > min_score {
            scored.push((path.clone(), score));
        }
    }
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    let mut result: HashSet<String> = scored.into_iter().take(top_n).map(|(p, _)| p).collect();
    result.insert(query_path.to_string());
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::ImageMetadata;

    #[test]
    fn normalize_drops_short_and_splits() {
        let tokens = normalize_prompt_tokens("A cat, 42! Ultra-detailed portrait");
        assert!(tokens.contains("cat"));
        assert!(!tokens.contains("a"));
        assert!(!tokens.contains("42"));
        assert!(tokens.contains("ultra"));
        assert!(tokens.contains("detailed"));
        assert!(tokens.contains("portrait"));
    }

    #[test]
    fn jaccard_identical_and_disjoint() {
        let a: HashSet<_> = ["cat", "dog", "bird"].into_iter().map(String::from).collect();
        let b = a.clone();
        assert!((jaccard_similarity(&a, &b) - 1.0).abs() < f64::EPSILON);
        let c: HashSet<_> = ["fish", "tree"].into_iter().map(String::from).collect();
        assert_eq!(jaccard_similarity(&a, &c), 0.0);
        let empty = HashSet::new();
        assert_eq!(jaccard_similarity(&empty, &empty), 0.0);
    }

    #[test]
    fn find_similar_ranks_overlap_and_includes_self() {
        let mut index = HashMap::new();
        index.insert(
            "a.png".into(),
            PromptIndexEntry {
                tokens: ["red", "apple", "tree"].into_iter().map(String::from).collect(),
                seed: Some("1".into()),
            },
        );
        index.insert(
            "b.png".into(),
            PromptIndexEntry {
                tokens: ["red", "apple", "leaf"].into_iter().map(String::from).collect(),
                seed: Some("1".into()),
            },
        );
        index.insert(
            "c.png".into(),
            PromptIndexEntry {
                tokens: ["blue", "ocean"].into_iter().map(String::from).collect(),
                seed: Some("99".into()),
            },
        );
        let result = find_similar_paths(&index, "a.png", 50, 0.0).unwrap();
        assert!(result.contains("a.png"));
        assert!(result.contains("b.png"));
        assert!(!result.contains("c.png"));
    }

    #[test]
    fn entry_from_meta_uses_prompt_then_parameters() {
        let mut meta = ImageMetadata::default();
        meta.prompt = Some("masterpiece best quality".into());
        meta.raw_parameters = Some("Steps: 20, Seed: 12345, Sampler: Euler".into());
        let entry = entry_from_meta(&meta).unwrap();
        assert!(entry.tokens.contains("masterpiece"));
        assert_eq!(entry.seed.as_deref(), Some("12345"));

        let mut params_only = ImageMetadata::default();
        params_only.raw_parameters = Some("Seed: 99, Steps: 30".into());
        let entry2 = entry_from_meta(&params_only).unwrap();
        assert!(entry2.tokens.contains("seed"));
        assert_eq!(entry2.seed.as_deref(), Some("99"));
    }

    #[test]
    fn rekey_prompt_index_moves_entry() {
        let mut index = HashMap::new();
        index.insert(
            "old.png".into(),
            PromptIndexEntry {
                tokens: ["red", "apple"].into_iter().map(String::from).collect(),
                seed: Some("7".into()),
            },
        );
        rekey_prompt_index(&mut index, "old.png", "new.png");
        assert!(!index.contains_key("old.png"));
        let entry = index.get("new.png").expect("rekeyed");
        assert!(entry.tokens.contains("red"));
        assert_eq!(entry.seed.as_deref(), Some("7"));

        rekey_prompt_index(&mut index, "missing.png", "other.png");
        assert!(!index.contains_key("other.png"));
        assert!(index.contains_key("new.png"));
    }

    #[test]
    fn upsert_prompt_index_inserts_and_removes() {
        let mut index = HashMap::new();
        let mut meta = ImageMetadata::default();
        meta.prompt = Some("cyberpunk city street".into());
        upsert_prompt_index(&mut index, "a.png", &meta);
        assert!(index.contains_key("a.png"));

        let empty = ImageMetadata::default();
        upsert_prompt_index(&mut index, "a.png", &empty);
        assert!(!index.contains_key("a.png"));
    }
}
