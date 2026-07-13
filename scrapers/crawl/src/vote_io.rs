//! Parquet writers for vote decisions, results, tallies, and members.

use crate::vote_types::{
    UnresolvedVoteEventDraft, VoteAssemblyOutput, VoteDecisionDraft, VoteResultDraft,
    VoteResultMemberDraft, VoteTallyDraft,
};
use arrow::array::{ArrayRef, BooleanArray, RecordBatch, StringArray, UInt32Array};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

macro_rules! col {
    ($rows:expr, $f:expr) => {
        Arc::new(StringArray::from($rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
    };
}

macro_rules! u32_col {
    ($rows:expr, $f:expr) => {
        Arc::new(UInt32Array::from($rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
    };
}

macro_rules! bool_col {
    ($rows:expr, $f:expr) => {
        Arc::new(BooleanArray::from($rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
    };
}

fn write_parquet(
    path: &Path,
    schema: Arc<Schema>,
    columns: Vec<ArrayRef>,
) -> Result<(), Box<dyn Error>> {
    let batch = RecordBatch::try_new(schema.clone(), columns)?;
    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

pub fn write_votes_parquet(path: &Path, rows: &[VoteDecisionDraft]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("vote_id", DataType::Utf8, false),
        Field::new("result_id", DataType::Utf8, false),
        Field::new("session_id", DataType::UInt32, false),
        Field::new("meeting_id", DataType::UInt32, false),
        Field::new("date", DataType::Utf8, false),
        Field::new("seq", DataType::UInt32, false),
        Field::new("title_nl", DataType::Utf8, false),
        Field::new("title_fr", DataType::Utf8, false),
        Field::new("method", DataType::Utf8, false),
        Field::new("status", DataType::Utf8, false),
        Field::new("outcome", DataType::Utf8, false),
        Field::new("dossier_id", DataType::Utf8, false),
        Field::new("document_id", DataType::Utf8, false),
        Field::new("motion_id", DataType::Utf8, false),
        Field::new("source_roll_call_number", DataType::Utf8, false),
        Field::new("reuses_result", DataType::Boolean, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));
    write_parquet(
        path,
        schema,
        vec![
            col!(rows, |r| r.vote_id.clone()),
            col!(rows, |r| r.result_id.clone()),
            u32_col!(rows, |r| r.session_id),
            u32_col!(rows, |r| r.meeting_id),
            col!(rows, |r| r.date.clone()),
            u32_col!(rows, |r| r.seq),
            col!(rows, |r| r.title_nl.clone()),
            col!(rows, |r| r.title_fr.clone()),
            col!(rows, |r| r.method.clone()),
            col!(rows, |r| r.status.clone()),
            col!(rows, |r| r.outcome.clone()),
            col!(rows, |r| r.dossier_id.clone()),
            col!(rows, |r| r.document_id.clone()),
            col!(rows, |r| r.motion_id.clone()),
            col!(rows, |r| r.source_roll_call_number.clone()),
            bool_col!(rows, |r| r.reuses_result),
            col!(rows, |r| r.source_url.clone()),
            col!(rows, |r| r.cache_path.clone()),
        ],
    )
}

pub fn write_vote_results_parquet(
    path: &Path,
    rows: &[VoteResultDraft],
) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("result_id", DataType::Utf8, false),
        Field::new("session_id", DataType::UInt32, false),
        Field::new("meeting_id", DataType::UInt32, false),
        Field::new("seq", DataType::UInt32, false),
        Field::new("method", DataType::Utf8, false),
        Field::new("named", DataType::Boolean, false),
        Field::new("status", DataType::Utf8, false),
        Field::new("outcome", DataType::Utf8, false),
        Field::new("source_roll_call_number", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));
    write_parquet(
        path,
        schema,
        vec![
            col!(rows, |r| r.result_id.clone()),
            u32_col!(rows, |r| r.session_id),
            u32_col!(rows, |r| r.meeting_id),
            u32_col!(rows, |r| r.seq),
            col!(rows, |r| r.method.clone()),
            bool_col!(rows, |r| r.named),
            col!(rows, |r| r.status.clone()),
            col!(rows, |r| r.outcome.clone()),
            col!(rows, |r| r.source_roll_call_number.clone()),
            col!(rows, |r| r.source_url.clone()),
            col!(rows, |r| r.cache_path.clone()),
        ],
    )
}

pub fn write_vote_tallies_parquet(
    path: &Path,
    rows: &[VoteTallyDraft],
) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("result_id", DataType::Utf8, false),
        Field::new("tally_kind", DataType::Utf8, false),
        Field::new("option_key", DataType::Utf8, false),
        Field::new("label_nl", DataType::Utf8, false),
        Field::new("label_fr", DataType::Utf8, false),
        Field::new("dimension", DataType::Utf8, false),
        Field::new("count", DataType::UInt32, false),
        Field::new("selected", DataType::Boolean, false),
    ]));
    write_parquet(
        path,
        schema,
        vec![
            col!(rows, |r| r.result_id.clone()),
            col!(rows, |r| r.tally_kind.clone()),
            col!(rows, |r| r.option_key.clone()),
            col!(rows, |r| r.label_nl.clone()),
            col!(rows, |r| r.label_fr.clone()),
            col!(rows, |r| r.dimension.clone()),
            u32_col!(rows, |r| r.count),
            bool_col!(rows, |r| r.selected),
        ],
    )
}

pub fn write_vote_result_members_parquet(
    path: &Path,
    rows: &[VoteResultMemberDraft],
) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("result_id", DataType::Utf8, false),
        Field::new("position", DataType::Utf8, false),
        Field::new("seq", DataType::UInt32, false),
        Field::new("raw_name", DataType::Utf8, false),
    ]));
    write_parquet(
        path,
        schema,
        vec![
            col!(rows, |r| r.result_id.clone()),
            col!(rows, |r| r.position.clone()),
            u32_col!(rows, |r| r.seq),
            col!(rows, |r| r.raw_name.clone()),
        ],
    )
}

pub fn write_unresolved_vote_events_parquet(
    path: &Path,
    rows: &[UnresolvedVoteEventDraft],
) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("session_id", DataType::UInt32, false),
        Field::new("meeting_id", DataType::UInt32, false),
        Field::new("event_kind", DataType::Utf8, false),
        Field::new("source_roll_call_number", DataType::Utf8, false),
        Field::new("block_start", DataType::UInt32, false),
        Field::new("block_end", DataType::UInt32, false),
        Field::new("reason", DataType::Utf8, false),
        Field::new("evidence_text", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));
    write_parquet(
        path,
        schema,
        vec![
            u32_col!(rows, |r| r.session_id),
            u32_col!(rows, |r| r.meeting_id),
            col!(rows, |r| r.event_kind.clone()),
            col!(rows, |r| r.source_roll_call_number.clone()),
            u32_col!(rows, |r| r.block_start),
            u32_col!(rows, |r| r.block_end),
            col!(rows, |r| r.reason.clone()),
            col!(rows, |r| r.evidence_text.clone()),
            col!(rows, |r| r.source_url.clone()),
            col!(rows, |r| r.cache_path.clone()),
        ],
    )
}

pub fn write_vote_bundle(dir: &Path, bundle: &VoteAssemblyOutput) -> Result<(), Box<dyn Error>> {
    write_votes_parquet(&dir.join("votes.parquet"), &bundle.decisions)?;
    write_vote_results_parquet(&dir.join("vote_results.parquet"), &bundle.results)?;
    write_vote_tallies_parquet(&dir.join("vote_tallies.parquet"), &bundle.tallies)?;
    write_vote_result_members_parquet(&dir.join("vote_result_members.parquet"), &bundle.members)?;
    write_unresolved_vote_events_parquet(
        &dir.join("vote_unresolved_events.parquet"),
        &bundle.unresolved_events,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    fn schema(path: &Path) -> Arc<Schema> {
        let file = File::open(path).unwrap();
        ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .schema()
            .clone()
    }

    #[test]
    fn vote_tables_use_typed_arrow_columns() {
        let dir = std::env::temp_dir().join(format!("crawl_vote_io_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let votes = dir.join("votes.parquet");
        write_votes_parquet(
            &votes,
            &[VoteDecisionDraft {
                vote_id: "56-1-v1".into(),
                result_id: "56-1-r1".into(),
                session_id: 56,
                meeting_id: 1,
                date: "2026-01-01".into(),
                seq: 1,
                title_nl: String::new(),
                title_fr: String::new(),
                method: "roll_call".into(),
                status: "complete".into(),
                outcome: "adopted".into(),
                dossier_id: String::new(),
                document_id: String::new(),
                motion_id: String::new(),
                source_roll_call_number: "1".into(),
                reuses_result: false,
                source_url: "https://example.test".into(),
                cache_path: "report.html".into(),
            }],
        )
        .unwrap();
        let vote_schema = schema(&votes);
        assert_eq!(
            vote_schema
                .field_with_name("session_id")
                .unwrap()
                .data_type(),
            &DataType::UInt32
        );
        assert_eq!(
            vote_schema.field_with_name("seq").unwrap().data_type(),
            &DataType::UInt32
        );
        assert_eq!(
            vote_schema
                .field_with_name("reuses_result")
                .unwrap()
                .data_type(),
            &DataType::Boolean
        );

        let results = dir.join("results.parquet");
        write_vote_results_parquet(
            &results,
            &[VoteResultDraft {
                result_id: "56-1-r1".into(),
                session_id: 56,
                meeting_id: 1,
                seq: 1,
                method: "roll_call".into(),
                named: true,
                status: "complete".into(),
                outcome: "adopted".into(),
                source_roll_call_number: "1".into(),
                source_url: "https://example.test".into(),
                cache_path: "report.html".into(),
            }],
        )
        .unwrap();
        assert_eq!(
            schema(&results)
                .field_with_name("named")
                .unwrap()
                .data_type(),
            &DataType::Boolean
        );

        let tallies = dir.join("tallies.parquet");
        write_vote_tallies_parquet(
            &tallies,
            &[VoteTallyDraft {
                result_id: "56-1-r1".into(),
                tally_kind: "position".into(),
                option_key: "yes".into(),
                label_nl: "ja".into(),
                label_fr: "oui".into(),
                dimension: "overall".into(),
                count: 10,
                selected: true,
            }],
        )
        .unwrap();
        let tally_schema = schema(&tallies);
        assert_eq!(
            tally_schema.field_with_name("count").unwrap().data_type(),
            &DataType::UInt32
        );
        assert_eq!(
            tally_schema
                .field_with_name("selected")
                .unwrap()
                .data_type(),
            &DataType::Boolean
        );

        let members = dir.join("members.parquet");
        write_vote_result_members_parquet(
            &members,
            &[VoteResultMemberDraft {
                result_id: "56-1-r1".into(),
                position: "yes".into(),
                seq: 1,
                raw_name: "Example".into(),
            }],
        )
        .unwrap();
        assert_eq!(
            schema(&members).field_with_name("seq").unwrap().data_type(),
            &DataType::UInt32
        );

        let unresolved = dir.join("unresolved.parquet");
        write_unresolved_vote_events_parquet(
            &unresolved,
            &[UnresolvedVoteEventDraft {
                session_id: 56,
                meeting_id: 1,
                event_kind: "language_group_counts".into(),
                source_roll_call_number: "3".into(),
                block_start: 4,
                block_end: 5,
                reason: "language_group_sum_mismatch:yes".into(),
                evidence_text: "Ja 40 70 20".into(),
                source_url: "https://example.test".into(),
                cache_path: "report.html".into(),
            }],
        )
        .unwrap();
        let unresolved_schema = schema(&unresolved);
        assert_eq!(
            unresolved_schema
                .field_with_name("block_start")
                .unwrap()
                .data_type(),
            &DataType::UInt32
        );
        assert_eq!(
            unresolved_schema
                .field_with_name("reason")
                .unwrap()
                .data_type(),
            &DataType::Utf8
        );

        std::fs::remove_dir_all(dir).unwrap();
    }
}
