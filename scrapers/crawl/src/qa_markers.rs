//! S6 crosscheck: source speaker markers vs extracted utterance rows per meeting.

use crate::agenda_timeline::MeetingKind;
use crate::meeting_report::extract_utterances_from_document;
use crate::report_blocks::{parse_report_blocks, read_report_html};
use crate::speaker_parse::count_source_markers;
use scraper::Html;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerCheckResult {
    pub meeting_kind: &'static str,
    pub meeting_id: u32,
    pub turn_markers: usize,
    pub chair_markers: usize,
    pub utterance_rows: usize,
    pub ok: bool,
    pub detail: String,
}

pub fn check_markers_vs_utterances(
    cache_path: &Path,
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
) -> Result<MarkerCheckResult, Box<dyn std::error::Error>> {
    let html = read_report_html(cache_path)?;
    let document = Html::parse_document(&html);
    let blocks = parse_report_blocks(&document);
    let (turn_markers, chair_markers) = count_source_markers(&blocks);
    let marker_total = turn_markers + chair_markers;

    let utterances = extract_utterances_from_document(
        &document,
        meeting_kind,
        session_id,
        meeting_id,
        "fixture",
        cache_path.to_string_lossy().as_ref(),
    );
    let utterance_rows = utterances.len();

    // Bilingual duplicate turns and merged chair lines reduce rows vs raw markers.
    let ok = if marker_total == 0 {
        utterance_rows == 0
    } else {
        utterance_rows > 0 && utterance_rows <= marker_total && utterance_rows * 2 >= turn_markers
    };

    let detail = format!(
        "markers turn={turn_markers} chair={chair_markers} total={marker_total}; utterances={utterance_rows}"
    );

    Ok(MarkerCheckResult {
        meeting_kind: meeting_kind.as_str(),
        meeting_id,
        turn_markers,
        chair_markers,
        utterance_rows,
        ok,
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_cache_root() -> Option<std::path::PathBuf> {
        let candidates = [
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../partijgedrag/core/cache/sessions/56/meetings"),
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../cache/sessions/56/meetings"),
        ];
        candidates.into_iter().find(|p| p.exists())
    }

    fn run_fixture(kind: &str, meeting_id: u32) -> Option<MarkerCheckResult> {
        let root = fixture_cache_root()?;
        let path = root.join(kind).join(format!("56-{meeting_id}.html"));
        if !path.exists() {
            return None;
        }
        let meeting_kind = match kind {
            "plenary" => MeetingKind::Plenary,
            "commission" => MeetingKind::Commission,
            _ => return None,
        };
        check_markers_vs_utterances(&path, meeting_kind, 56, meeting_id).ok()
    }

    #[test]
    fn fixture_baseline_utterance_counts() {
        const BASELINE: &[(&str, u32, usize)] = &[
            ("plenary", 2, 29),
            ("plenary", 19, 98),
            ("plenary", 50, 123),
            ("plenary", 52, 129),
            ("plenary", 57, 202),
            ("plenary", 100, 96),
            ("plenary", 117, 108),
            ("plenary", 120, 82),
            ("plenary", 133, 44),
            ("commission", 2, 30),
            ("commission", 15, 16),
            ("commission", 17, 114),
            ("commission", 19, 42),
            ("commission", 30, 31),
            ("commission", 57, 57),
        ];

        let mut checked = 0usize;
        for (kind, meeting_id, expected_rows) in BASELINE {
            let Some(result) = run_fixture(kind, *meeting_id) else {
                continue;
            };
            checked += 1;
            assert_eq!(
                result.utterance_rows, *expected_rows,
                "{kind} {meeting_id}: expected {expected_rows} utterance rows, got {} ({})",
                result.utterance_rows, result.detail
            );
        }
        if checked == 0 {
            return;
        }
        assert_eq!(
            checked,
            BASELINE.len(),
            "not all baseline fixtures were found"
        );
    }

    #[test]
    fn commission_408_coenegrachts_kerncentrales() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/sessions/56/meetings/commission/56-408.html");
        if !path.exists() {
            return;
        }
        let html = read_report_html(&path).unwrap();
        let document = Html::parse_document(&html);
        let utterances = extract_utterances_from_document(
            &document,
            MeetingKind::Commission,
            56,
            408,
            "fixture",
            path.to_string_lossy().as_ref(),
        );
        let found: Vec<_> = utterances
            .iter()
            .filter(|u| {
                u.raw_speaker.contains("Coenegrachts")
                    && u.text.contains("kerncentrales over te nemen")
            })
            .collect();
        assert!(
            !found.is_empty(),
            "expected Coenegrachts kerncentrales utterance among {} rows",
            utterances.len()
        );
    }

    #[test]
    fn s6_markers_match_utterances_on_fixtures() {
        let plenary = [2, 19, 50, 52, 57, 100, 117, 120, 133];
        let commission = [2, 15, 17, 19, 30, 57];
        let mut checked = 0usize;
        let mut failures = Vec::new();

        for id in plenary {
            if let Some(result) = run_fixture("plenary", id) {
                checked += 1;
                if !result.ok {
                    failures.push(format!("plenary {id}: {}", result.detail));
                }
            }
        }
        for id in commission {
            if let Some(result) = run_fixture("commission", id) {
                checked += 1;
                if !result.ok {
                    failures.push(format!("commission {id}: {}", result.detail));
                }
            }
        }

        if checked == 0 {
            return;
        }

        assert!(
            failures.is_empty(),
            "S6 failures ({} meetings checked):\n{}",
            checked,
            failures.join("\n")
        );
    }

    #[test]
    fn question_threads_non_empty_on_ic017_and_ip117() {
        let root = match fixture_cache_root() {
            Some(r) => r,
            None => return,
        };
        for (kind, meeting_id) in [("plenary", 117u32), ("commission", 17)] {
            let path = root.join(kind).join(format!("56-{meeting_id}.html"));
            if !path.exists() {
                continue;
            }
            let meeting_kind = if kind == "plenary" {
                MeetingKind::Plenary
            } else {
                MeetingKind::Commission
            };
            let html = read_report_html(&path).unwrap();
            let document = Html::parse_document(&html);
            let utterances = extract_utterances_from_document(
                &document,
                meeting_kind,
                56,
                meeting_id,
                "fixture",
                path.to_string_lossy().as_ref(),
            );
            if kind == "plenary" {
                let question_rows: Vec<_> = utterances
                    .iter()
                    .filter(|u| u.item_kind == "question")
                    .collect();
                assert!(!question_rows.is_empty(), "plenary 117 question utterances");
            } else {
                let question_rows: Vec<_> = utterances
                    .iter()
                    .filter(|u| u.item_kind == "question" && !u.item_id.is_empty())
                    .collect();
                assert!(
                    !question_rows.is_empty(),
                    "commission 17 question utterances"
                );
            }
        }
    }

    #[test]
    fn meeting_thread_non_empty_on_ip019() {
        let root = match fixture_cache_root() {
            Some(r) => r,
            None => return,
        };
        let path = root.join("plenary").join("56-19.html");
        if !path.exists() {
            return;
        }
        let html = read_report_html(&path).unwrap();
        let document = Html::parse_document(&html);
        let utterances = extract_utterances_from_document(
            &document,
            MeetingKind::Plenary,
            56,
            19,
            "fixture",
            path.to_string_lossy().as_ref(),
        );
        assert!(
            utterances.len() >= 50,
            "ip019 meeting thread should have substantial speech"
        );
    }
}
