use chrono::Utc;
use sha2::{Digest, Sha256};

/// Closed vocabulary for entity-level warning kinds (Issue 8).
pub const WARNING_KINDS: &[&str] = &[
    "source_conflict",
    "source_anomaly",
    "source_gap",
    "extraction",
    "integrity",
    "coverage",
];

#[derive(Debug, Clone)]
pub struct CheckDetail {
    pub check_id: String,
    pub severity: String,
    pub status: String,
    pub session_id: String,
    pub meeting_kind: String,
    pub meeting_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub expected: String,
    pub actual: String,
    pub message: String,
    pub source_url: String,
    pub cache_path: String,
    pub source_block: String,
    pub created_at: String,
    /// Deterministic id over check subject/values/artifact/block (excludes `created_at`).
    pub warning_id: String,
    /// Closed vocabulary: source_conflict, source_anomaly, source_gap, extraction, integrity, coverage.
    pub warning_kind: String,
    /// Exact graph node type when this detail targets a graph entity (e.g. `VoteResult`).
    pub graph_node_type: String,
    /// Exact graph node id (e.g. `56-135-r16`). Never a source-local id like `1#1`.
    pub graph_node_id: String,
    /// Canonical `crawl::artifact_id(source_url, cache_path)` when provenance is known.
    pub source_artifact_id: String,
}

impl CheckDetail {
    pub fn new(
        check_id: impl Into<String>,
        severity: impl Into<String>,
        status: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            check_id: check_id.into(),
            severity: severity.into(),
            status: status.into(),
            session_id: String::new(),
            meeting_kind: String::new(),
            meeting_id: String::new(),
            entity_type: String::new(),
            entity_id: String::new(),
            expected: String::new(),
            actual: String::new(),
            message: message.into(),
            source_url: String::new(),
            cache_path: String::new(),
            source_block: String::new(),
            created_at: Utc::now().to_rfc3339(),
            warning_id: String::new(),
            warning_kind: String::new(),
            graph_node_type: String::new(),
            graph_node_id: String::new(),
            source_artifact_id: String::new(),
        }
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = session_id.into();
        self
    }

    pub fn with_meeting(
        mut self,
        meeting_kind: impl Into<String>,
        meeting_id: impl Into<String>,
    ) -> Self {
        self.meeting_kind = meeting_kind.into();
        self.meeting_id = meeting_id.into();
        self
    }

    pub fn with_entity(
        mut self,
        entity_type: impl Into<String>,
        entity_id: impl Into<String>,
    ) -> Self {
        self.entity_type = entity_type.into();
        self.entity_id = entity_id.into();
        self
    }

    pub fn with_values(mut self, expected: impl Into<String>, actual: impl Into<String>) -> Self {
        self.expected = expected.into();
        self.actual = actual.into();
        self
    }

    pub fn with_source(
        mut self,
        source_url: impl Into<String>,
        cache_path: impl Into<String>,
    ) -> Self {
        self.source_url = source_url.into();
        self.cache_path = cache_path.into();
        self
    }

    pub fn with_source_block(mut self, source_block: impl Into<String>) -> Self {
        self.source_block = source_block.into();
        self
    }

    pub fn with_warning_kind(mut self, warning_kind: impl Into<String>) -> Self {
        self.warning_kind = warning_kind.into();
        self
    }

    pub fn with_graph_node(
        mut self,
        graph_node_type: impl Into<String>,
        graph_node_id: impl Into<String>,
    ) -> Self {
        self.graph_node_type = graph_node_type.into();
        self.graph_node_id = graph_node_id.into();
        self
    }

    pub fn with_source_artifact_id(mut self, source_artifact_id: impl Into<String>) -> Self {
        self.source_artifact_id = source_artifact_id.into();
        self
    }

    /// Fill `source_artifact_id` (when URL/cache present) and deterministic `warning_id`.
    pub fn finalize(mut self) -> Self {
        if self.source_artifact_id.is_empty()
            && (!self.source_url.is_empty() || !self.cache_path.is_empty())
        {
            self.source_artifact_id = crawl::artifact_id(&self.source_url, &self.cache_path);
        }
        self.warning_id = compute_warning_id(&self);
        self
    }
}

/// Deterministic warning id excluding `created_at`.
pub fn compute_warning_id(detail: &CheckDetail) -> String {
    let mut hasher = Sha256::new();
    for part in [
        detail.check_id.as_str(),
        detail.severity.as_str(),
        detail.status.as_str(),
        detail.session_id.as_str(),
        detail.meeting_kind.as_str(),
        detail.meeting_id.as_str(),
        detail.entity_type.as_str(),
        detail.entity_id.as_str(),
        detail.expected.as_str(),
        detail.actual.as_str(),
        detail.message.as_str(),
        detail.source_url.as_str(),
        detail.cache_path.as_str(),
        detail.source_block.as_str(),
        detail.warning_kind.as_str(),
        detail.graph_node_type.as_str(),
        detail.graph_node_id.as_str(),
        detail.source_artifact_id.as_str(),
    ] {
        hasher.update(part.as_bytes());
        hasher.update(b"|");
    }
    format!("{:x}", hasher.finalize())
}

pub fn finalize_details(details: Vec<CheckDetail>) -> Vec<CheckDetail> {
    details.into_iter().map(CheckDetail::finalize).collect()
}

#[derive(Debug, Clone)]
pub struct MeetingCoverageSnapshot {
    pub meeting_kind: String,
    pub meeting_id: String,
    pub source_words: usize,
    pub saved_words: usize,
    pub ratio: f64,
    pub updated_at: String,
}

/// Committed regression snapshots use the same shape as a run-time coverage snapshot.
pub type CoverageBaselineRow = MeetingCoverageSnapshot;

#[derive(Debug, Clone)]
pub struct CheckSummary {
    pub table: String,
    pub check: String,
    pub status: String,
    pub count: usize,
    pub detail: String,
    pub examples: String,
}

#[derive(Debug, Clone)]
pub struct AliasCandidate {
    pub raw_name: String,
    pub cleaned_name: String,
    pub matched_person_id: String,
    pub source_bucket: String,
    pub context_id: String,
    pub check_id: String,
    pub confidence: String,
}

pub fn table_for_check_id(check_id: &str) -> String {
    check_id.split('.').next().unwrap_or("qa").to_string()
}

pub fn status_rank(status: &str) -> u8 {
    match status {
        "error" | "fail" => 4,
        "warn" => 3,
        "info" => 2,
        "pass" => 1,
        _ => 0,
    }
}

pub fn worst_status<'a>(statuses: impl Iterator<Item = &'a str>) -> &'static str {
    let mut worst = "pass";
    for s in statuses {
        if status_rank(s) > status_rank(worst) {
            worst = match s {
                "error" | "fail" => "fail",
                "warn" => "warn",
                "info" => "info",
                _ => worst,
            };
        }
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_id_stable_and_ignores_created_at() {
        let a = CheckDetail::new("vote.compact_total_vs_member_names", "warn", "warn", "msg")
            .with_entity("vote_result", "56-135-r16")
            .with_graph_node("VoteResult", "56-135-r16")
            .with_warning_kind("source_conflict")
            .with_values("a", "b")
            .with_source("https://example.com", "cache/x.html")
            .finalize();
        let mut b = a.clone();
        b.created_at = "different".into();
        assert_eq!(a.warning_id, compute_warning_id(&b));
        assert!(!a.warning_id.is_empty());
        assert!(!a.source_artifact_id.is_empty());
    }

    #[test]
    fn source_local_ids_are_not_graph_ids() {
        let d = CheckDetail::new("vote.appendix_bucket_counts", "warn", "warn", "msg")
            .with_entity("vote_result", "16#1")
            .finalize();
        assert!(d.graph_node_id.is_empty());
        assert_eq!(d.entity_id, "16#1");
    }
}
