use crawl::artifact_id as crawl_artifact_id;
use std::collections::HashMap;

pub fn artifact_id(source_url: &str, cache_path: &str) -> String {
    crawl_artifact_id::artifact_id(source_url, cache_path)
}

pub fn register_artifact(
    registry: &mut HashMap<String, (String, String)>,
    source_url: &str,
    cache_path: &str,
) -> String {
    let id = artifact_id(source_url, cache_path);
    registry
        .entry(id.clone())
        .or_insert_with(|| (source_url.to_string(), cache_path.to_string()));
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
}
