//! European-locale remuneration amount parsing for regimand.be HTML cells.

/// Parse a scraped remuneration cell into canonical min/max EUR strings.
pub fn parse_remuneration_text(raw: &str) -> Option<(String, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains("Niet bezoldigd") {
        return Some(("0".to_string(), "0".to_string()));
    }

    let without_prefix = trimmed.replace("Afgerond op ", "");
    let normalized = without_prefix.trim().to_string();
    if let Some(idx) = normalized.find(" - ") {
        let min = parse_eur_amount(&normalized[..idx])?;
        let max = parse_eur_amount(&normalized[idx + 3..])?;
        return Some((format_eur_amount(min), format_eur_amount(max)));
    }

    let value = parse_eur_amount(&normalized)?;
    let formatted = format_eur_amount(value);
    Some((formatted.clone(), formatted))
}

fn parse_eur_amount(raw: &str) -> Option<f64> {
    let stripped = raw.trim().replace(['\u{00a0}', '€'], "");
    let mut text = stripped.trim();
    if text.len() >= 3 && text[..3].eq_ignore_ascii_case("eur") {
        text = text[3..].trim();
    } else if let Some(rest) = text
        .strip_suffix("EUR")
        .or_else(|| text.strip_suffix("eur"))
    {
        text = rest.trim();
    }

    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return None;
    }

    let normalized = if let Some(comma_pos) = compact.rfind(',') {
        let int_part = compact[..comma_pos].replace('.', "");
        let frac_part = &compact[comma_pos + 1..];
        if frac_part.contains(',') || int_part.is_empty() {
            return None;
        }
        format!("{int_part}.{frac_part}")
    } else {
        compact.replace('.', "")
    };

    normalized.parse::<f64>().ok()
}

fn format_eur_amount(value: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    let cents = (value * 100.0).round() as i64;
    let whole = cents / 100;
    let frac = (cents % 100).abs();
    if frac == 0 {
        whole.to_string()
    } else if frac % 10 == 0 {
        format!("{whole}.{}", frac / 10)
    } else {
        format!("{whole}.{frac:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_large_european_amount_from_clarinval_fixture() {
        // cache/remunerations/Clarinval-David-2024.html — Vice-eerste minister row.
        let (min, max) = parse_remuneration_text("279 463,46 €").expect("amount");
        assert!((min.parse::<f64>().unwrap() - 279_463.46).abs() < 0.01);
        assert_eq!(min, max);
    }

    #[test]
    fn parses_range_endpoints_independently() {
        let (min, max) = parse_remuneration_text("1,00 - 6 129,00 EUR").expect("range");
        assert!((min.parse::<f64>().unwrap() - 1.0).abs() < 0.01);
        assert!((max.parse::<f64>().unwrap() - 6_129.0).abs() < 0.01);
    }

    #[test]
    fn unpaid_mandate_is_exact_zero() {
        assert_eq!(
            parse_remuneration_text("Niet bezoldigd"),
            Some(("0".to_string(), "0".to_string()))
        );
    }

    #[test]
    fn malformed_text_returns_none() {
        assert!(parse_remuneration_text("onbekend").is_none());
        assert!(parse_remuneration_text("").is_none());
    }

    #[test]
    fn handles_nonbreaking_spaces_and_euro_suffix() {
        let (min, max) = parse_remuneration_text("1,00 - 6\u{00a0}129,00 €").expect("range");
        assert!((min.parse::<f64>().unwrap() - 1.0).abs() < 0.01);
        assert!((max.parse::<f64>().unwrap() - 6_129.0).abs() < 0.01);
    }

    #[test]
    fn rounded_amount_prefix_is_ignored() {
        let (min, _) = parse_remuneration_text("Afgerond op 10.001,00 €").expect("amount");
        assert!((min.parse::<f64>().unwrap() - 10_001.0).abs() < 0.01);
    }
}
