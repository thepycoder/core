use scraper::{Html, Selector};
use std::sync::LazyLock;

static SEL_P: LazyLock<Selector> = LazyLock::new(|| Selector::parse("p").unwrap());
static SEL_B: LazyLock<Selector> = LazyLock::new(|| Selector::parse("b").unwrap());
static SEL_A: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a").unwrap());

/// Canonical commission membership role from a detail-page bold label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommissionRole {
    Chair,
    Subchair,
    Permanent,
    Replacement,
}

/// Normalize the first bold role label on a commission detail page to a canonical token.
///
/// Source labels look like `Voorzitter(s):` / `Ondervoorzitters:` (see
/// `cache/commissions/details/justitie.html`). Matching must be exact after
/// normalization — substring matching would treat `Ondervoorzitters` as `Voorzitter`.
pub fn normalize_role_label(label: &str) -> Option<CommissionRole> {
    let mut s = label.to_lowercase();
    s = s.replace(['(', ')'], " ");
    s = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>();
    let tokens: Vec<&str> = s.split_whitespace().filter(|t| *t != "s").collect();
    let key = tokens.join(" ");

    match key.as_str() {
        "voorzitter" | "voorzitters" => Some(CommissionRole::Chair),
        "ondervoorzitter" | "ondervoorzitters" => Some(CommissionRole::Subchair),
        "vaste leden" => Some(CommissionRole::Permanent),
        "plaatsvervangers" => Some(CommissionRole::Replacement),
        _ => None,
    }
}

/// Extract member display names for a single role from a commission detail document.
pub fn extract_members(doc: &Html, role: CommissionRole) -> String {
    let mut names = Vec::new();

    for p in doc.select(&SEL_P) {
        let Some(first_b) = p.select(&SEL_B).next() else {
            continue;
        };
        let label = first_b.text().collect::<String>();
        if normalize_role_label(&label) != Some(role) {
            continue;
        }
        for a in p.select(&SEL_A) {
            let name = a.text().collect::<String>().trim().to_string();
            if !name.is_empty() {
                names.push(name);
            }
        }
    }

    names.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Source: cache/commissions/details/justitie.html — Voorzitter vs Ondervoorzitters.
    const JUSTITIE_ROLES_HTML: &str = r#"
        <div id="story">
            <h4>JUSTITIE </h4>
            <p><b>Voorzitter(s):</b>
                <br><b>Les Engagés:</b>
                <A HREF="cvview54.cfm?key=08161">Ismaël Nuino</A>
            </p>
            <p><b>Ondervoorzitters:</b>
                <br><b>cd&v:</b>
                <A HREF="cvview54.cfm?key=07193">Steven Matheï</A>
                <br><b>N-VA:</b>
                <A HREF="cvview54.cfm?key=06230">Kristien Van Vaerenbergh</A>
            </p>
            <p><b>Vaste Leden:</b>
                <br><b>N-VA:</b>
                <A HREF="cvview54.cfm?key=06907">Christoph D'Haese</A>
            </p>
        </div>
    "#;

    // Source: cache/commissions/details/naturalisaties.html — chair only, no subchairs.
    const NATURALISATIES_ROLES_HTML: &str = r#"
        <div id="story">
            <h4>NATURALISATIES </h4>
            <p><b>Voorzitter(s):</b>
                <br><b>PS:</b>
                <A HREF="cvview54.cfm?key=07124">Khalil Aouasti</A>
            </p>
            <p><b>Vaste Leden:</b>
                <br><b>N-VA:</b>
                <A HREF="cvview54.cfm?key=08000">Jeroen Bergers</A>
            </p>
            <p><b>Plaatsvervangers:</b>
                <br><b>VB:</b>
                <A HREF="cvview54.cfm?key=07888">Werner Somers</A>
            </p>
        </div>
    "#;

    #[test]
    fn normalize_voorzitter_and_ondervoorzitter_independently() {
        assert_eq!(
            normalize_role_label("Voorzitter(s):"),
            Some(CommissionRole::Chair)
        );
        assert_eq!(
            normalize_role_label("Ondervoorzitters:"),
            Some(CommissionRole::Subchair)
        );
        assert_eq!(
            normalize_role_label("Vaste Leden:"),
            Some(CommissionRole::Permanent)
        );
        assert_eq!(
            normalize_role_label("Plaatsvervangers:"),
            Some(CommissionRole::Replacement)
        );
        assert_eq!(normalize_role_label("Les Engagés:"), None);
    }

    #[test]
    fn justitie_separates_chair_and_subchairs() {
        let doc = Html::parse_document(JUSTITIE_ROLES_HTML);
        assert_eq!(extract_members(&doc, CommissionRole::Chair), "Ismaël Nuino");
        assert_eq!(
            extract_members(&doc, CommissionRole::Subchair),
            "Steven Matheï, Kristien Van Vaerenbergh"
        );
        assert_eq!(
            extract_members(&doc, CommissionRole::Permanent),
            "Christoph D'Haese"
        );
    }

    #[test]
    fn naturalisaties_has_chair_without_subchairs() {
        let doc = Html::parse_document(NATURALISATIES_ROLES_HTML);
        assert_eq!(
            extract_members(&doc, CommissionRole::Chair),
            "Khalil Aouasti"
        );
        assert_eq!(extract_members(&doc, CommissionRole::Subchair), "");
        assert_eq!(
            extract_members(&doc, CommissionRole::Permanent),
            "Jeroen Bergers"
        );
        assert_eq!(
            extract_members(&doc, CommissionRole::Replacement),
            "Werner Somers"
        );
    }
}
