//! Central catalog for plenary meeting-report corpus policy.
//!
//! See `docs/meeting-report-corpus-policy.md` for rationale. Raw HTML and
//! `report_blocks.parquet` remain canonical evidence even when text is not
//! promoted to `Utterance` rows.

use crate::agenda_timeline::MeetingKind;
use crate::report_blocks::{BlockTag, ReportBlock};

/// Human-readable policy document path (repo-relative).
pub const POLICY_DOC: &str = "docs/meeting-report-corpus-policy.md";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusClass {
    /// Opening session, credentials verification, agenda adoption.
    Constitutive,
    /// Constitutive plus funeral tributes, appointments, admin comms.
    ConstitutiveAdministrative,
    /// Contains procedural oath/credentials items but substantial political content.
    Mixed,
    /// Ordinary session opening with bureau/commission appointments.
    SessionOpening,
    /// Vote-dominated sitting; vote QA is the primary completeness metric.
    VoteDominated,
}

impl CorpusClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Constitutive => "constitutive",
            Self::ConstitutiveAdministrative => "constitutive_administrative",
            Self::Mixed => "mixed",
            Self::SessionOpening => "session_opening",
            Self::VoteDominated => "vote_dominated",
        }
    }

    /// Whole-report classes where low speech-coverage is expected by policy.
    pub fn suppresses_coverage_warning(self) -> bool {
        matches!(
            self,
            Self::Constitutive | Self::ConstitutiveAdministrative
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorpusClassification {
    pub class: CorpusClass,
    /// Short note surfaced in QA detail rows.
    pub note: &'static str,
}

struct CatalogEntry {
    session_id: u32,
    meeting_kind: MeetingKind,
    meeting_id: u32,
    class: CorpusClass,
    note: &'static str,
}

/// Explicit session catalog. Prefer this over heuristics for known meetings.
const CATALOG: &[CatalogEntry] = &[
    // Fully or primarily constitutive (session 56 plenary).
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 1,
        class: CorpusClass::Constitutive,
        note: "opening extraordinary session; credentials verification; agenda adoption",
    },
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 2,
        class: CorpusClass::Constitutive,
        note: "credentials reports, admission votes, constitutional oaths (56-2.html)",
    },
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 3,
        class: CorpusClass::Constitutive,
        note: "admission, credentials verification, oath report (56-3.html)",
    },
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 4,
        class: CorpusClass::ConstitutiveAdministrative,
        note: "oaths, tributes, appointments, administrative communications (56-4.html)",
    },
    // Mixed: procedural oath/credentials agenda items but not whole-report exempt.
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 7,
        class: CorpusClass::Mixed,
        note: "contains oath/credentials item plus substantive debate",
    },
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 15,
        class: CorpusClass::Mixed,
        note: "contains oath/credentials item plus substantive debate",
    },
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 24,
        class: CorpusClass::Mixed,
        note: "successor oaths plus government declaration and confidence motion",
    },
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 35,
        class: CorpusClass::Mixed,
        note: "contains oath/credentials item plus substantive debate",
    },
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 63,
        class: CorpusClass::Mixed,
        note: "contains oath/credentials item plus substantive debate",
    },
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 69,
        class: CorpusClass::Mixed,
        note: "contains oath/credentials item plus substantive debate",
    },
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 93,
        class: CorpusClass::Mixed,
        note: "contains oath/credentials item plus substantive debate",
    },
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 119,
        class: CorpusClass::Mixed,
        note: "contains oath/credentials item plus substantive debate",
    },
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 67,
        class: CorpusClass::SessionOpening,
        note: "ordinary session opening, bureau/commission appointments, chair address",
    },
    CatalogEntry {
        session_id: 56,
        meeting_kind: MeetingKind::Plenary,
        meeting_id: 79,
        class: CorpusClass::VoteDominated,
        note: "confidence and motion votes dominate; speech coverage is secondary",
    },
];

pub fn classify_meeting(
    session_id: u32,
    meeting_kind: MeetingKind,
    meeting_id: u32,
) -> Option<CorpusClassification> {
    CATALOG
        .iter()
        .find(|e| {
            e.session_id == session_id && e.meeting_kind == meeting_kind && e.meeting_id == meeting_id
        })
        .map(|e| CorpusClassification {
            class: e.class,
            note: e.note,
        })
}

/// Procedural credentials/oath **agenda headings** (h1/h2), not debate prose.
///
/// Example source: `cache/sessions/56/meetings/plenary/56-2.html` — meeting 2 lacks
/// normal h2 structure; the credentials section appears in report blocks as
/// `Onderzoek van de geloofsbrieven en eedafleggingen`.
pub fn is_procedural_credentials_heading(text: &str) -> bool {
    let folded = text.to_lowercase();
    folded.contains("onderzoek van de geloofsbrieven")
        || folded.contains("vérification des pouvoirs et prestations de serment")
        || folded.contains("verification des pouvoirs et prestations de serment")
}

/// True when report blocks indicate a whole-report constitutive session (not mixed).
///
/// Requires a procedural credentials heading in h1/h2 **and** no political agenda
/// headings that would indicate substantive debate content.
pub fn whole_report_constitutive_from_blocks(blocks: &[ReportBlock]) -> bool {
    let mut has_credentials_heading = false;
    let mut has_political_agenda = false;

    for block in blocks {
        if !matches!(block.tag, BlockTag::H1 | BlockTag::H2) {
            continue;
        }
        if is_procedural_credentials_heading(&block.text) {
            has_credentials_heading = true;
            continue;
        }
        if is_political_agenda_heading(&block.text) {
            has_political_agenda = true;
        }
    }

    has_credentials_heading && !has_political_agenda
}

fn is_political_agenda_heading(text: &str) -> bool {
    let folded = text.to_lowercase();
    folded.contains("interpellatie")
        || folded.contains("interpellation")
        || folded.contains(" mondelinge vragen")
        || folded.contains(" questions orales")
        || folded.contains("regeringsverklaring")
        || folded.contains("déclaration gouvernementale")
        || folded.contains("motie van vertrouwen")
        || folded.contains("motion de confiance")
        || folded.contains("algemeen debat")
        || folded.contains("débat d'actualité")
        || folded.contains(" vraag van ")
        || folded.contains(" question de ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report_blocks::BlockTag;

    fn heading(tag: BlockTag, text: &str) -> ReportBlock {
        ReportBlock {
            index: 0,
            tag,
            text: text.to_string(),
            inlines: Vec::new(),
            table_rows: None,
            lang: None,
            class: None,
            has_oraspr: false,
            content_hash: String::new(),
            word_count: 0,
        }
    }

    #[test]
    fn catalog_constitutive_meetings_suppress_coverage() {
        for id in [1, 2, 3] {
            let c = classify_meeting(56, MeetingKind::Plenary, id).unwrap();
            assert_eq!(c.class, CorpusClass::Constitutive);
            assert!(c.class.suppresses_coverage_warning());
        }
        let four = classify_meeting(56, MeetingKind::Plenary, 4).unwrap();
        assert_eq!(four.class, CorpusClass::ConstitutiveAdministrative);
        assert!(four.class.suppresses_coverage_warning());
    }

    #[test]
    fn mixed_meeting_24_still_evaluated_for_coverage() {
        let c = classify_meeting(56, MeetingKind::Plenary, 24).unwrap();
        assert_eq!(c.class, CorpusClass::Mixed);
        assert!(!c.class.suppresses_coverage_warning());
    }

    #[test]
    fn meeting_2_credentials_heading_in_blocks() {
        // Originating document: cache/sessions/56/meetings/plenary/56-2.html
        assert!(is_procedural_credentials_heading(
            "Onderzoek van de geloofsbrieven en eedafleggingen"
        ));
        assert!(is_procedural_credentials_heading(
            "01 Vérification des pouvoirs et prestations de serment"
        ));
    }

    #[test]
    fn meeting_22_political_serment_not_credentials_heading() {
        // Originating document: cache/sessions/56/meetings/plenary/56-22.html
        // Political question about President Trump's oath — not a credentials agenda item.
        assert!(!is_procedural_credentials_heading(
            "l'idée de gâcher ce moment important de prestation de serment pour nos futurs collègues"
        ));
    }

    #[test]
    fn mixed_fixture_with_oath_heading_still_has_political_agenda() {
        let blocks = vec![
            heading(
                BlockTag::H2,
                "Onderzoek van de geloofsbrieven en eedafleggingen",
            ),
            heading(
                BlockTag::H2,
                "03 Vraag van Marieke O'Hara (cd&v) over migratie",
            ),
        ];
        assert!(!whole_report_constitutive_from_blocks(&blocks));
    }

    #[test]
    fn whole_report_constitutive_fixture_without_political_headings() {
        let blocks = vec![
            heading(
                BlockTag::H2,
                "Onderzoek van de geloofsbrieven en eedafleggingen",
            ),
            heading(BlockTag::H2, "Goedkeuring van de agenda"),
        ];
        assert!(whole_report_constitutive_from_blocks(&blocks));
    }

    #[test]
    fn vote_dominated_meeting_79_cataloged() {
        let c = classify_meeting(56, MeetingKind::Plenary, 79).unwrap();
        assert_eq!(c.class, CorpusClass::VoteDominated);
        assert!(!c.class.suppresses_coverage_warning());
    }
}
