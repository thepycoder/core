use crate::types::CheckDetail;
use identity::parquet_io::{read_all_rows, read_string_column};
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

const CHAIR_SUBCHAIR_OVERLAP_CHECK: &str = "commission.chair_subchair_overlap";

pub fn run_commission_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let path = data_dir.join("commissions.parquet");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut details = Vec::new();
    for batch in read_all_rows(&path)? {
        let names = read_string_column(&batch, "name")?;
        let chairs = read_string_column(&batch, "chairs")?;
        let subchairs = read_string_column(&batch, "subchairs")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            details.extend(check_chair_subchair_overlap(
                &names[i],
                &chairs[i],
                &subchairs[i],
                &source_urls[i],
                &cache_paths[i],
            ));
        }
    }

    Ok(details)
}

/// Split CSV role lists, case-fold for comparison (preserving accents on the
/// reported name), and emit one fail detail per person listed in both roles.
fn check_chair_subchair_overlap(
    commission: &str,
    chairs: &str,
    subchairs: &str,
    source_url: &str,
    cache_path: &str,
) -> Vec<CheckDetail> {
    let chair_names = split_names(chairs);
    let subchair_names = split_names(subchairs);
    if chair_names.is_empty() || subchair_names.is_empty() {
        return Vec::new();
    }

    let mut chair_by_fold: HashMap<String, &str> = HashMap::new();
    for name in &chair_names {
        chair_by_fold
            .entry(name.to_lowercase())
            .or_insert(name.as_str());
    }

    let mut details = Vec::new();
    let mut seen_folds = std::collections::HashSet::new();
    for name in &subchair_names {
        let fold = name.to_lowercase();
        if !seen_folds.insert(fold.clone()) {
            continue;
        }
        if let Some(chair_name) = chair_by_fold.get(&fold) {
            details.push(
                CheckDetail::new(
                    CHAIR_SUBCHAIR_OVERLAP_CHECK,
                    "error",
                    "fail",
                    format!(
                        "commission `{commission}` lists `{chair_name}` as both chair and subchair"
                    ),
                )
                .with_entity("Person", *chair_name)
                .with_values(
                    "disjoint chair and subchair roles",
                    format!("commission={commission}"),
                )
                .with_source(source_url, cache_path),
            );
        }
    }

    details
}

fn split_names(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_emits_one_detail_per_person() {
        let details = check_chair_subchair_overlap(
            "justitie",
            "Ismaël Nuino, Steven Matheï, Kristien Van Vaerenbergh",
            "Steven Matheï, Kristien Van Vaerenbergh",
            "https://example.com/justitie",
            "commissions/details/justitie.html",
        );
        assert_eq!(details.len(), 2);
        assert!(
            details
                .iter()
                .all(|d| d.check_id == CHAIR_SUBCHAIR_OVERLAP_CHECK && d.status == "fail")
        );
        let ids: Vec<_> = details.iter().map(|d| d.entity_id.as_str()).collect();
        assert!(ids.contains(&"Steven Matheï"));
        assert!(ids.contains(&"Kristien Van Vaerenbergh"));
    }

    #[test]
    fn disjoint_roles_pass() {
        let details = check_chair_subchair_overlap(
            "justitie",
            "Ismaël Nuino",
            "Steven Matheï, Kristien Van Vaerenbergh",
            "https://example.com/justitie",
            "commissions/details/justitie.html",
        );
        assert!(details.is_empty());
    }

    #[test]
    fn empty_role_lists_do_not_emit_empty_name_findings() {
        assert!(
            check_chair_subchair_overlap(
                "naturalisaties",
                "Khalil Aouasti",
                "",
                "https://example.com/naturalisaties",
                "commissions/details/naturalisaties.html",
            )
            .is_empty()
        );
        assert!(
            check_chair_subchair_overlap(
                "empty",
                "",
                "",
                "https://example.com",
                "commissions/details/empty.html",
            )
            .is_empty()
        );
        assert!(
            check_chair_subchair_overlap(
                "commas",
                ", ,",
                ", ",
                "https://example.com",
                "commissions/details/empty.html",
            )
            .is_empty()
        );
    }

    #[test]
    fn case_fold_compares_without_stripping_accents() {
        let details = check_chair_subchair_overlap(
            "justitie",
            "Ismaël Nuino",
            "ismaël nuino",
            "https://example.com/justitie",
            "commissions/details/justitie.html",
        );
        assert_eq!(details.len(), 1);
        assert_eq!(details[0].entity_id, "Ismaël Nuino");
    }
}
