use crawl::answer_io::AnswerDraft;
use crawl::qrva_text::{
    docname_internal_id, flatten_qrva_text, parse_aut_actr_id, parse_oral_refs, qrva_answer_id,
    qrva_detail_url, route_id, written_question_id,
};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct WrittenQuestionDraft {
    pub question_id: String,
    pub session_id: u32,
    pub docname: String,
    pub kind: String,
    pub author_actr_id: String,
    pub author_raw: String,
    pub depot_date: String,
    pub deadline_date: String,
    pub lang: String,
    pub title_nl: String,
    pub title_fr: String,
    pub text_nl: String,
    pub text_fr: String,
    pub main_thesa_nl: String,
    pub main_thesa_fr: String,
    pub oral_refs: String,
    pub qrva_route_ids: String,
    pub internal_ids: String,
    pub source_url: String,
    pub cache_path: String,
}

#[derive(Debug, Clone)]
pub struct WrittenRouteDraft {
    pub route_id: String,
    pub question_id: String,
    pub session_id: u32,
    pub qrva_id: i64,
    pub sdocname: String,
    pub docname: String,
    pub deptnum: String,
    pub deptpres: String,
    pub dept_title_nl: String,
    pub dept_title_fr: String,
    pub subdept_nl: String,
    pub subdept_fr: String,
    pub questnum: String,
    pub statusq: String,
    pub source_url: String,
    pub cache_path: String,
}

#[derive(Debug, Clone)]
pub struct WrittenStagingOutput {
    pub questions: Vec<WrittenQuestionDraft>,
    pub routes: Vec<WrittenRouteDraft>,
    pub answers: Vec<AnswerDraft>,
}

fn field_str(item: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(v) = item.get(*key) {
            if let Some(s) = v.as_str() {
                return s.trim().to_string();
            }
            if let Some(n) = v.as_i64() {
                return n.to_string();
            }
        }
    }
    String::new()
}

fn field_i64(item: &Value, keys: &[&str]) -> i64 {
    for key in keys {
        if let Some(v) = item.get(*key) {
            if let Some(n) = v.as_i64() {
                return n;
            }
            if let Some(s) = v.as_str()
                && let Ok(n) = s.parse()
            {
                return n;
            }
        }
    }
    0
}

fn field_text(item: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(v) = item.get(*key) {
            let flat = flatten_qrva_text(v);
            if !flat.is_empty() {
                return flat;
            }
        }
    }
    String::new()
}

pub fn parse_route_record(
    session_id: u32,
    item: &Value,
    cache_path: &str,
) -> (WrittenRouteDraft, Vec<AnswerDraft>) {
    let qrva_id = field_i64(item, &["ID", "id"]);
    let docname = field_str(item, &["DOCNAME", "docname"]);
    let sdocname = field_str(item, &["SDOCNAME", "sdocname"]);
    let route = route_id(session_id, qrva_id);
    let question_id = written_question_id(session_id, &docname);
    let source_url = if sdocname.is_empty() {
        qrva_detail_url(&format!("{session_id}--unknown-{qrva_id}"))
    } else {
        qrva_detail_url(&sdocname)
    };

    let route_row = WrittenRouteDraft {
        route_id: route.clone(),
        question_id: question_id.clone(),
        session_id,
        qrva_id,
        sdocname,
        docname: docname.clone(),
        deptnum: field_str(item, &["DEPTNUM", "deptnum"]),
        deptpres: field_str(item, &["DEPTPRES", "deptpres"]),
        dept_title_nl: field_str(item, &["DEPTN", "deptn"]),
        dept_title_fr: field_str(item, &["DEPTF", "deptf"]),
        subdept_nl: field_str(item, &["SUBDEPTN", "subdeptn"]),
        subdept_fr: field_str(item, &["SUBDEPTF", "subdeptf"]),
        questnum: field_str(item, &["QUESTNUM", "questnum"]),
        statusq: field_str(item, &["STATUSQ", "statusq"]),
        source_url: source_url.clone(),
        cache_path: cache_path.to_string(),
    };

    let mut answers = Vec::new();
    for slot in 1..=4u8 {
        let status = field_str(
            item,
            &[&format!("STATUSA{slot}"), &format!("statusa{slot}")],
        );
        let text_nl = field_text(item, &[&format!("TEXTA{slot}N"), &format!("texta{slot}n")]);
        let text_fr = field_text(item, &[&format!("TEXTA{slot}F"), &format!("texta{slot}f")]);
        if status.is_empty() && text_nl.is_empty() && text_fr.is_empty() {
            continue;
        }
        answers.push(AnswerDraft {
            answer_id: qrva_answer_id(&route, slot),
            question_id: question_id.clone(),
            route_id: route.clone(),
            session_id,
            meeting_id: String::new(),
            meeting_kind: String::new(),
            agenda_id: String::new(),
            answer_slot: slot,
            kind: "written".to_string(),
            text_nl,
            text_fr,
            status,
            answer_num: field_str(item, &[&format!("NUMA{slot}"), &format!("numa{slot}")]),
            publication_ref: field_str(
                item,
                &[&format!("PUBLICA{slot}"), &format!("publica{slot}")],
            ),
            casa: field_str(item, &[&format!("CASA{slot}"), &format!("casa{slot}")]),
            source_kind: "qrva".to_string(),
            confidence: "exact".to_string(),
            source_url: source_url.clone(),
            cache_path: cache_path.to_string(),
        });
    }

    (route_row, answers)
}

pub fn build_staging_from_records(
    session_id: u32,
    records: &[(Value, String)],
) -> WrittenStagingOutput {
    let mut by_docname: BTreeMap<String, Vec<(Value, String)>> = BTreeMap::new();
    for (item, cache_path) in records {
        let docname = field_str(item, &["DOCNAME", "docname"]);
        if docname.is_empty() {
            continue;
        }
        by_docname
            .entry(docname)
            .or_default()
            .push((item.clone(), cache_path.clone()));
    }

    let mut questions = Vec::new();
    let mut routes = Vec::new();
    let mut answers = Vec::new();

    for (docname, group) in by_docname {
        let question_id = written_question_id(session_id, &docname);
        let first = &group[0].0;
        let first_cache = group[0].1.clone();

        let title_nl = field_str(first, &["TITN", "titn"]);
        let title_fr = field_str(first, &["TITF", "titf"]);
        let text_nl = field_text(first, &["TEXTQN", "textqn"]);
        let text_fr = field_text(first, &["TEXTQF", "textqf"]);
        let mut oral_set = parse_oral_refs(&title_nl);
        for r in parse_oral_refs(&title_fr) {
            if !oral_set.contains(&r) {
                oral_set.push(r);
            }
        }

        let mut route_ids = Vec::new();
        for (item, cache_path) in &group {
            let (route_row, mut route_answers) = parse_route_record(session_id, item, cache_path);
            route_ids.push(route_row.route_id.clone());
            routes.push(route_row);
            answers.append(&mut route_answers);
        }

        let author_raw = field_str(first, &["AUT", "aut"]);
        questions.push(WrittenQuestionDraft {
            question_id,
            session_id,
            docname: docname.clone(),
            kind: "written".to_string(),
            author_actr_id: parse_aut_actr_id(&author_raw).unwrap_or_default(),
            author_raw,
            depot_date: field_str(first, &["DEPOTDAT", "depotdat"]),
            deadline_date: field_str(first, &["DELAIDAT", "delaidat"]),
            lang: field_str(first, &["LANG", "lang"]),
            title_nl,
            title_fr,
            text_nl,
            text_fr,
            main_thesa_nl: field_str(first, &["MAIN_THESAN", "mainthesan"]),
            main_thesa_fr: field_str(first, &["MAIN_THESAF", "mainthesaf"]),
            oral_refs: oral_set.join(","),
            qrva_route_ids: route_ids.join(","),
            internal_ids: docname_internal_id(&docname),
            source_url: qrva_detail_url(&field_str(first, &["SDOCNAME", "sdocname"])),
            cache_path: first_cache,
        });
    }

    WrittenStagingOutput {
        questions,
        routes,
        answers,
    }
}

pub fn records_from_search_page(body: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    if let Some(items) = body.get("items").and_then(|v| v.as_array()) {
        for item in items {
            if item.get("DOCNAME").is_some() || item.get("docname").is_some() {
                out.push(item.clone());
            } else if let Some(inner) = item.get("items").and_then(|v| v.as_array()) {
                out.extend(inner.iter().cloned());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn groups_multi_recipient_docname() {
        let records = vec![
            (
                json!({
                    "ID": 1,
                    "DOCNAME": "2025202606531",
                    "SDOCNAME": "56--8-0953-2025202606531",
                    "AUT": "Tine Gielis (07772)",
                    "TITN": "Controles",
                    "TITF": "Contrôles",
                    "TEXTQN": {"br": ["NL body"]},
                    "TEXTQF": {"br": ["FR body"]},
                    "DEPTNUM": "1447",
                    "DEPTN": "Minister NL",
                    "DEPTF": "Ministre FR",
                    "QUESTNUM": "953",
                    "STATUSQ": "transmited"
                }),
                "cache/a.json".to_string(),
            ),
            (
                json!({
                    "ID": 2,
                    "DOCNAME": "2025202606531",
                    "SDOCNAME": "56-B045-7-0930-2025202606531",
                    "AUT": "Tine Gielis (07772)",
                    "TITN": "Controles",
                    "TITF": "Contrôles",
                    "TEXTQN": {"br": ["NL body"]},
                    "TEXTQF": {"br": ["FR body"]},
                    "DEPTNUM": "1446",
                    "DEPTN": "Minister NL 2",
                    "DEPTF": "Ministre FR 2",
                    "QUESTNUM": "930",
                    "STATUSQ": "answerReceived",
                    "STATUSA1": "publicated",
                    "TEXTA1N": {"br": ["Antwoord NL"]},
                    "TEXTA1F": {"br": ["Réponse FR"]},
                    "NUMA1": "1"
                }),
                "cache/b.json".to_string(),
            ),
        ];
        let out = build_staging_from_records(56, &records);
        assert_eq!(out.questions.len(), 1);
        assert_eq!(out.routes.len(), 2);
        assert_eq!(out.answers.len(), 1);
        assert_eq!(out.questions[0].question_id, "56_written_2025202606531");
    }
}
