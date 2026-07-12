//! Shared speech / vote-appendix zone predicates for segmentation and QA coverage.

use regex::Regex;
use std::sync::OnceLock;

static HARD_BOUNDARY: OnceLock<Regex> = OnceLock::new();
static VOTE_APPENDIX: OnceLock<Regex> = OnceLock::new();
static STAGE_DIRECTION: OnceLock<Regex> = OnceLock::new();
static VOTE_APPENDIX_BUCKET: OnceLock<Regex> = OnceLock::new();

pub fn hard_boundary_regex() -> &'static Regex {
    HARD_BOUNDARY.get_or_init(|| {
        Regex::new(r"(?i)(?:Het incident is gesloten|L'incident est clos|DETAIL VAN DE NAAMSTEMMINGEN)")
            .unwrap()
    })
}

pub fn vote_appendix_regex() -> &'static Regex {
    VOTE_APPENDIX.get_or_init(|| Regex::new(r"(?i)DETAIL VAN DE NAAMSTEMMINGEN").unwrap())
}

pub fn stage_direction_regex() -> &'static Regex {
    STAGE_DIRECTION.get_or_init(|| {
        Regex::new(
            r"(?i)^(?:Hervatting van de algemene bespreking|Reprise de la discussion générale)$",
        )
        .unwrap()
    })
}

pub fn vote_appendix_bucket_regex() -> &'static Regex {
    VOTE_APPENDIX_BUCKET.get_or_init(|| {
        Regex::new(r"(?i)Naamstemming\s*-\s*Vote\s*nominatif\s*:\s*\d+").unwrap()
    })
}

pub fn is_vote_appendix_heading(text: &str) -> bool {
    vote_appendix_regex().is_match(text)
}

pub fn is_hard_boundary(text: &str) -> bool {
    hard_boundary_regex().is_match(text)
}

pub fn is_stage_direction(text: &str) -> bool {
    stage_direction_regex().is_match(text)
}

pub fn is_vote_appendix_bucket_header(text: &str) -> bool {
    vote_appendix_bucket_regex().is_match(text)
}
