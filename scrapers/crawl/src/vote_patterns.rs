//! Centrally catalogued bilingual vote section markers, table labels, and outcome phrases.
//!
//! Reference meetings in session 56 plenary cache — see `tests/fixtures/votes/`.

use regex::Regex;
use std::sync::OnceLock;

static COMPACT_VOTE: OnceLock<Regex> = OnceLock::new();
static APPENDIX_VOTE: OnceLock<Regex> = OnceLock::new();
static APPENDIX_VOTE_REVERSE: OnceLock<Regex> = OnceLock::new();
static PARAGRAPH_VOTE: OnceLock<Regex> = OnceLock::new();
static REUSE_RESULT: OnceLock<Regex> = OnceLock::new();
static SITTING_STANDING_OUTCOME: OnceLock<Regex> = OnceLock::new();
static QUORUM_FAILURE: OnceLock<Regex> = OnceLock::new();
static CANDIDATE_TALLY: OnceLock<Regex> = OnceLock::new();
static DOSSIER_REF: OnceLock<Regex> = OnceLock::new();
static FORMAL_VOTE_BEGIN: OnceLock<Regex> = OnceLock::new();
static PARTICIPATION_COUNT: OnceLock<Regex> = OnceLock::new();

pub fn compact_vote_re() -> &'static Regex {
    COMPACT_VOTE.get_or_init(|| Regex::new(r"(?i)\(\s*Stemming\s*/\s*vote\s+(\d+)\s*\)").unwrap())
}

pub fn appendix_vote_re() -> &'static Regex {
    APPENDIX_VOTE
        .get_or_init(|| Regex::new(r"(?i)Naamstemming\s*-\s*Vote\s*nominatif\s*:\s*(\d+)").unwrap())
}

pub fn appendix_vote_reverse_re() -> &'static Regex {
    APPENDIX_VOTE_REVERSE
        .get_or_init(|| Regex::new(r"(?i)Vote\s*nominatif\s*-\s*Naamstemming\s*:\s*(\d+)").unwrap())
}

pub fn paragraph_vote_re() -> &'static Regex {
    PARAGRAPH_VOTE.get_or_init(|| Regex::new(r"(?i)\(\s*Stemming\s*/\s*vote\s+(\d+)\s*\)").unwrap())
}

/// Result reuse — meeting 102/127 cache.
pub fn reuse_result_re() -> &'static Regex {
    REUSE_RESULT.get_or_init(|| {
        Regex::new(
            r"(?i)(?:mag\s+de\s+uitslag\s+van\s+de\s+vorige\s+stemming|le\s+résultat\s+du\s+précédent\s+vote)",
        )
        .unwrap()
    })
}

/// Formal sitting/standing adoption/rejection — meetings 15, 131, 133.
pub fn sitting_standing_outcome_re() -> &'static Regex {
    SITTING_STANDING_OUTCOME.get_or_init(|| {
        Regex::new(
            r"(?i)(?:bij\s+zitten\s+en\s+opstaan|par\s+assis\s+et\s+levé).{0,80}(?:aangenomen|verworpen|adoptée|adopté|rejetée|rejeté)",
        )
        .unwrap()
    })
}

/// Quorum failure prose — meeting 110.
pub fn quorum_failure_re() -> &'static Regex {
    QUORUM_FAILURE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:het\s+)?quorum\s+is\s+niet\s+bereikt|le\s+quorum\s+n['']est\s+pas\s+atteint",
        )
        .unwrap()
    })
}

/// Candidate tally after secret ballot — meetings 14, 16.
pub fn candidate_tally_re() -> &'static Regex {
    CANDIDATE_TALLY.get_or_init(|| {
        Regex::new(
            r"(?i)(?:de\s+(?:heer|mevrouw)|m\.|mme\.?)\s+(.+?)\s+(?:heeft|a\s+obtenu)\s+(\d+)\s+(?:stemmen|voix)(?:\s+gekregen)?",
        )
        .unwrap()
    })
}

/// Formal vote sequence markers — meeting 110 (no naamstemmingen heading).
pub fn formal_vote_begin_re() -> &'static Regex {
    FORMAL_VOTE_BEGIN
        .get_or_init(|| Regex::new(r"(?i)begin van de stemming|début du vote").unwrap())
}

/// Participation counts on quorum failure — meeting 110: `65/71 leden hebben deelgenomen`.
pub fn participation_count_re() -> &'static Regex {
    PARTICIPATION_COUNT.get_or_init(|| {
        Regex::new(
            r"(?i)(\d+)(?:\s*/\s*(\d+))?\s*(?:leden|membres).{0,60}(?:deelgenomen|pris part)",
        )
        .unwrap()
    })
}

pub fn parse_participation(text: &str) -> Option<(u32, Option<u32>)> {
    participation_count_re().captures(text).map(|caps| {
        let participated: u32 = caps[1].parse().unwrap_or(0);
        let required = caps.get(2).and_then(|m| m.as_str().parse().ok());
        (participated, required)
    })
}

pub fn dossier_ref_re() -> &'static Regex {
    DOSSIER_REF.get_or_init(|| Regex::new(r"\((\d+/\d+(?:-\d+)?)\)").unwrap())
}

pub fn parse_compact_vote_number(text: &str) -> Option<String> {
    compact_vote_re()
        .captures(text)
        .map(|caps| caps[1].to_string())
}

pub fn parse_appendix_vote_number(text: &str) -> Option<String> {
    appendix_vote_re()
        .captures(text)
        .or_else(|| appendix_vote_reverse_re().captures(text))
        .map(|caps| caps[1].to_string())
}

pub fn parse_paragraph_vote_number(text: &str) -> Option<String> {
    paragraph_vote_re()
        .captures(text)
        .map(|caps| caps[1].to_string())
}

pub fn appendix_marker_for_vote(text: &str, vote_index: &str) -> bool {
    parse_appendix_vote_number(text).as_deref() == Some(vote_index)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoteBucket {
    Yes,
    No,
    Abstain,
    Total,
    Voters,
    Valid,
    BlankInvalid,
    MajorityThreshold,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoteSectionKind {
    RollCall,
    SecretBallot,
    Appendix,
    None,
}

pub fn votes_section_heading(text: &str) -> VoteSectionKind {
    let lower = text.to_lowercase();
    if lower.contains("geheime stemming") || lower.contains("scrutin secret") {
        return VoteSectionKind::SecretBallot;
    }
    if lower.contains("detail van de naamstemming") || lower.contains("détail du vote nominatif") {
        return VoteSectionKind::Appendix;
    }
    if lower.contains("naamstemming") || lower.contains("votes nominatifs") {
        return VoteSectionKind::RollCall;
    }
    VoteSectionKind::None
}

pub fn vote_bucket_label(label: &str) -> Option<VoteBucket> {
    let norm = label.trim().to_lowercase();
    match norm.as_str() {
        "ja" | "oui" | "yes" => Some(VoteBucket::Yes),
        "nee" | "non" | "no" => Some(VoteBucket::No),
        "onthoudingen" | "abstentions" | "abstention" => Some(VoteBucket::Abstain),
        "totaal" | "total" => Some(VoteBucket::Total),
        "stemmen" | "aantal stemmen" | "nombre de votants" | "votants" => Some(VoteBucket::Voters),
        "geldige stemmen" | "votes valables" => Some(VoteBucket::Valid),
        "blanco en nietige bulletins" | "bulletins blancs ou nuls" => {
            Some(VoteBucket::BlankInvalid)
        }
        "volstrekte meerderheid" | "majorité absolue" => Some(VoteBucket::MajorityThreshold),
        _ => None,
    }
}

pub fn is_language_group_header(cells: &[&str]) -> bool {
    let joined = cells
        .iter()
        .map(|c| c.trim().to_uppercase())
        .collect::<Vec<_>>();
    joined.iter().any(|c| c == "N")
        && joined.iter().any(|c| c == "TOT." || c == "TOT")
        && joined.iter().any(|c| c == "F")
}

pub fn paragraph_vote_title(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.starts_with("stemming over")
        || lower.starts_with("vote sur")
        || lower.contains("stemming over amendement")
        || lower.contains("vote sur l'amendement")
        || lower.contains("stemming over het geheel")
        || lower.contains("vote sur l'ensemble")
}

pub fn is_sitting_standing_proposal(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("bij zitten en opstaan") || lower.contains("par assis et levé")
}

pub fn formal_outcome(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    if lower.contains("aangenomen") || lower.contains("adoptée") || lower.contains("adopté") {
        Some("adopted")
    } else if lower.contains("verworpen") || lower.contains("rejetée") || lower.contains("rejeté")
    {
        Some("rejected")
    } else {
        None
    }
}

pub fn looks_like_voter_names(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() <= 3 {
        return false;
    }
    if parse_appendix_vote_number(trimmed).is_some() {
        return false;
    }
    if compact_vote_re().is_match(trimmed) || paragraph_vote_re().is_match(trimmed) {
        return false;
    }
    trimmed.chars().any(|c| c.is_alphabetic())
        && !trimmed.to_lowercase().contains("naamstemming")
        && !trimmed.to_lowercase().contains("vote nominatif")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_reuse_phrase() {
        assert!(
            reuse_result_re()
                .is_match("Mag de uitslag van de vorige stemming ook gelden voor deze stemming?")
        );
    }

    #[test]
    fn parses_participation_count() {
        let (participated, required) =
            parse_participation("65/71 leden hebben deelgenomen aan de stemming.").unwrap();
        assert_eq!(participated, 65);
        assert_eq!(required, Some(71));
    }
}
