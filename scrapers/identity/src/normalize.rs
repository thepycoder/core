use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

static LEADING_MARKERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[\d•·\u2022]+\s*|[\-–\u2013]\s*)").unwrap()
});

static LEADING_TURN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d{2}\.\d{2}\d?\s+").unwrap()
});

static COMMA_ROLE_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i),\s*(?:premier\s+ministre|prime\s+minister|ministre(?:\s+président)?|minister|staatssecretaris|secrétaire(?:\s+d[''']?[eé]tat)?|state\s+secretary).*$",
    )
    .unwrap()
});

static BROKEN_PARTY_PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\([^)]*$").unwrap());

static TITLE_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:e\s+)?(?:e\s*)?erste\s+minister|(?:eerste\s+minister|ministre(?:\s+président)?|minister|staatssecretaris|de\s+heer|mevrouw|le\s+ministre|la\s+ministre|monsieur|madame)\s+",
    )
    .unwrap()
});

fn strip_leading_markers(raw: &str) -> String {
    let mut out = raw.to_string();
    loop {
        let next = LEADING_MARKERS.replace(&out, "");
        if next.len() == out.len() {
            break;
        }
        out = next.into_owned();
    }
    out.trim().to_string()
}

fn normalize_apostrophes(raw: &str) -> String {
    raw.replace('\u{2019}', "'").replace('\u{2018}', "'")
}

fn strip_leading_turn(raw: &str) -> String {
    let mut out = raw.to_string();
    loop {
        let next = LEADING_TURN.replace(&out, "");
        if next.len() == out.len() {
            break;
        }
        out = next.into_owned();
    }
    out.trim().to_string()
}

/// Known misspellings scraped from plenary question headers (wrong -> correct).
pub fn typo_corrections() -> HashMap<String, String> {
    [
        ("Steven Coengrachts", "Steven Coenegrachts"),
        ("Ridouhane Chahid", "Ridouane Chahid"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

pub fn apply_typo_fix(raw: &str, typo_map: &HashMap<String, String>) -> String {
    typo_map
        .get(raw.trim())
        .cloned()
        .unwrap_or_else(|| raw.trim().to_string())
}

/// Strip honorifics and fix common scrape artefacts before name matching.
pub fn clean_raw_name(raw: &str) -> String {
    let collapsed = WHITESPACE
        .replace_all(raw.trim(), " ")
        .into_owned();
    let apostrophe = normalize_apostrophes(&collapsed);
    let no_turn = strip_leading_turn(&apostrophe);
    let stripped = strip_leading_markers(&no_turn);
    let no_comma_role = COMMA_ROLE_SUFFIX
        .replace(&stripped, "")
        .trim()
        .to_string();
    let no_broken_paren = BROKEN_PARTY_PAREN
        .replace(&no_comma_role, "")
        .trim()
        .to_string();
    TITLE_PREFIX
        .replace(&no_broken_paren, "")
        .trim()
        .to_string()
}

/// Lowercase, transliterate accents, collapse whitespace.
pub fn normalize_name(raw: &str) -> String {
    let lowered = raw.trim().to_lowercase();
    let transliterated = lowered
        .replace('ä', "ae")
        .replace('ö', "oe")
        .replace('ü', "ue")
        .replace('ß', "ss")
        .replace('é', "e")
        .replace('è', "e")
        .replace('ê', "e")
        .replace('ë', "e")
        .replace('à', "a")
        .replace('â', "a")
        .replace('ç', "c")
        .replace('ñ', "n")
        .replace('ø', "o")
        .replace('î', "i")
        .replace('ï', "i")
        .replace('ô', "o")
        .replace('ù', "u")
        .replace('û', "u");
    WHITESPACE
        .replace_all(transliterated.trim(), " ")
        .into_owned()
}

/// List/vote appendix stores names as "Last First"; reorder to "First Last".
///
/// Two tokens swap simply (`Jambon Jan` → `Jan Jambon`). Longer vote appendix
/// names keep surname particles with the trailing surname word
/// (`Donckt Wim Van der` → `Wim Van der Donckt`).
pub fn reorder_name(raw: &str) -> String {
    let raw = raw.trim();
    let parts: Vec<&str> = raw.split_whitespace().collect();
    match parts.as_slice() {
        [] => String::new(),
        [only] => (*only).to_string(),
        [a, b] => format!("{b} {a}"),
        [head, middle, tail @ ..] => format!("{middle} {} {head}", tail.join(" ")),
    }
}

/// Vote appendix stores names as "Last First"; reorder to "First Last".
pub fn convert_name(name: &str) -> String {
    reorder_name(name)
}

#[derive(Debug, Clone)]
pub struct PersonName {
    pub first_name: String,
    pub last_name: String,
}

impl PersonName {
    pub fn full(&self) -> String {
        format!("{} {}", self.first_name.trim(), self.last_name.trim()).trim().to_string()
    }

    pub fn reversed(&self) -> String {
        format!("{} {}", self.last_name.trim(), self.first_name.trim()).trim().to_string()
    }

    pub fn normalized_full(&self) -> String {
        normalize_name(&self.full())
    }

    pub fn normalized_reversed(&self) -> String {
        normalize_name(&self.reversed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(normalize_name("  Jan   Jambon  "), "jan jambon");
    }

    #[test]
    fn reorder_name_swaps_last_first() {
        assert_eq!(reorder_name("Jambon Jan"), "Jan Jambon");
    }

    #[test]
    fn reorder_name_handles_surname_particles() {
        assert_eq!(reorder_name("Donckt Wim Van der"), "Wim Van der Donckt");
        assert_eq!(reorder_name("Bosch Annik Van den"), "Annik Van den Bosch");
        assert_eq!(reorder_name("Roover Peter De"), "Peter De Roover");
        assert_eq!(reorder_name("Riet Katrijn van"), "Katrijn van Riet");
        assert_eq!(reorder_name("Ngoi Mutyebele"), "Mutyebele Ngoi");
    }

    #[test]
    fn clean_raw_name_strips_titles() {
        assert_eq!(
            clean_raw_name("E eerste minister  Alexander De Croo"),
            "Alexander De Croo"
        );
        assert_eq!(
            clean_raw_name("E erste minister  Alexander De Croo"),
            "Alexander De Croo"
        );
        assert_eq!(clean_raw_name("De heer Jan Jambon"), "Jan Jambon");
        assert_eq!(clean_raw_name("Minister Georges Gilkinet"), "Georges Gilkinet");
    }

    #[test]
    fn clean_raw_name_strips_leading_markers() {
        assert_eq!(clean_raw_name("0     Steven Vandeput"), "Steven Vandeput");
        assert_eq!(clean_raw_name("4  Minister  Jan Jambon"), "Jan Jambon");
        assert_eq!(clean_raw_name("-Bert Wollants"), "Bert Wollants");
        assert_eq!(clean_raw_name("- Bert Wollants"), "Bert Wollants");
        assert_eq!(clean_raw_name("'t Hooft"), "'t Hooft");
    }

    #[test]
    fn clean_raw_name_strips_comma_role_suffixes() {
        assert_eq!(
            clean_raw_name("Bart De Wever , premier ministre"),
            "Bart De Wever"
        );
        assert_eq!(
            clean_raw_name("Alexia Bertrand , secrétaire d'État"),
            "Alexia Bertrand"
        );
        assert_eq!(
            clean_raw_name("Nicole de Moor , secrétaire d'État"),
            "Nicole de Moor"
        );
    }

    #[test]
    fn clean_raw_name_strips_turn_and_minister_prefix() {
        assert_eq!(
            clean_raw_name("01.03     Minister  Vanessa Matz"),
            "Vanessa Matz"
        );
        assert_eq!(
            clean_raw_name("07.05     Minister  Bernard Quintin"),
            "Bernard Quintin"
        );
    }

    #[test]
    fn clean_raw_name_fixes_broken_party_and_apostrophe() {
        assert_eq!(
            clean_raw_name("Stefaan Van Hecke  (Ecolo-Groen"),
            "Stefaan Van Hecke"
        );
        assert_eq!(clean_raw_name("Jan  Jambon"), "Jan Jambon");
        assert_eq!(
            clean_raw_name("Roberto D\u{2019}Amico"),
            "Roberto D'Amico"
        );
    }

    #[test]
    fn typo_fix_applies_known_map() {
        let map = typo_corrections();
        assert_eq!(
            apply_typo_fix("Steven Coengrachts", &map),
            "Steven Coenegrachts"
        );
    }
}
