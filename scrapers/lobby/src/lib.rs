use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

pub const LOBBY_PDF_URL: &str = "https://www.dekamer.be/kvvcr/pdf_sections/lobby/lobbyregister.pdf";

static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:https?://|www\.)[\w./\-]+").unwrap());

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScrapedLobby {
    pub name: String,
    pub contacts: String,
    pub interests: String,
    pub url: String,
    pub source_url: String,
    pub cache_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnBounds {
    pub org_end: usize,
    pub interest_start: usize,
    pub url_start: usize,
}

impl Default for ColumnBounds {
    fn default() -> Self {
        Self {
            org_end: 32,
            interest_start: 56,
            url_start: 98,
        }
    }
}

pub fn extract_lobby_from_layout(
    text: &str,
    source_url: &str,
    cache_path: &str,
) -> Result<Vec<ScrapedLobby>, Box<dyn std::error::Error>> {
    let bounds = detect_column_bounds(text);
    let mut entries = Vec::new();
    let mut current: Option<ScrapedLobby> = None;

    for line in text.lines() {
        if line.trim().is_empty() || should_skip_line(line) {
            continue;
        }

        let cols = slice_columns(line, bounds);
        if !cols.iter().any(|value| !value.is_empty()) {
            continue;
        }

        if is_new_entry(line, bounds) {
            if let Some(entry) = current.take() {
                entries.push(finalize_entry(entry));
            }
            current = Some(ScrapedLobby {
                name: cols[0].clone(),
                contacts: cols[1].clone(),
                interests: cols[2].clone(),
                url: cols[3].clone(),
                source_url: source_url.to_string(),
                cache_path: cache_path.to_string(),
            });
        } else if let Some(entry) = current.as_mut() {
            merge_columns(entry, line, &cols, bounds);
        }
    }

    if let Some(entry) = current {
        entries.push(finalize_entry(entry));
    }

    entries.retain(|entry| {
        !entry.name.is_empty()
            && !Regex::new(r"^\d{2}-\d{2}-\d{2}$")
                .unwrap()
                .is_match(entry.name.trim())
    });

    Ok(entries)
}

pub fn dedupe_lobby(rows: Vec<ScrapedLobby>) -> Vec<ScrapedLobby> {
    let mut by_name: HashMap<String, ScrapedLobby> = HashMap::new();
    for row in rows {
        by_name.entry(row.name.clone()).or_insert(row);
    }
    let mut out: Vec<_> = by_name.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Detect fixed-width column boundaries from URL token positions in the layout text.
pub fn detect_column_bounds(text: &str) -> ColumnBounds {
    let mut bounds = ColumnBounds::default();

    let mut url_starts = Vec::new();
    for line in text.lines() {
        if should_skip_line(line) || line.trim().is_empty() {
            continue;
        }
        for mat in URL_RE.find_iter(line) {
            url_starts.push(mat.start());
        }
    }
    if let Some(url_start) = percentile_low(&url_starts, 0.1) {
        bounds.url_start = url_start.min(bounds.url_start);
    }

    bounds
}

fn percentile_low(values: &[usize], fraction: f64) -> Option<usize> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64) * fraction).floor() as usize;
    Some(sorted[idx.min(sorted.len() - 1)])
}

pub fn should_skip_line(line: &str) -> bool {
    if line.contains("LOBBYREGISTER") {
        return true;
    }
    if line.contains("laatst bijgewerkt") || line.contains("dernière mise") {
        return true;
    }

    let trimmed = line.trim_start();
    if trimmed.starts_with("organisme") && line.contains("contactpersonen") {
        return true;
    }
    if trimmed.starts_with("organisation") && line.contains("personnes de contact") {
        return true;
    }
    if trimmed.starts_with("behartigt belangen") || trimmed.starts_with("gère des intérêts") {
        return true;
    }

    Regex::new(r"^\d{2}-\d{2}-\d{2}$")
        .map(|re| re.is_match(line.trim()))
        .unwrap_or(false)
}

fn is_new_entry(line: &str, bounds: ColumnBounds) -> bool {
    first_non_space_byte_index(line) < bounds.org_end
}

fn first_non_space_byte_index(line: &str) -> usize {
    line.bytes()
        .position(|byte| byte != b' ')
        .unwrap_or(usize::MAX)
}

fn slice_columns(line: &str, bounds: ColumnBounds) -> [String; 4] {
    [
        slice_byte_range(line, 0, Some(bounds.org_end)),
        slice_byte_range(line, bounds.org_end, Some(bounds.interest_start)),
        slice_byte_range(line, bounds.interest_start, Some(bounds.url_start)),
        slice_byte_range(line, bounds.url_start, None),
    ]
}

fn slice_byte_range(line: &str, start: usize, end: Option<usize>) -> String {
    let start = byte_index_at_or_before(line, start);
    let end = end
        .map(|value| byte_index_at_or_before(line, value))
        .unwrap_or(line.len());
    if start >= end || start >= line.len() {
        return String::new();
    }
    line.get(start..end).unwrap_or("").trim().to_string()
}

fn byte_index_at_or_before(text: &str, byte_index: usize) -> usize {
    if byte_index >= text.len() {
        return text.len();
    }

    let mut index = byte_index;
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn merge_columns(entry: &mut ScrapedLobby, line: &str, cols: &[String; 4], bounds: ColumnBounds) {
    let pos = first_non_space_byte_index(line);
    if pos < bounds.interest_start {
        merge_field(&mut entry.contacts, &cols[1]);
    }
    if pos < bounds.url_start {
        merge_field(&mut entry.interests, &cols[2]);
    }
    merge_field(&mut entry.url, &cols[3]);
}

fn merge_field(target: &mut String, value: &str) {
    if value.is_empty() {
        return;
    }

    if target.is_empty() {
        *target = value.to_string();
    } else {
        target.push(' ');
        target.push_str(value);
    }
}

fn finalize_entry(mut entry: ScrapedLobby) -> ScrapedLobby {
    entry.url = normalize_url_field(&entry.url);
    strip_url_bleed(&mut entry.contacts, &entry.url);
    strip_url_bleed(&mut entry.interests, &entry.url);
    entry
}

fn normalize_url_field(raw: &str) -> String {
    URL_RE
        .find_iter(raw)
        .map(|mat| mat.as_str().trim_end_matches('.').to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_url_bleed(field: &mut String, canonical_url: &str) {
    if field.is_empty() || canonical_url.is_empty() {
        return;
    }

    let canonical = canonical_url.to_lowercase();
    let mut cleaned = field.clone();
    for mat in URL_RE.find_iter(field) {
        let token = mat.as_str();
        let lower = token.to_lowercase();
        if canonical.contains(&lower) || lower.len() >= 4 && canonical.contains(&lower[..4]) {
            cleaned = cleaned.replace(token, " ");
        }
    }
    *field = normalize_whitespace(&cleaned);
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Source: cache/lobby/lobbyregister.pdf (pdftotext -layout), Agoria row.
    const FIXTURE_AGORIA: &str = "\
Agoria                          Bart Steukers            lidbedrijven van Agoria (Belgische         www.agoria.be/www.wsc/rep/prg
                                Beatrice Vanden Abeele   bedrijven die technologisch gedreven       www.agoria.be
                                                         activiteiten verrichten of diensten        www.agoria.be/nl/Agoria-
                                                         verlenen).                                 Vlaanderen";

    // Source: cache/lobby/lobbyregister.pdf, Air Cargo Belgium row.
    const FIXTURE_AIR_CARGO: &str = "\
Air Cargo Belgium               Freek de Witte           bevordering van de samenwerking          www.aircargobelgium.be
                                Nathan Goethals          binnen de Belgische
                                Andres Jimenez           luchtvrachtindustrie, operationele
                                                         uitmuntendheid, duurzaamheid en
                                                         digitale connectiviteit, zodat de
                                                         gemeenschap transparante, veilige en
                                                         klantgerichte toeleveringsketens kan
                                                         organiseren.";

    // Source: cache/lobby/lobbyregister.pdf, A&T Efficiency row (accented contact).
    const FIXTURE_AT_EFFICIENCY: &str = "\
A&T Efficiency                  Thomas Charlier         bureau d’études facilitant la                www.atefficiency.be
                                André Roelandt          collaboration entre les entreprises
                                                        privées et le secteur public aux
                                                        bénéfices des deux.";

    // Source: cache/lobby/lobbyregister.pdf, Aidants Proches row (accented Céline Feuillat).
    const FIXTURE_AIDANTS: &str = "\
Aidants Proches                 Céline Feuillat          centre de compétences pour les aidants     www.aidants.be
                                                         proches.";

    fn parse_fixture(body: &str) -> Vec<ScrapedLobby> {
        extract_lobby_from_layout(body, LOBBY_PDF_URL, "lobby/lobbyregister.pdf").unwrap()
    }

    #[test]
    fn agoria_columns_match_pdf_layout() {
        let entry = parse_fixture(FIXTURE_AGORIA)
            .into_iter()
            .find(|row| row.name == "Agoria")
            .expect("Agoria row");
        assert!(entry.contacts.contains("Bart Steukers"));
        assert!(entry.contacts.contains("Beatrice Vanden Abeele"));
        assert!(entry.interests.contains("lidbedrijven van Agoria"));
        assert!(!entry.interests.contains("www."));
        assert!(entry.url.contains("www.agoria.be"));
    }

    #[test]
    fn air_cargo_belgium_columns_match_pdf_layout() {
        let entry = parse_fixture(FIXTURE_AIR_CARGO)
            .into_iter()
            .find(|row| row.name == "Air Cargo Belgium")
            .expect("Air Cargo Belgium row");
        assert!(entry.contacts.contains("Freek de Witte"));
        assert!(entry.contacts.contains("Andres Jimenez"));
        assert!(entry.interests.contains("luchtvrachtindustrie"));
        assert!(!entry.interests.contains("www."));
        assert_eq!(entry.url, "www.aircargobelgium.be");
    }

    #[test]
    fn at_efficiency_preserves_accented_contacts() {
        let entry = parse_fixture(FIXTURE_AT_EFFICIENCY)
            .into_iter()
            .find(|row| row.name == "A&T Efficiency")
            .expect("A&T Efficiency row");
        assert!(entry.contacts.contains("Thomas Charlier"));
        assert!(entry.contacts.contains("André Roelandt"));
        assert!(entry.interests.contains("bureau d’études facilitant la"));
        assert!(!entry.interests.contains("www."));
        assert_eq!(entry.url, "www.atefficiency.be");
    }

    #[test]
    fn aidants_proches_preserves_accented_contact() {
        let entry = parse_fixture(FIXTURE_AIDANTS)
            .into_iter()
            .find(|row| row.name == "Aidants Proches")
            .expect("Aidants Proches row");
        assert_eq!(entry.contacts, "Céline Feuillat");
        assert!(entry.interests.contains("centre de compétences"));
        assert!(!entry.interests.contains("www."));
        assert_eq!(entry.url, "www.aidants.be");
    }

    #[test]
    fn skip_patterns_do_not_drop_acodev_rows() {
        let sample = "\
ABR - BVI                       Alain Dewit              association professionnelle des sociétés   www.abrbvi.be
                                                         de recouvrement de créances.
ACODEV                          Raphaël Maldague         fédération des organisations de la         www.acodev.be
                                Bruno Nicostrate         société civile francophones et";
        let entries = parse_fixture(sample);
        let abr = entries.iter().find(|row| row.name == "ABR - BVI").unwrap();
        let acodev = entries.iter().find(|row| row.name == "ACODEV").unwrap();
        assert_eq!(abr.contacts, "Alain Dewit");
        assert!(acodev.contacts.contains("Bruno Nicostrate"));
    }

    #[test]
    fn legacy_sample_still_parses() {
        let sample = "\
       organisme                   contactpersonen                behartigt belangen voor                           WEB
11.11.11                        Naima Charkaoui          koepel van de Vlaamse Noord-               www.11.be
                                                         Zuidbeweging.
AB InBev                        Aron Wils                brouwen, verkoop en marketing van          www.ab-inbev.be
                                                         bieren.";

        let entries = parse_fixture(sample);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "11.11.11");
        assert_eq!(entries[0].url, "www.11.be");
        assert_eq!(entries[1].name, "AB InBev");
        assert_eq!(entries[1].url, "www.ab-inbev.be");
    }

    #[test]
    fn cached_pdf_fixture_organisations_match_layout() {
        let pdf = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/lobby/lobbyregister.pdf");
        if !pdf.exists() {
            return;
        }

        let output = std::process::Command::new("pdftotext")
            .args(["-layout", pdf.to_str().unwrap(), "-"])
            .output()
            .expect("pdftotext");
        let text = String::from_utf8(output.stdout).expect("utf8 layout text");
        let entries = extract_lobby_from_layout(&text, LOBBY_PDF_URL, "lobby/lobbyregister.pdf")
            .expect("parse lobby pdf");

        let agoria = entries
            .iter()
            .find(|row| row.name == "Agoria")
            .expect("Agoria");
        assert!(!agoria.interests.contains("www."));
        assert!(agoria.contacts.contains("Beatrice Vanden Abeele"));

        let at = entries
            .iter()
            .find(|row| row.name == "A&T Efficiency")
            .expect("A&T Efficiency");
        assert!(at.contacts.contains("André Roelandt"));
        assert!(at.interests.contains("bureau d’études"));

        let bleed = entries
            .iter()
            .filter(|row| row.interests.contains("www."))
            .count();
        assert_eq!(bleed, 0, "expected zero interests column URL bleed");
    }
}
