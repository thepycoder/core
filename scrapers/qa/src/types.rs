use chrono::Utc;

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
