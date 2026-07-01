use crate::common::{dedupe_unresolved, reason_label, split_csv, UnresolvedRow, SESSION_ID};
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use identity::resolver::{Bucket, Resolution, Resolver};
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct VoteCastRow {
    pub vote_cast_id: String,
    pub vote_id: String,
    pub session_id: String,
    pub meeting_id: String,
    pub person_id: String,
    pub position: String,
    pub raw_name: String,
    pub source_url: String,
    pub cache_path: String,
    pub confidence: String,
}

#[derive(Debug, Clone)]
pub struct VoteReconciliationRow {
    pub vote_id: String,
    pub session_id: String,
    pub meeting_id: String,
    pub yes: String,
    pub no: String,
    pub abstain: String,
    pub members_yes_count: String,
    pub members_no_count: String,
    pub members_abstain_count: String,
    pub reconciled: String,
    pub source_url: String,
    pub cache_path: String,
}

pub struct VoteCastOutput {
    pub casts: Vec<VoteCastRow>,
    pub unresolved: Vec<UnresolvedRow>,
    pub reconciliation: Vec<VoteReconciliationRow>,
}

pub fn normalize_vote_casts(
    data_dir: &Path,
    resolver: &Resolver,
) -> Result<VoteCastOutput, Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/plenary/votes.parquet"));
    let mut casts = Vec::new();
    let mut unresolved = Vec::new();
    let mut reconciliation = Vec::new();

    for batch in read_all_rows(&path)? {
        let vote_ids = read_string_column(&batch, "vote_id")?;
        let session_ids = read_string_column(&batch, "session_id")?;
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let yes_counts = read_string_column(&batch, "yes")?;
        let no_counts = read_string_column(&batch, "no")?;
        let abstain_counts = read_string_column(&batch, "abstain")?;
        let members_yes = read_string_column(&batch, "members_yes")?;
        let members_no = read_string_column(&batch, "members_no")?;
        let members_abstain = read_string_column(&batch, "members_abstain")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            let vote_id = vote_ids[i].clone();
            let source_url = source_urls[i].clone();
            let cache_path = cache_paths[i].clone();

            let yes_names = split_csv(&members_yes[i]);
            let no_names = split_csv(&members_no[i]);
            let abstain_names = split_csv(&members_abstain[i]);

            let positions = [
                ("yes", yes_names),
                ("no", no_names),
                ("abstain", abstain_names),
            ];

            for (position, names) in positions {
                for (seq, name) in names.iter().enumerate() {
                    let detail = resolver.resolve_detail(name, Bucket::Vote);
                    match &detail.resolution {
                        Resolution::Resolved(person_id) => {
                            casts.push(VoteCastRow {
                                vote_cast_id: format!("{}_{position}_{seq}", vote_id),
                                vote_id: vote_id.clone(),
                                session_id: session_ids[i].clone(),
                                meeting_id: meeting_ids[i].clone(),
                                person_id: person_id.clone(),
                                position: position.to_string(),
                                raw_name: name.clone(),
                                source_url: source_url.clone(),
                                cache_path: cache_path.clone(),
                                confidence: "exact".to_string(),
                            });
                        }
                        Resolution::Unresolved(reason) => {
                            unresolved.push(UnresolvedRow {
                                raw_name: detail.raw_name,
                                typo_corrected: detail.typo_corrected,
                                norm_primary: detail.norm_primary,
                                norm_reordered: detail.norm_reordered,
                                reason: reason_label(reason).to_string(),
                                source_bucket: "votes".to_string(),
                                role: position.to_string(),
                                context_id: vote_id.clone(),
                                context_label: format!("vote {}", vote_id),
                                raw_field: name.clone(),
                                source_url: source_url.clone(),
                                cache_path: cache_path.clone(),
                            });
                        }
                    }
                }
            }

            let yes_count = yes_counts[i].parse::<usize>().unwrap_or(0);
            let no_count = no_counts[i].parse::<usize>().unwrap_or(0);
            let abstain_count = abstain_counts[i].parse::<usize>().unwrap_or(0);
            let members_yes_count = split_csv(&members_yes[i]).len();
            let members_no_count = split_csv(&members_no[i]).len();
            let members_abstain_count = split_csv(&members_abstain[i]).len();
            let reconciled = yes_count == members_yes_count
                && no_count == members_no_count
                && abstain_count == members_abstain_count;

            reconciliation.push(VoteReconciliationRow {
                vote_id: vote_id.clone(),
                session_id: session_ids[i].clone(),
                meeting_id: meeting_ids[i].clone(),
                yes: yes_counts[i].clone(),
                no: no_counts[i].clone(),
                abstain: abstain_counts[i].clone(),
                members_yes_count: members_yes_count.to_string(),
                members_no_count: members_no_count.to_string(),
                members_abstain_count: members_abstain_count.to_string(),
                reconciled: reconciled.to_string(),
                source_url,
                cache_path,
            });
        }
    }

    casts.sort_by(|a, b| {
        a.vote_id
            .cmp(&b.vote_id)
            .then(a.position.cmp(&b.position))
            .then(a.person_id.cmp(&b.person_id))
    });
    dedupe_unresolved(&mut unresolved);

    Ok(VoteCastOutput {
        casts,
        unresolved,
        reconciliation,
    })
}

pub fn write_vote_casts(path: &Path, rows: &[VoteCastRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("vote_cast_id", false),
        utf8_field("vote_id", false),
        utf8_field("session_id", false),
        utf8_field("meeting_id", false),
        utf8_field("person_id", false),
        utf8_field("position", false),
        utf8_field("raw_name", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
        utf8_field("confidence", false),
    ]);

    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }

    write_parquet(
        path,
        schema,
        vec![
            col!(|r| r.vote_cast_id.clone()),
            col!(|r| r.vote_id.clone()),
            col!(|r| r.session_id.clone()),
            col!(|r| r.meeting_id.clone()),
            col!(|r| r.person_id.clone()),
            col!(|r| r.position.clone()),
            col!(|r| r.raw_name.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.confidence.clone()),
        ],
    )
}

pub fn write_vote_reconciliation(
    path: &Path,
    rows: &[VoteReconciliationRow],
) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("vote_id", false),
        utf8_field("session_id", false),
        utf8_field("meeting_id", false),
        utf8_field("yes", false),
        utf8_field("no", false),
        utf8_field("abstain", false),
        utf8_field("members_yes_count", false),
        utf8_field("members_no_count", false),
        utf8_field("members_abstain_count", false),
        utf8_field("reconciled", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
    ]);

    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }

    write_parquet(
        path,
        schema,
        vec![
            col!(|r| r.vote_id.clone()),
            col!(|r| r.session_id.clone()),
            col!(|r| r.meeting_id.clone()),
            col!(|r| r.yes.clone()),
            col!(|r| r.no.clone()),
            col!(|r| r.abstain.clone()),
            col!(|r| r.members_yes_count.clone()),
            col!(|r| r.members_no_count.clone()),
            col!(|r| r.members_abstain_count.clone()),
            col!(|r| r.reconciled.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
        ],
    )
}
