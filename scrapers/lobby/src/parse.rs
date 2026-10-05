use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:https?://|www\.)[\w./\-]+").unwrap());
static DATE_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{2}-\d{2}-\d{2}$").unwrap());

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScrapedLobby {
    pub name: String,
    pub contacts: String,
    pub interests: String,
    pub url: String,
}

/// Column start positions (in characters) in `pdftotext -layout` output.
/// The register is printed from a fixed template, but columns drift by a
/// character or two between pages, so segments are assigned with some slack.
const CONTACTS_MIN_COL: usize = 28;
const INTERESTS_MIN_COL: usize = 50;
const DEFAULT_URL_MIN_COL: usize = 95;

/// Parses the lobby register from `pdftotext -layout` text.
///
/// Organisation names wrap over several lines, and those lines can carry the
/// next contact person, so a name on its own does not start an organisation.
/// A name line starts one when it also has a contact person and a URL
/// (wrapped name lines can carry an organisation's second URL, but then lack
/// a contact), or interests text after the previous organisation's
/// description ended with a full stop. Other lines continue the current
/// organisation, with one contact person per line.
pub fn parse_lobby_layout(text: &str) -> Vec<ScrapedLobby> {
    let url_min_col = detect_url_column(text);
    let mut entries = Vec::new();
    let mut current: Option<ScrapedLobby> = None;

    for line in text.lines() {
        if line.trim().is_empty() || should_skip_line(line) {
            continue;
        }

        let [name, contacts, interests, url] = split_columns(line, url_min_col);
        let starts_entry = !name.is_empty()
            && match &current {
                None => true,
                Some(entry) => {
                    (!contacts.is_empty() && URL_RE.is_match(&url))
                        || (!interests.is_empty() && entry.interests.ends_with('.'))
                }
            };

        if starts_entry {
            if let Some(entry) = current.take() {
                entries.push(finalize_entry(entry));
            }
            current = Some(ScrapedLobby {
                name,
                contacts,
                interests,
                url,
            });
        } else if let Some(entry) = current.as_mut() {
            append(&mut entry.name, &name, " ");
            // A parenthesised email belongs to the contact on the line above.
            let separator = if contacts.starts_with('(') { " " } else { ", " };
            append(&mut entry.contacts, &contacts, separator);
            append(&mut entry.interests, &interests, " ");
            append(&mut entry.url, &url, " ");
        }
    }

    if let Some(entry) = current {
        entries.push(finalize_entry(entry));
    }

    entries.retain(|entry| !entry.name.is_empty());
    entries
}

/// Keeps the first entry per organisation name, sorted by name.
pub fn dedupe_lobby(rows: Vec<ScrapedLobby>) -> Vec<ScrapedLobby> {
    let mut by_name = BTreeMap::new();
    for row in rows {
        by_name.entry(row.name.clone()).or_insert(row);
    }
    by_name.into_values().collect()
}

/// The URL column moves the most between pages, so put its start at the
/// leftmost URLs (10th percentile) when that is left of the default.
fn detect_url_column(text: &str) -> usize {
    let mut url_starts: Vec<usize> = text
        .lines()
        .filter(|line| !line.trim().is_empty() && !should_skip_line(line))
        .flat_map(segments)
        .filter(|(col, segment)| *col >= INTERESTS_MIN_COL && URL_RE.is_match(segment))
        .map(|(col, _)| col)
        .collect();
    if url_starts.is_empty() {
        return DEFAULT_URL_MIN_COL;
    }
    url_starts.sort_unstable();
    url_starts[url_starts.len() / 10].min(DEFAULT_URL_MIN_COL)
}

/// Splits a line into text segments separated by two or more spaces, with
/// the character column each segment starts at.
fn segments(line: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut start = 0;
    let mut spaces = 0;

    for (col, c) in line.chars().enumerate() {
        if c == ' ' {
            spaces += 1;
            continue;
        }
        if spaces >= 2 && !current.is_empty() {
            out.push((start, std::mem::take(&mut current)));
        }
        if current.is_empty() {
            start = col;
        } else if spaces == 1 {
            current.push(' ');
        }
        current.push(c);
        spaces = 0;
    }
    if !current.is_empty() {
        out.push((start, current));
    }
    out
}

fn split_columns(line: &str, url_min_col: usize) -> [String; 4] {
    let mut cols: [String; 4] = Default::default();
    for (col, segment) in segments(line) {
        let index = if col >= url_min_col {
            3
        } else if col >= INTERESTS_MIN_COL {
            2
        } else if col >= CONTACTS_MIN_COL {
            1
        } else {
            0
        };
        append(&mut cols[index], &segment, " ");
    }
    cols
}

fn should_skip_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    line.contains("LOBBYREGISTER")
        || line.contains("laatst bijgewerkt")
        || line.contains("dernière mise")
        || (trimmed.starts_with("organisme") && line.contains("contactpersonen"))
        || (trimmed.starts_with("organisation") && line.contains("personnes de contact"))
        || trimmed.starts_with("behartigt belangen")
        || trimmed.starts_with("gère des intérêts")
        || DATE_LINE_RE.is_match(line.trim())
}

fn append(target: &mut String, value: &str, separator: &str) {
    if value.is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push_str(separator);
    }
    target.push_str(value);
}

fn finalize_entry(mut entry: ScrapedLobby) -> ScrapedLobby {
    entry.url = URL_RE
        .find_iter(&entry.url)
        .map(|m| m.as_str().trim_end_matches('.'))
        .collect::<Vec<_>>()
        .join(" ");
    entry
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(text: &str, name: &str) -> ScrapedLobby {
        parse_lobby_layout(text)
            .into_iter()
            .find(|row| row.name == name)
            .unwrap_or_else(|| panic!("{name} row"))
    }

    #[test]
    fn continuation_lines_extend_each_column() {
        let text = "\
Agoria                          Bart Steukers            lidbedrijven van Agoria (Belgische         www.agoria.be/www.wsc/rep/prg
                                Beatrice Vanden Abeele   bedrijven die technologisch gedreven       www.agoria.be
                                                         activiteiten verrichten of diensten        www.agoria.be/nl/Agoria-
                                                         verlenen).                                 Vlaanderen";
        let entry = parse_one(text, "Agoria");
        assert_eq!(entry.contacts, "Bart Steukers, Beatrice Vanden Abeele");
        assert_eq!(
            entry.interests,
            "lidbedrijven van Agoria (Belgische bedrijven die technologisch gedreven \
             activiteiten verrichten of diensten verlenen)."
        );
        assert!(entry.url.starts_with("www.agoria.be/www.wsc/rep/prg"));
    }

    #[test]
    fn keeps_accented_names_and_narrow_url_column() {
        let text = "\
A&T Efficiency                  Thomas Charlier         bureau d’études facilitant la                www.atefficiency.be
                                André Roelandt          collaboration entre les entreprises
                                                        privées et le secteur public aux
                                                        bénéfices des deux.";
        let entry = parse_one(text, "A&T Efficiency");
        assert_eq!(entry.contacts, "Thomas Charlier, André Roelandt");
        assert!(entry.interests.starts_with("bureau d’études facilitant la"));
        assert!(!entry.interests.contains("www."));
        assert_eq!(entry.url, "www.atefficiency.be");
    }

    #[test]
    fn wrapped_names_stay_one_organisation() {
        let text = "\
American Express                Gonzalo Perez Del Arco   American Express Europe                  www.americanexpress.com
Europe
Amnesty International           Stan Brabant             les droits humains.                      www.amnesty.be
Belgique                        Montserrat Carreras
BAGO (Belgian                   Jean-Christophe Choffray   secteur privé légal des jeux en faveur de    www.bago.be
Association of Gaming           Tom De Clercq              la canalisation des joueurs, la protection
Operators)                      Emmanuel Mewissen          des consommateurs et une régulation
                                Damien Thiéry              durable.
Procter & Gamble                Vincent Vandepitte     onderzoeks-, ontwikkelings- en               https://nl-be.pg.com/
Services Company                                       coördinatieactiviteiten.                     https://fr-be.pg.com/";
        let entries = parse_lobby_layout(text);
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "American Express Europe",
                "Amnesty International Belgique",
                "BAGO (Belgian Association of Gaming Operators)",
                "Procter & Gamble Services Company",
            ]
        );
        assert_eq!(entries[1].contacts, "Stan Brabant, Montserrat Carreras");
        assert_eq!(
            entries[2].contacts,
            "Jean-Christophe Choffray, Tom De Clercq, Emmanuel Mewissen, Damien Thiéry"
        );
        assert_eq!(
            entries[3].url,
            "https://nl-be.pg.com/ https://fr-be.pg.com/"
        );
    }

    #[test]
    fn email_lines_attach_to_the_contact_above() {
        let text = "\
Nationale Kamer van                     Quentin Debray                     Belgische gerechtsdeurwaarders.            www.gerechtsdeurwaarders.be
Gerechtsdeurwaarders                    (president@nkgb-cnhb.be)
(code judiciaire, art. 555/1, 8°& 9°)   Jessica Rodriguez
                                        (jessica.rodriguez@nkgb-cnhb.be)";
        let entry = parse_one(
            text,
            "Nationale Kamer van Gerechtsdeurwaarders (code judiciaire, art. 555/1, 8°& 9°)",
        );
        assert_eq!(
            entry.contacts,
            "Quentin Debray (president@nkgb-cnhb.be), \
             Jessica Rodriguez (jessica.rodriguez@nkgb-cnhb.be)"
        );
    }

    #[test]
    fn skips_page_headers_and_footers() {
        let text = "\
                                              LOBBYREGISTER – REGISTRE DES LOBBIES
       organisme                   contactpersonen                behartigt belangen voor                           WEB
      organisation               personnes de contact              gère des intérêts pour
11.11.11                        Naima Charkaoui          koepel van de Vlaamse Noord-               www.11.be
                                                         Zuidbeweging.
laatst bijgewerkt – dernière mise à jour :
01-07-25
AbbVie                          Quentin Haxhe            entreprise pharmaceutique.
                                Gregory Willocq";
        let entries = parse_lobby_layout(text);
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["11.11.11", "AbbVie"]);
        assert!(entries[0].interests.starts_with("koepel van de Vlaamse"));
        assert_eq!(entries[1].contacts, "Quentin Haxhe, Gregory Willocq");
        assert_eq!(entries[1].url, "");
    }

    #[test]
    fn dedupe_keeps_first_row_per_name() {
        let row = |name: &str, contacts: &str| ScrapedLobby {
            name: name.into(),
            contacts: contacts.into(),
            ..Default::default()
        };
        let rows = dedupe_lobby(vec![row("B", "1"), row("A", "2"), row("B", "3")]);
        assert_eq!(rows, vec![row("A", "2"), row("B", "1")]);
    }
}
