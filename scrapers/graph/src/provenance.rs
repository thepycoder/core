use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

pub fn artifact_id(source_url: &str, cache_path: &str) -> String {
    let mut hasher = DefaultHasher::new();
    source_url.hash(&mut hasher);
    cache_path.hash(&mut hasher);
    format!("art_{:016x}", hasher.finish())
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
    fn artifact_id_is_stable() {
        let a = artifact_id("https://example.com", "path/to.html");
        let b = artifact_id("https://example.com", "path/to.html");
        assert_eq!(a, b);
        assert!(a.starts_with("art_"));
    }
}
