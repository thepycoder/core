use crawl::artifact_id as crawl_artifact_id;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct ArtifactEntry {
    pub source_url: String,
    pub cache_path: String,
    /// Content hash recorded at normalize/transform time, when known.
    pub transform_content_hash: Option<String>,
}

pub fn artifact_id(source_url: &str, cache_path: &str) -> String {
    crawl_artifact_id::artifact_id(source_url, cache_path)
}

pub fn register_artifact(
    registry: &mut HashMap<String, ArtifactEntry>,
    source_url: &str,
    cache_path: &str,
) -> String {
    register_artifact_with_hash(registry, source_url, cache_path, None)
}

pub fn register_artifact_with_hash(
    registry: &mut HashMap<String, ArtifactEntry>,
    source_url: &str,
    cache_path: &str,
    transform_content_hash: Option<&str>,
) -> String {
    let id = artifact_id(source_url, cache_path);
    let entry = registry.entry(id.clone()).or_insert_with(|| ArtifactEntry {
        source_url: source_url.to_string(),
        cache_path: cache_path.to_string(),
        transform_content_hash: None,
    });
    if let Some(hash) = transform_content_hash.filter(|h| !h.is_empty()) {
        if entry.transform_content_hash.is_none() {
            entry.transform_content_hash = Some(hash.to_string());
        }
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_id_matches_crawl() {
        let url = "https://example.com";
        let path = "sessions/56/meetings/plenary/56-60.html";
        assert_eq!(artifact_id(url, path), crawl::artifact_id(url, path));
    }

    #[test]
    fn register_records_transform_hash() {
        let mut reg = HashMap::new();
        let id = register_artifact_with_hash(&mut reg, "https://x", "a.html", Some("abc"));
        assert_eq!(reg[&id].transform_content_hash.as_deref(), Some("abc"));
    }
}
