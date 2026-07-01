use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

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

/// List page stores names as "Last First"; reorder to "First Last".
pub fn reorder_name(raw: &str) -> String {
    let raw = raw.trim();
    let mut parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.len() > 1 {
        let first = parts.pop().unwrap();
        format!("{} {}", first, parts.join(" "))
    } else {
        raw.to_string()
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
    fn typo_fix_applies_known_map() {
        let map = typo_corrections();
        assert_eq!(
            apply_typo_fix("Steven Coengrachts", &map),
            "Steven Coenegrachts"
        );
    }
}
