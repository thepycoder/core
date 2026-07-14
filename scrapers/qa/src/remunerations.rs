//! QA checks for regimand remuneration staging.

use crate::types::CheckDetail;
use identity::parquet_io::{read_all_rows, read_string_column};
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

const AMOUNT_SCALE_THRESHOLD_EUR: f64 = 1_000_000.0;

pub fn run_remuneration_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let path = data_dir.join("remunerations.parquet");
    if !path.exists() {
        return Ok(details);
    }

    let mut mandate_groups: HashMap<String, Vec<usize>> = HashMap::new();
    let mut rows: Vec<RemunerationRow> = Vec::new();

    for batch in read_all_rows(&path)? {
        let first_names = read_string_column(&batch, "first_name")?;
        let last_names = read_string_column(&batch, "last_name")?;
        let years = read_string_column(&batch, "year")?;
        let mandates = read_string_column(&batch, "mandate")?;
        let institutes = read_string_column(&batch, "institute")?;
        let mins = read_string_column(&batch, "remuneration_min")?;
        let maxs = read_string_column(&batch, "remuneration_max")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            let row = RemunerationRow {
                first_name: first_names[i].clone(),
                last_name: last_names[i].clone(),
                year: years[i].clone(),
                mandate: mandates[i].clone(),
                institute: institutes[i].clone(),
                remuneration_min: mins[i].clone(),
                remuneration_max: maxs[i].clone(),
                source_url: source_urls[i].clone(),
                cache_path: cache_paths[i].clone(),
            };
            let key = mandate_business_key(&row);
            mandate_groups.entry(key).or_default().push(rows.len());
            rows.push(row);
        }
    }

    for row in &rows {
        details.extend(check_amount_valid(row)?);
        details.extend(check_amount_scale(row)?);
    }

    for indices in mandate_groups.values() {
        if indices.len() < 2 {
            continue;
        }
        details.push(duplicate_mandate_detail(&rows, indices)?);
    }

    Ok(details)
}

#[derive(Debug, Clone)]
struct RemunerationRow {
    first_name: String,
    last_name: String,
    year: String,
    mandate: String,
    institute: String,
    remuneration_min: String,
    remuneration_max: String,
    source_url: String,
    cache_path: String,
}

fn mandate_business_key(row: &RemunerationRow) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        row.last_name, row.first_name, row.year, row.mandate, row.institute
    )
}

fn entity_id(row: &RemunerationRow) -> String {
    mandate_business_key(row)
}

fn person_label(row: &RemunerationRow) -> String {
    format!("{} {}", row.first_name, row.last_name)
}

fn parse_amount(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<f64>().ok()
}

fn check_amount_valid(row: &RemunerationRow) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let min = parse_amount(&row.remuneration_min);
    let max = parse_amount(&row.remuneration_max);

    let invalid = min.is_none()
        || max.is_none()
        || !min.unwrap().is_finite()
        || !max.unwrap().is_finite()
        || min.unwrap() < 0.0
        || max.unwrap() < 0.0
        || min.unwrap() > max.unwrap();

    if !invalid {
        return Ok(Vec::new());
    }

    Ok(vec![
        CheckDetail::new(
            "remuneration.amount_valid",
            "error",
            "fail",
            format!(
                "remuneration for {} {} ({}) has invalid amount range",
                row.first_name, row.last_name, row.year
            ),
        )
        .with_entity("Remuneration", &entity_id(row))
        .with_values(
            "finite nonnegative min <= max EUR",
            format!(
                "person={}, year={}, mandate={}, institute={}, min={}, max={}",
                person_label(row),
                row.year,
                row.mandate,
                row.institute,
                row.remuneration_min,
                row.remuneration_max
            ),
        )
        .with_source(&row.source_url, &row.cache_path),
    ])
}

fn check_amount_scale(row: &RemunerationRow) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let Some(max) = parse_amount(&row.remuneration_max) else {
        return Ok(Vec::new());
    };
    if max <= AMOUNT_SCALE_THRESHOLD_EUR {
        return Ok(Vec::new());
    }

    Ok(vec![
        CheckDetail::new(
            "remuneration.amount_scale",
            "warn",
            "warn",
            format!(
                "remuneration max {:.2} EUR exceeds conservative annual threshold for {} {} ({})",
                max, row.first_name, row.last_name, row.year
            ),
        )
        .with_entity("Remuneration", &entity_id(row))
        .with_values(
            format!("max <= {AMOUNT_SCALE_THRESHOLD_EUR} EUR"),
            format!(
                "person={}, year={}, mandate={}, institute={}, min={}, max={}",
                person_label(row),
                row.year,
                row.mandate,
                row.institute,
                row.remuneration_min,
                row.remuneration_max
            ),
        )
        .with_source(&row.source_url, &row.cache_path),
    ])
}

fn duplicate_mandate_detail(
    rows: &[RemunerationRow],
    indices: &[usize],
) -> Result<CheckDetail, Box<dyn Error>> {
    let sample = &rows[indices[0]];
    let ranges: Vec<String> = indices
        .iter()
        .map(|idx| {
            let row = &rows[*idx];
            if row.remuneration_min == row.remuneration_max {
                row.remuneration_min.clone()
            } else {
                format!("{}-{}", row.remuneration_min, row.remuneration_max)
            }
        })
        .collect();

    Ok(CheckDetail::new(
        "remuneration.duplicate_mandate",
        "warn",
        "warn",
        format!(
            "logical duplicate remuneration mandate for {} {} ({})",
            sample.first_name, sample.last_name, sample.year
        ),
    )
    .with_entity("Remuneration", &entity_id(sample))
    .with_values(
        "one row per person/year/mandate/institute",
        format!(
            "person={}, year={}, mandate={}, institute={}, rows={}, amounts=[{}]",
            person_label(sample),
            sample.year,
            sample.mandate,
            sample.institute,
            indices.len(),
            ranges.join("; ")
        ),
    )
    .with_source(&sample.source_url, &sample.cache_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_row(min: &str, max: &str) -> RemunerationRow {
        RemunerationRow {
            first_name: "David".into(),
            last_name: "Clarinval".into(),
            year: "2024".into(),
            mandate: "Vice-eerste minister".into(),
            institute: "Federale Regering".into(),
            remuneration_min: min.into(),
            remuneration_max: max.into(),
            source_url: "https://public.regimand.be/".into(),
            cache_path: "remunerations/Clarinval-David-2024.html".into(),
        }
    }

    #[test]
    fn valid_amount_passes() {
        let details = check_amount_valid(&sample_row("279463.46", "279463.46")).unwrap();
        assert!(details.is_empty());
    }

    #[test]
    fn reversed_range_fails_amount_valid() {
        let details = check_amount_valid(&sample_row("100", "50")).unwrap();
        assert_eq!(details.len(), 1);
        assert_eq!(details[0].check_id, "remuneration.amount_valid");
        assert_eq!(details[0].status, "fail");
    }

    #[test]
    fn implausible_scale_warns() {
        let details = check_amount_valid(&sample_row("0", "27946346")).unwrap();
        assert!(details.is_empty());
        let details = check_amount_scale(&sample_row("0", "27946346")).unwrap();
        assert_eq!(details.len(), 1);
        assert_eq!(details[0].check_id, "remuneration.amount_scale");
    }

    #[test]
    fn duplicate_group_emits_one_detail_with_all_ranges() {
        let rows = vec![sample_row("1", "1"), sample_row("2", "2")];
        let detail = duplicate_mandate_detail(&rows, &[0, 1]).unwrap();
        assert_eq!(detail.check_id, "remuneration.duplicate_mandate");
        assert!(detail.actual.contains("amounts=[1; 2]"));
    }
}
