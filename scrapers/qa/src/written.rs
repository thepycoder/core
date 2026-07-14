//! QA checks for written Q&A (QRVA API + inline oral-written sections).

use crate::types::CheckDetail;
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::SESSION_ID;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;

pub fn run_written_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    details.extend(check_written_docname_unique(data_dir)?);
    details.extend(check_route_question_integrity(data_dir)?);
    details.extend(check_answer_question_links(data_dir)?);
    details.extend(check_oral_written_links(data_dir)?);
    details.extend(check_written_author_resolution(data_dir)?);
    details.extend(check_department_roles(data_dir)?);
    details.extend(check_published_answer_text(data_dir)?);
    Ok(details)
}

fn check_published_answer_text(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let path = data_dir.join(format!("sessions/{SESSION_ID}/written/answers.parquet"));
    if !path.exists() {
        return Ok(details);
    }

    for batch in read_all_rows(&path)? {
        let answer_ids = read_string_column(&batch, "answer_id")?;
        let routes = read_string_column(&batch, "route_id")?;
        let slots = read_string_column(&batch, "answer_slot")?;
        let statuses = read_string_column(&batch, "status")?;
        let kinds = read_string_column(&batch, "kind")?;
        let source_kinds = read_string_column(&batch, "source_kind")?;
        let text_nl = read_string_column(&batch, "text_nl")?;
        let text_fr = read_string_column(&batch, "text_fr")?;
        let publication_refs = read_string_column(&batch, "publication_ref")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            if !is_blank_published_qrva_answer(
                &statuses[i],
                &kinds[i],
                &source_kinds[i],
                &text_nl[i],
                &text_fr[i],
            ) {
                continue;
            }
            details.push(
                CheckDetail::new(
                    "written.published_answer_text_present",
                    "error",
                    "fail",
                    format!(
                        "published QRVA answer {} has no NL or FR text",
                        answer_ids[i]
                    ),
                )
                .with_entity("Answer", &answer_ids[i])
                .with_values(
                    "nonempty text_nl or text_fr",
                    format!(
                        "route={}, slot={}, publication_ref={}, status={}",
                        routes[i], slots[i], publication_refs[i], statuses[i]
                    ),
                )
                .with_source(&source_urls[i], &cache_paths[i]),
            );
        }
    }
    Ok(details)
}

fn is_blank_published_qrva_answer(
    status: &str,
    kind: &str,
    source_kind: &str,
    text_nl: &str,
    text_fr: &str,
) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "publicated" | "published"
    ) && kind == "written"
        && source_kind == "qrva"
        && text_nl.trim().is_empty()
        && text_fr.trim().is_empty()
}

fn check_written_docname_unique(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let path = data_dir.join(format!("sessions/{SESSION_ID}/written/questions.parquet"));
    if !path.exists() {
        return Ok(details);
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    for batch in read_all_rows(&path)? {
        let docnames = read_string_column(&batch, "docname")?;
        for d in docnames {
            *seen.entry(d).or_default() += 1;
        }
    }
    for (docname, count) in seen {
        if count > 1 {
            details.push(
                CheckDetail::new(
                    "written.duplicate_docname",
                    "error",
                    "fail",
                    format!("DOCNAME {docname} appears {count} times in written questions"),
                )
                .with_entity("docname", &docname),
            );
        }
    }
    Ok(details)
}

fn check_route_question_integrity(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let questions_path = data_dir.join(format!("sessions/{SESSION_ID}/written/questions.parquet"));
    let routes_path = data_dir.join(format!("sessions/{SESSION_ID}/written/routes.parquet"));
    if !questions_path.exists() || !routes_path.exists() {
        return Ok(details);
    }
    let mut question_ids: HashSet<String> = HashSet::new();
    for batch in read_all_rows(&questions_path)? {
        for id in read_string_column(&batch, "question_id")? {
            question_ids.insert(id);
        }
    }
    for batch in read_all_rows(&routes_path)? {
        let route_ids = read_string_column(&batch, "route_id")?;
        let question_ids_col = read_string_column(&batch, "question_id")?;
        for i in 0..batch.num_rows() {
            if !question_ids.contains(&question_ids_col[i]) {
                details.push(
                    CheckDetail::new(
                        "written.route_missing_question",
                        "error",
                        "fail",
                        format!(
                            "route {} references missing question {}",
                            route_ids[i], question_ids_col[i]
                        ),
                    )
                    .with_entity("route", &route_ids[i]),
                );
            }
        }
    }
    Ok(details)
}

fn check_answer_question_links(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let written_answers = data_dir.join(format!("sessions/{SESSION_ID}/written/answers.parquet"));
    if !written_answers.exists() {
        return Ok(details);
    }
    let mut route_questions: HashMap<String, String> = HashMap::new();
    let routes_path = data_dir.join(format!("sessions/{SESSION_ID}/written/routes.parquet"));
    if routes_path.exists() {
        for batch in read_all_rows(&routes_path)? {
            let route_ids = read_string_column(&batch, "route_id")?;
            let question_ids = read_string_column(&batch, "question_id")?;
            for i in 0..batch.num_rows() {
                route_questions.insert(route_ids[i].clone(), question_ids[i].clone());
            }
        }
    }
    for batch in read_all_rows(&written_answers)? {
        let answer_ids = read_string_column(&batch, "answer_id")?;
        let question_ids = read_string_column(&batch, "question_id")?;
        let route_ids = read_string_column(&batch, "route_id")?;
        for i in 0..batch.num_rows() {
            if let Some(expected_q) = route_questions.get(&route_ids[i]) {
                if expected_q != &question_ids[i] {
                    details.push(
                        CheckDetail::new(
                            "written.answer_route_question_mismatch",
                            "error",
                            "fail",
                            format!(
                                "answer {} route {} points to question {} but route expects {}",
                                answer_ids[i], route_ids[i], question_ids[i], expected_q
                            ),
                        )
                        .with_entity("answer", &answer_ids[i]),
                    );
                }
            }
        }
    }
    Ok(details)
}

fn check_oral_written_links(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let path = data_dir.join("normalized/oral_written_links.parquet");
    if !path.exists() {
        return Ok(details);
    }
    for batch in read_all_rows(&path)? {
        let written = read_string_column(&batch, "written_question_id")?;
        let canonical = read_string_column(&batch, "canonical_question_id")?;
        let statuses = read_string_column(&batch, "status")?;
        let oral_refs = read_string_column(&batch, "oral_ref")?;
        for i in 0..batch.num_rows() {
            if statuses[i] == "ambiguous" {
                details.push(
                    CheckDetail::new(
                        "written.ambiguous_oral_reference",
                        "warn",
                        "warn",
                        format!(
                            "written question {} has ambiguous oral ref {:?}",
                            written[i], oral_refs[i]
                        ),
                    )
                    .with_entity("question", &written[i]),
                );
            }
            if statuses[i] == "exact" && written[i] != canonical[i] {
                details.push(
                    CheckDetail::new(
                        "written.exact_oral_merge",
                        "info",
                        "pass",
                        format!(
                            "written {} merged to oral question {} via {}",
                            written[i], canonical[i], oral_refs[i]
                        ),
                    )
                    .with_entity("question", &written[i]),
                );
            }
        }
    }
    Ok(details)
}

fn check_written_author_resolution(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let asked_path = data_dir.join("normalized/written_asked.parquet");
    let questions_path = data_dir.join(format!("sessions/{SESSION_ID}/written/questions.parquet"));
    if !asked_path.exists() || !questions_path.exists() {
        return Ok(details);
    }
    let asked_count = read_all_rows(&asked_path)?
        .iter()
        .map(|b| b.num_rows())
        .sum::<usize>();
    let question_count = read_all_rows(&questions_path)?
        .iter()
        .map(|b| b.num_rows())
        .sum::<usize>();
    if question_count > 0 && asked_count == 0 {
        details.push(CheckDetail::new(
            "written.no_resolved_authors",
            "warn",
            "warn",
            "written questions exist but no written_asked edges were normalized",
        ));
    }
    Ok(details)
}

fn check_department_roles(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let routes_path = data_dir.join(format!("sessions/{SESSION_ID}/written/routes.parquet"));
    let external_path = data_dir.join("identity/external_persons.parquet");
    if !routes_path.exists() || !external_path.exists() {
        return Ok(details);
    }
    let mut deptnums: HashSet<String> = HashSet::new();
    for batch in read_all_rows(&routes_path)? {
        for d in read_string_column(&batch, "deptnum")? {
            if !d.is_empty() {
                deptnums.insert(d);
            }
        }
    }
    let mut external_ids: HashSet<String> = HashSet::new();
    for batch in read_all_rows(&external_path)? {
        for id in read_string_column(&batch, "external_person_id")? {
            external_ids.insert(id);
        }
    }
    for deptnum in deptnums {
        let expected = format!("ext:role:dept:{deptnum}");
        if !external_ids.contains(&expected) {
            details.push(
                CheckDetail::new(
                    "written.missing_department_role",
                    "warn",
                    "warn",
                    format!("department role {expected} not seeded in external_persons"),
                )
                .with_entity("department", &deptnum),
            );
        }
    }
    Ok(details)
}

#[cfg(test)]
mod tests {
    use super::is_blank_published_qrva_answer;

    #[test]
    fn published_qrva_answer_with_both_languages_blank_is_flagged() {
        assert!(is_blank_published_qrva_answer(
            "publicated",
            "written",
            "qrva",
            "",
            "  "
        ));
        assert!(is_blank_published_qrva_answer(
            "Published",
            "written",
            "qrva",
            "\n",
            "\t"
        ));
    }

    #[test]
    fn nonpublished_or_nonqrva_or_populated_answer_is_not_flagged() {
        assert!(!is_blank_published_qrva_answer(
            "answerReceived",
            "written",
            "qrva",
            "",
            ""
        ));
        assert!(!is_blank_published_qrva_answer(
            "published",
            "oral",
            "qrva",
            "",
            ""
        ));
        assert!(!is_blank_published_qrva_answer(
            "published",
            "written",
            "other",
            "",
            ""
        ));
        assert!(!is_blank_published_qrva_answer(
            "published",
            "written",
            "qrva",
            "Antwoord",
            ""
        ));
    }
}
