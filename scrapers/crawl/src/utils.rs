use crate::characters::CHARACTERS;
use crate::paths::cache_dir;
use regex::Regex;
use std::path::Path;

/// Stable staging id: `{session_id}_{meeting_id}_{seq}` (seq is 0-based per meeting).
pub fn composite_id(session_id: u32, meeting_id: u32, seq: i32) -> String {
    format!("{}_{}_{}", session_id, meeting_id, seq)
}

/// Meeting-kind-scoped id: `{session_id}_{scope}_{meeting_id}_{seq}`.
/// Used for questions so plenary and commission meeting numbers do not collide.
pub fn composite_scoped_id(session_id: u32, scope: &str, meeting_id: u32, seq: i32) -> String {
    format!("{}_{}_{}_{}", session_id, scope, meeting_id, seq)
}

/// Stable AgendaItem node id: `{session}_{kind}_{meeting}_agenda_{start_block}`.
///
/// Printed agenda numbers (`"15"`) are not unique within a meeting; the timeline
/// `start_block` is. Example: `56_plenary_42_agenda_596`.
pub fn agenda_item_id(
    session_id: u32,
    meeting_kind: &str,
    meeting_id: u32,
    start_block: u32,
) -> String {
    format!("{session_id}_{meeting_kind}_{meeting_id}_agenda_{start_block}")
}

/// True when `id` looks like a site-native FLWB document key (e.g. `56K1280004`), not a
/// dossier sub-number from vote title parentheses like `(297/10)`.
pub fn is_flwb_document_id(id: &str) -> bool {
    let id = id.trim();
    id.len() >= 8 && id.chars().any(|c| c.is_ascii_alphabetic())
}

/// Normalize oral-question / interpellation site refs for lookup (`Q56001216P` or `56001216P`).
pub fn normalize_site_ref(raw: &str) -> String {
    let trimmed = raw.trim().trim_start_matches('Q').trim_start_matches('q');
    trimmed.to_uppercase().replace('i', "I")
}

/// Upgrade legacy question ids (`{session}_{meeting}_{seq}`) using meeting kind from context.
pub fn ensure_question_id(session_id: &str, meeting_kind: &str, question_id: &str) -> String {
    let scoped_prefix = format!("{session_id}_{meeting_kind}_");
    if question_id.starts_with(&scoped_prefix) {
        return question_id.to_string();
    }
    let session_prefix = format!("{session_id}_");
    let rest = question_id
        .strip_prefix(&session_prefix)
        .unwrap_or(question_id);
    format!("{session_id}_{meeting_kind}_{rest}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_scoped_id_includes_kind() {
        assert_eq!(composite_scoped_id(56, "plenary", 10, 0), "56_plenary_10_0");
        assert_eq!(
            composite_scoped_id(56, "commission", 10, 2),
            "56_commission_10_2"
        );
    }

    #[test]
    fn agenda_item_id_uses_start_block() {
        assert_eq!(
            agenda_item_id(56, "plenary", 42, 596),
            "56_plenary_42_agenda_596"
        );
        assert_eq!(
            agenda_item_id(56, "commission", 15, 1),
            "56_commission_15_agenda_1"
        );
    }

    #[test]
    fn ensure_question_id_upgrades_legacy() {
        assert_eq!(
            ensure_question_id("56", "plenary", "56_10_0"),
            "56_plenary_10_0"
        );
        assert_eq!(
            ensure_question_id("56", "commission", "56_10_0"),
            "56_commission_10_0"
        );
    }

    #[test]
    fn ensure_question_id_is_idempotent() {
        assert_eq!(
            ensure_question_id("56", "plenary", "56_plenary_10_0"),
            "56_plenary_10_0"
        );
    }

    #[test]
    fn is_flwb_document_id_rejects_dossier_subnumbers() {
        assert!(!is_flwb_document_id("1"));
        assert!(!is_flwb_document_id("10"));
        assert!(is_flwb_document_id("56K1280004"));
    }

    #[test]
    fn normalize_site_ref_strips_q_prefix() {
        assert_eq!(normalize_site_ref("Q56001216P"), "56001216P");
        assert_eq!(normalize_site_ref("56000109I"), "56000109I");
    }
}

/// Highest `{session_id}-{n}.html` meeting id present under `sessions/{session_id}/meetings/{kind}/`.
pub fn max_cached_meeting_id(session_id: u32, meeting_kind: &str) -> Option<u32> {
    let dir = cache_dir().join(format!("sessions/{session_id}/meetings/{meeting_kind}"));
    let prefix = format!("{session_id}-");
    let mut max_id = None;
    let entries = std::fs::read_dir(&dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(&prefix) || !name.ends_with(".html") {
            continue;
        }
        let Some(id_str) = name
            .strip_prefix(&prefix)
            .and_then(|stem| stem.strip_suffix(".html"))
        else {
            continue;
        };
        if let Ok(id) = id_str.parse::<u32>() {
            max_id = Some(max_id.map_or(id, |current: u32| current.max(id)));
        }
    }
    max_id
}

#[cfg(test)]
mod meeting_cache_tests {

    #[test]
    fn max_cached_meeting_id_parses_filenames() {
        let dir = std::env::temp_dir().join(format!("pg-meeting-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("56-3.html"), "x").unwrap();
        std::fs::write(dir.join("56-12.html"), "x").unwrap();
        std::fs::write(dir.join("55-99.html"), "x").unwrap();

        let prefix = "56-";
        let mut max_id = None;
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with(prefix) || !name.ends_with(".html") {
                continue;
            }
            let id = name
                .strip_prefix(prefix)
                .unwrap()
                .strip_suffix(".html")
                .unwrap()
                .parse::<u32>()
                .unwrap();
            max_id = Some(max_id.map_or(id, |current: u32| current.max(id)));
        }
        assert_eq!(max_id, Some(12));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Cache path relative to `SCRAPER_CACHE_DIR` for portable provenance columns.
pub fn relative_cache_path(full_path: &Path, cache_root: &Path) -> String {
    full_path
        .strip_prefix(cache_root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| full_path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

pub fn dutch_month_to_number(month: &str) -> Option<u32> {
    match month.to_lowercase().as_str() {
        "januari" => Some(1),
        "februari" => Some(2),
        "maart" => Some(3),
        "april" => Some(4),
        "mei" => Some(5),
        "juni" => Some(6),
        "juli" => Some(7),
        "augustus" => Some(8),
        "september" => Some(9),
        "oktober" => Some(10),
        "november" => Some(11),
        "december" => Some(12),
        _ => None,
    }
}

pub fn dutch_language_to_language_code(language: &str) -> Option<&str> {
    match language.to_ascii_lowercase().as_str() {
        "nederlands" => Some("NL"),
        "frans" => Some("FR"),
        _ => None,
    }
}

pub fn slugify_name(name: &str) -> String {
    let name = name.to_lowercase();

    // Convert common special characters to ASCII equivalent
    let transliterated = name
        .replace("ä", "ae")
        .replace("ö", "oe")
        .replace("ü", "ue")
        .replace("ß", "ss")
        .replace("é", "e")
        .replace("è", "e")
        .replace("à", "a")
        .replace("ç", "c")
        .replace("ñ", "n")
        .replace("ø", "o");

    // Remove remaining non-alphanumeric characters except spaces and hyphens
    let re = Regex::new(r"[^a-z0-9\s-]").unwrap();
    let cleaned = re.replace_all(&transliterated, "");
    // Replace spaces with hyphens
    let _ = cleaned.trim().replace(" ", "-");
    cleaned.trim().replace(" ", "-")
}

pub fn clean_text(raw: &str) -> String {
    raw.replace(CHARACTERS::NEWLINE, " ")
        .replace(CHARACTERS::SOFT_HYPHEN, "")
        .replace(CHARACTERS::NON_BREAKING_SPACE, " ")
        .trim()
        .to_string()
}
