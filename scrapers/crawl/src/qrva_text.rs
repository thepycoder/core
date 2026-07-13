use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

pub const QRVA_API_BASE: &str = "https://data.lachambre.be/v0";

static AUT_ACTR: OnceLock<Regex> = OnceLock::new();
static ORAL_REF: OnceLock<Regex> = OnceLock::new();

fn aut_actr_regex() -> &'static Regex {
    AUT_ACTR.get_or_init(|| Regex::new(r"\((\d{4,6})\)").unwrap())
}

fn oral_ref_regex() -> &'static Regex {
    ORAL_REF.get_or_init(|| {
        Regex::new(
            r"(?i)(?:\b(?:Q|MV|QO)\s*(\d{6,8}[A-Za-z])|\(Q(\d{6,8}[A-Za-z])\))",
        )
        .unwrap()
    })
}

/// Flatten QRVA API text fields (`{"br": ["…"]}` or plain string).
pub fn flatten_qrva_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.trim().to_string(),
        Value::Object(map) => {
            if let Some(Value::Array(parts)) = map.get("br") {
                return parts
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            if let Some(s) = map.get("text").and_then(|v| v.as_str()) {
                return s.trim().to_string();
            }
            String::new()
        }
        Value::Array(parts) => parts
            .iter()
            .filter_map(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Parse Chamber actor id from AUT field, e.g. `Tine Gielis, cd&v (07772)` → `7772`.
pub fn parse_aut_actr_id(aut: &str) -> Option<String> {
    aut_actr_regex()
        .captures(aut)
        .map(|c| c[1].trim_start_matches('0').to_string())
        .filter(|s| !s.is_empty())
}

/// Map QRVA actor id to cvview person_id (`O7772`).
pub fn actr_id_to_person_id(actr_id: &str) -> String {
    let digits = actr_id.trim_start_matches('0');
    format!("O{digits:0>4}")
}

pub fn written_question_id(session_id: u32, docname: &str) -> String {
    format!("{session_id}_written_{docname}")
}

pub fn route_id(session_id: u32, api_id: i64) -> String {
    format!("{session_id}_qrva_{api_id}")
}

pub fn qrva_answer_id(route: &str, slot: u8) -> String {
    format!("{route}_a{slot}")
}

pub fn inline_answer_id(question_id: &str) -> String {
    format!("{question_id}_a1")
}

pub fn department_external_id(deptnum: &str) -> String {
    format!("ext:role:dept:{deptnum}")
}

pub fn docname_internal_id(docname: &str) -> String {
    format!("DO {docname}")
}

/// Extract oral control refs from titles/text (`Q56016772C`, `MV 009725C`, …).
pub fn parse_oral_refs(text: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for cap in oral_ref_regex().captures_iter(text) {
        let id = cap
            .get(1)
            .or_else(|| cap.get(2))
            .map(|m| m.as_str().to_uppercase())
            .unwrap_or_default();
        if id.is_empty() {
            continue;
        }
        let normalized = if id.starts_with('Q') {
            id
        } else {
            format!("Q{id}")
        };
        if !refs.contains(&normalized) {
            refs.push(normalized);
        }
    }
    refs
}

pub fn qrva_detail_url(sdocname: &str) -> String {
    format!("{QRVA_API_BASE}/qrva/{sdocname}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flatten_br_array() {
        let v = json!({"br": [" line one ", "line two"]});
        assert_eq!(flatten_qrva_text(&v), "line one\nline two");
    }

    #[test]
    fn parse_actr_from_aut() {
        assert_eq!(
            parse_aut_actr_id("Tine\n      Gielis,\n      cd&v (07772)"),
            Some("7772".to_string())
        );
    }

    #[test]
    fn actr_maps_to_cvview_key() {
        assert_eq!(actr_id_to_person_id("7772"), "O7772");
        assert_eq!(actr_id_to_person_id("07772"), "O7772");
    }

    #[test]
    fn oral_refs_from_title() {
        let refs = parse_oral_refs("Federale subsidies (MV 009725C).");
        assert_eq!(refs, vec!["Q009725C".to_string()]);
    }

    #[test]
    fn written_ids_are_stable() {
        assert_eq!(written_question_id(56, "2025202606531"), "56_written_2025202606531");
        assert_eq!(route_id(56, 316583), "56_qrva_316583");
        assert_eq!(
            qrva_answer_id("56_qrva_316583", 1),
            "56_qrva_316583_a1"
        );
    }
}
