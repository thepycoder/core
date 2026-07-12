use crate::io::{read_coverage_baselines, write_coverage_baselines};
use crate::types::CoverageBaselineRow;
use std::error::Error;
use std::path::Path;

pub fn update_coverage_baseline(
    rows: &[CoverageBaselineRow],
    baseline_path: &Path,
) -> Result<(), Box<dyn Error>> {
    write_coverage_baselines(baseline_path, rows)
}

pub fn load_coverage_baseline(
    baseline_path: &Path,
) -> Result<std::collections::HashMap<(String, String), CoverageBaselineRow>, Box<dyn Error>> {
    let rows = read_coverage_baselines(baseline_path)?;
    Ok(rows
        .into_iter()
        .map(|r| ((r.meeting_kind.clone(), r.meeting_id.clone()), r))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn roundtrip_coverage_baseline() {
        let rows = vec![CoverageBaselineRow {
            meeting_kind: "plenary".to_string(),
            meeting_id: "117".to_string(),
            source_words: 50_000,
            saved_words: 25_000,
            ratio: 0.5,
            updated_at: Utc::now().to_rfc3339(),
        }];
        let tmp = std::env::temp_dir().join("coverage_baseline_test.parquet");
        update_coverage_baseline(&rows, &tmp).unwrap();
        let loaded = load_coverage_baseline(&tmp).unwrap();
        assert_eq!(loaded.len(), 1);
        let key = ("plenary".to_string(), "117".to_string());
        assert!((loaded[&key].ratio - 0.5).abs() < f64::EPSILON);
        let _ = std::fs::remove_file(tmp);
    }
}
