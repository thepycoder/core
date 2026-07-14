use crate::types::CheckDetail;
use identity::parquet_io::{read_all_rows, read_string_column};
use regex::Regex;
use std::error::Error;
use std::path::Path;
use std::sync::LazyLock;

static URL_TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:https?://|www\.)[\w./\-]+").unwrap());
static DOMAIN_FRAGMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:www\.|[a-z0-9-]+\.(?:be|com|org|eu|net|int))\S*").unwrap());

const URL_PLACEMENT_CHECK: &str = "lobby.url_placement";
const COLUMN_BLEED_CHECK: &str = "lobby.column_bleed";

pub fn run_lobby_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let path = data_dir.join("lobby.parquet");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut details = Vec::new();
    for batch in read_all_rows(&path)? {
        let names = read_string_column(&batch, "name")?;
        let contacts = read_string_column(&batch, "contacts")?;
        let interests = read_string_column(&batch, "interests")?;
        let urls = read_string_column(&batch, "url")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            details.extend(check_url_placement(
                &names[i],
                &contacts[i],
                &interests[i],
                &urls[i],
                &source_urls[i],
                &cache_paths[i],
            ));
            details.extend(check_column_bleed(
                &names[i],
                &contacts[i],
                &interests[i],
                &urls[i],
                &source_urls[i],
                &cache_paths[i],
            ));
        }
    }

    Ok(details)
}

fn check_url_placement(
    name: &str,
    contacts: &str,
    interests: &str,
    url: &str,
    source_url: &str,
    cache_path: &str,
) -> Vec<CheckDetail> {
    let mut details = Vec::new();

    for (field, value) in [("contacts", contacts), ("interests", interests)] {
        for mat in URL_TOKEN_RE.find_iter(value) {
            details.push(
                lobby_detail(
                    URL_PLACEMENT_CHECK,
                    "warn",
                    name,
                    field,
                    mat.as_str(),
                    url,
                    source_url,
                    cache_path,
                    format!(
                        "organisation `{name}` has URL token `{token}` in `{field}`",
                        token = mat.as_str()
                    ),
                ),
            );
        }
    }

    let url_tokens: Vec<_> = URL_TOKEN_RE.find_iter(url).map(|m| m.as_str()).collect();
    if url_tokens.len() > 3 {
        details.push(lobby_detail(
            URL_PLACEMENT_CHECK,
            "warn",
            name,
            "url",
            &url_tokens.join(" "),
            "single canonical url field",
            source_url,
            cache_path,
            format!(
                "organisation `{name}` url field contains {} distinct URL tokens",
                url_tokens.len()
            ),
        ));
    }

    details
}

fn check_column_bleed(
    name: &str,
    contacts: &str,
    interests: &str,
    url: &str,
    source_url: &str,
    cache_path: &str,
) -> Vec<CheckDetail> {
    if url.is_empty() {
        return Vec::new();
    }

    let canonical = url.to_lowercase();
    let mut details = Vec::new();

    for (field, value) in [("contacts", contacts), ("interests", interests)] {
        for mat in URL_TOKEN_RE.find_iter(value) {
            let token = mat.as_str();
            let lower = token.to_lowercase();
            if lower.len() < canonical.len() && canonical.starts_with(&lower) {
                details.push(lobby_detail(
                    COLUMN_BLEED_CHECK,
                    "warn",
                    name,
                    field,
                    token,
                    url,
                    source_url,
                    cache_path,
                    format!(
                        "organisation `{name}` has truncated URL `{token}` in `{field}` matching canonical url",
                    ),
                ));
            }
        }

        for mat in DOMAIN_FRAGMENT_RE.find_iter(value) {
            let token = mat.as_str();
            if URL_TOKEN_RE.is_match(token) {
                continue;
            }
            let lower = token.to_lowercase();
            if canonical.starts_with(&lower) || (lower.len() >= 5 && canonical.contains(&lower)) {
                details.push(lobby_detail(
                    COLUMN_BLEED_CHECK,
                    "warn",
                    name,
                    field,
                    token,
                    url,
                    source_url,
                    cache_path,
                    format!(
                        "organisation `{name}` has URL fragment `{token}` in `{field}` matching canonical url",
                    ),
                ));
            }
        }
    }

    details
}

fn lobby_detail(
    check_id: &str,
    status: &str,
    name: &str,
    field: &str,
    token: &str,
    expected: &str,
    source_url: &str,
    cache_path: &str,
    message: String,
) -> CheckDetail {
    CheckDetail::new(check_id, status, status, message)
        .with_entity("lobby_organisation", name)
        .with_values(
            format!("field={field} expected={expected}"),
            format!("token={token}"),
        )
        .with_source(source_url, cache_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_in_interests_triggers_url_placement() {
        let details = check_url_placement(
            "Example Org",
            "Jane Doe",
            "lobbying for www.example.be policy",
            "www.example.be",
            "https://example.com",
            "lobby/lobbyregister.pdf",
        );
        assert_eq!(details.len(), 1);
        assert_eq!(details[0].check_id, URL_PLACEMENT_CHECK);
        assert!(details[0].message.contains("interests"));
    }

    #[test]
    fn clean_row_has_no_findings() {
        let details = check_url_placement(
            "Agoria",
            "Bart Steukers",
            "lidbedrijven van Agoria",
            "www.agoria.be",
            "https://example.com",
            "lobby/lobbyregister.pdf",
        );
        assert!(details.is_empty());

        let bleed = check_column_bleed(
            "Agoria",
            "Bart Steukers",
            "lidbedrijven van Agoria",
            "www.agoria.be",
            "https://example.com",
            "lobby/lobbyregister.pdf",
        );
        assert!(bleed.is_empty());
    }

    #[test]
    fn truncated_url_fragment_triggers_column_bleed() {
        let details = check_column_bleed(
            "Air Cargo Belgium",
            "Freek de Witte",
            "bevordering www.aircargobe",
            "www.aircargobelgium.be",
            "https://example.com",
            "lobby/lobbyregister.pdf",
        );
        assert_eq!(details.len(), 1);
        assert_eq!(details[0].check_id, COLUMN_BLEED_CHECK);
    }
}
