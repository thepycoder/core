//! Deterministic artifact identity from source URL and cache path.

use sha2::{Digest, Sha256};

pub const BLOCK_PARSER_VERSION: &str = "report_blocks_v2";
pub const REPORT_BLOCK_EXTRACTOR_VERSION: &str = "report_blocks_materialize_v1";
pub const VOTE_EXTRACTOR_VERSION: &str = "vote_assembly_v1";
pub const MEETING_SCOPE_EXTRACTOR_VERSION: &str = "meeting_parse_v2";

/// Content-independent SHA-256 hex id from provenance keys.
pub fn artifact_id(source_url: &str, cache_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_url.as_bytes());
    hasher.update(b"|");
    hasher.update(cache_path.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn content_hash(text: &str) -> String {
    content_hash_bytes(text.as_bytes())
}

pub fn content_hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_id_is_stable() {
        let a = artifact_id("https://example.com/a", "cache/x.html");
        let b = artifact_id("https://example.com/a", "cache/x.html");
        assert_eq!(a, b);
        assert_ne!(a, artifact_id("https://example.com/b", "cache/x.html"));
    }
}
