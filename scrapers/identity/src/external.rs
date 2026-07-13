use crate::normalize::{normalize_name, reorder_name};

/// Kind of external (non-Chamber-MP) actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalKind {
    Minister,
    StateSecretary,
    Expert,
    ProceduralRole,
    Institutional,
    Other,
}

impl ExternalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minister => "minister",
            Self::StateSecretary => "state_secretary",
            Self::Expert => "expert",
            Self::ProceduralRole => "procedural_role",
            Self::Institutional => "institutional",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExternalPersonRecord {
    pub external_person_id: String,
    pub display_name: String,
    pub kind: String,
    pub source: String,
    pub first_seen_bucket: String,
    pub source_url: String,
    pub cache_path: String,
}

#[derive(Debug, Clone)]
pub struct ExternalAliasRecord {
    pub alias_norm: String,
    pub external_person_id: String,
    pub source: String,
    pub confidence: String,
}

#[derive(Debug, Clone)]
pub struct ExternalContextRecord {
    pub context_id: String,
    pub external_person_id: String,
    pub meeting_id: String,
    pub meeting_kind: String,
    pub meeting_date: String,
    pub question_id: String,
    pub question_topics_nl: String,
    pub question_topics_fr: String,
    pub utterance_excerpt: String,
    pub source_url: String,
    pub cache_path: String,
    pub raw_field: String,
}

/// Institutional author labels that are orgs, not humans.
pub fn is_institutional_label(raw: &str) -> bool {
    let lower = raw.trim().to_lowercase();
    lower.contains("(auteur)")
        || lower == "(auteur)"
        || lower.starts_with("chambre/kamer")
        || lower.starts_with("greffe/griffie")
        || lower.starts_with("commission/commissie")
        || lower.starts_with("commissions/commissies")
        || lower.starts_with("sénat/senaat")
        || lower.starts_with("senat/senaat")
        || lower.contains("medewerker van de minister")
        || lower.contains("medewerkster van de minister")
}

pub fn is_procedural_role(raw: &str) -> bool {
    matches!(
        normalize_name(raw).as_str(),
        "voorzitter" | "le president" | "la presidente"
    )
}

pub fn institutional_external_id(raw: &str) -> Option<(&'static str, &'static str)> {
    let lower = raw.trim().to_lowercase();
    if lower.contains("greffe") || lower.contains("griffie") {
        return Some(("ext:org:greffe-griffie", "Greffe/Griffie"));
    }
    if lower.contains("chambre") || lower.contains("kamer") {
        return Some(("ext:org:chambre-kamer", "Chambre/Kamer"));
    }
    if lower.contains("commissions/commissies") {
        return Some(("ext:org:commissions-commissies", "Commissions/Commissies"));
    }
    if lower.contains("commission") || lower.contains("commissie") {
        return Some(("ext:org:commission-commissie", "Commission/Commissie"));
    }
    if lower.contains("sénat") || lower.contains("senaat") || lower.contains("senat") {
        return Some(("ext:org:senat-senaat", "Sénat/Senaat"));
    }
    if lower == "(auteur)" || lower.ends_with("(auteur)") {
        return Some(("ext:org:auteur", "(AUTEUR)"));
    }
    if lower.contains("medewerker van de minister") || lower.contains("medewerkster van de minister") {
        return Some(("ext:org:minister-staff", "Minister staff"));
    }
    None
}

pub fn procedural_external_id(raw: &str) -> Option<(&'static str, &'static str)> {
    if is_procedural_role(raw) {
        Some(("ext:role:voorzitter", "Voorzitter"))
    } else {
        None
    }
}

/// Strip trailing party suffix from author strings like "Johan Deckmyn VB".
pub fn strip_party_suffix(raw: &str) -> String {
    let trimmed = raw.trim();
    let without_broken_paren = regex::Regex::new(r"\s*\([^)]*$")
        .unwrap()
        .replace(trimmed, "")
        .trim()
        .to_string();
    let parts: Vec<&str> = without_broken_paren.split_whitespace().collect();
    if parts.len() < 3 {
        return without_broken_paren;
    }
    let last = parts[parts.len() - 1].to_lowercase();
    let last_two = format!("{} {}", parts[parts.len() - 2], parts[parts.len() - 1]).to_lowercase();
    let party_tokens = [
        "vb", "n-va", "ps", "ecolo", "groen", "vooruit", "open", "vld", "cd&v", "mr", "pvda",
        "ptb", "engages", "engagés", "ecolo-groen", "les", "engagés",
    ];
    if party_tokens.contains(&last.as_str()) || last_two == "open vld" || last_two == "les engagés" {
        return parts[..parts.len() - 1].join(" ");
    }
    without_broken_paren
}

pub fn slugify_name(name: &str) -> String {
    let cleaned = strip_party_suffix(name);
    let norm = normalize_name(&cleaned);
    norm.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else if c.is_whitespace() || c == '-' {
                '-'
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn person_external_id(name: &str) -> String {
    format!("ext:person:{}", slugify_name(name))
}

pub fn department_external_id(deptnum: &str) -> String {
    format!("ext:role:dept:{deptnum}")
}

pub fn classify_named_external(name: &str, bucket: &str) -> ExternalKind {
    let lower = name.trim().to_lowercase();
    if bucket == "respondents" || bucket == "speakers" {
        if lower.contains("staatssecretaris") || lower.contains("state secretary") {
            return ExternalKind::StateSecretary;
        }
        return ExternalKind::Minister;
    }
    if bucket == "authors" {
        return ExternalKind::Expert;
    }
    if bucket == "commission_members" {
        return ExternalKind::Expert;
    }
    ExternalKind::Other
}

pub fn alias_norms_for(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    let mut norms = Vec::new();
    let primary = normalize_name(trimmed);
    if !primary.is_empty() {
        norms.push(primary);
    }
    let stripped = strip_party_suffix(trimmed);
    if stripped != trimmed {
        let s = normalize_name(&stripped);
        if !s.is_empty() && !norms.contains(&s) {
            norms.push(s);
        }
    }
    let reordered = normalize_name(&reorder_name(trimmed));
    if !reordered.is_empty() && !norms.contains(&reordered) {
        norms.push(reordered);
    }
    norms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn procedural_role_matches_accented_chair() {
        assert!(is_procedural_role("Le président"));
        assert!(is_procedural_role("La présidente"));
        assert!(is_procedural_role("Voorzitter"));
        assert!(!is_procedural_role("Jan Jambon"));
    }

    #[test]
    fn institutional_label_matches_minister_staff() {
        assert!(is_institutional_label("Medewerker van de minister"));
        assert!(is_institutional_label("Medewerkster van de minister"));
        assert!(!is_institutional_label("Jan Jambon"));
    }

    #[test]
    fn strip_party_suffix_drops_broken_paren() {
        assert_eq!(
            strip_party_suffix("Stefaan Van Hecke  (Ecolo-Groen"),
            "Stefaan Van Hecke"
        );
    }
}
