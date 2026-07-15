use crate::common::{SESSION_ID, UnresolvedRow, dedupe_unresolved, reason_label};
use crate::provenance::{
    CONFIDENCE_EXACT, ContentHashCache, provenance_columns, provenance_fields, provenance_of,
};
use arrow::array::{ArrayRef, StringArray, UInt32Array};
use arrow::datatypes::{DataType, Field, Schema};
use identity::parquet_io::{
    read_all_rows, read_bool_column, read_string_column, read_u32_column, utf8_field, write_parquet,
};
use identity::resolver::{Bucket, Resolution, Resolver};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct VoteCastRow {
    pub vote_cast_id: String,
    pub result_id: String,
    pub session_id: u32,
    pub meeting_id: u32,
    pub person_id: String,
    pub position: String,
    pub raw_name: String,
    pub source_url: String,
    pub cache_path: String,
    pub source_artifact_id: String,
    pub source_content_hash: String,
    pub block_parser_version: String,
    pub extractor_version: String,
    pub confidence: f64,
}

/// Derived aggregate comparing tallies to named member counts.
/// Outside the source-derived provenance contract — keep url/cache only (no artifact/hash/versions).
#[derive(Debug, Clone)]
pub struct VoteReconciliationRow {
    pub result_id: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResultMetadata {
    session_id: u32,
    meeting_id: u32,
    method: String,
    named: bool,
    source_url: String,
    cache_path: String,
}

impl ResultMetadata {
    fn allows_named_casts(&self) -> bool {
        self.named
            && matches!(
                self.method.as_str(),
                "roll_call" | "language_group_roll_call"
            )
    }
}

pub fn normalize_vote_casts(
    data_dir: &Path,
    resolver: &Resolver,
) -> Result<VoteCastOutput, Box<dyn Error>> {
    let session = data_dir.join(format!("sessions/{SESSION_ID}/plenary"));
    let members_path = session.join("vote_result_members.parquet");
    let results_path = session.join("vote_results.parquet");
    let tallies_path = session.join("vote_tallies.parquet");

    let mut casts = Vec::new();
    let mut unresolved = Vec::new();
    let mut reconciliation = Vec::new();
    let mut tally_map: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let mut result_rows: HashMap<String, ResultMetadata> = HashMap::new();

    if results_path.exists() {
        for batch in read_all_rows(&results_path)? {
            let result_ids = read_string_column(&batch, "result_id")?;
            let session_ids = read_u32_column(&batch, "session_id")?;
            let meeting_ids = read_u32_column(&batch, "meeting_id")?;
            let methods = read_string_column(&batch, "method")?;
            let named = read_bool_column(&batch, "named")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                let metadata = ResultMetadata {
                    session_id: session_ids[i],
                    meeting_id: meeting_ids[i],
                    method: methods[i].clone(),
                    named: named[i],
                    source_url: source_urls[i].clone(),
                    cache_path: cache_paths[i].clone(),
                };
                if let Some(existing) = result_rows.insert(result_ids[i].clone(), metadata.clone())
                    && existing != metadata
                {
                    return Err(format!(
                        "conflicting duplicate vote result metadata for {}",
                        result_ids[i]
                    )
                    .into());
                }
            }
        }
    }

    if tallies_path.exists() {
        for batch in read_all_rows(&tallies_path)? {
            let result_ids = read_string_column(&batch, "result_id")?;
            let option_keys = read_string_column(&batch, "option_key")?;
            let dimensions = read_string_column(&batch, "dimension")?;
            let counts = read_u32_column(&batch, "count")?;
            for i in 0..batch.num_rows() {
                if dimensions[i] != "overall" {
                    continue;
                }
                let count = counts[i] as usize;
                tally_map
                    .entry(result_ids[i].clone())
                    .or_default()
                    .insert(option_keys[i].clone(), count);
            }
        }
    }

    if members_path.exists() {
        let mut hashes = ContentHashCache::new();
        let mut seen_members: HashSet<(String, String, u32, String)> = HashSet::new();
        let mut member_counts: HashMap<String, HashMap<String, usize>> = HashMap::new();

        for batch in read_all_rows(&members_path)? {
            let result_ids = read_string_column(&batch, "result_id")?;
            let positions = read_string_column(&batch, "position")?;
            let seqs = read_u32_column(&batch, "seq")?;
            let raw_names = read_string_column(&batch, "raw_name")?;

            for i in 0..batch.num_rows() {
                let result_id = result_ids[i].clone();
                let position = positions[i].clone();
                let name = raw_names[i].clone();
                let seq = seqs[i];
                if !seen_members.insert((result_id.clone(), position.clone(), seq, name.clone())) {
                    continue;
                }
                let Some(result) = result_rows.get(&result_id) else {
                    return Err(format!("vote member references unknown result {result_id}").into());
                };
                if !result.allows_named_casts() {
                    continue;
                }
                *member_counts
                    .entry(result_id.clone())
                    .or_default()
                    .entry(position.clone())
                    .or_default() += 1;
                let detail = resolver.resolve_detail(&name, Bucket::Vote);
                let prov =
                    hashes.vote_report(&result.source_url, &result.cache_path, CONFIDENCE_EXACT);
                match &detail.resolution {
                    Resolution::Resolved(person_id) => {
                        casts.push(VoteCastRow {
                            vote_cast_id: format!("{result_id}_{position}_{seq}"),
                            result_id: result_id.clone(),
                            session_id: result.session_id,
                            meeting_id: result.meeting_id,
                            person_id: person_id.clone(),
                            position: position.clone(),
                            raw_name: name.clone(),
                            source_url: prov.source_url.clone(),
                            cache_path: prov.cache_path.clone(),
                            source_artifact_id: prov.source_artifact_id.clone(),
                            source_content_hash: prov.source_content_hash.clone(),
                            block_parser_version: prov.block_parser_version.clone(),
                            extractor_version: prov.extractor_version.clone(),
                            confidence: prov.confidence,
                        });
                    }
                    Resolution::Unresolved(reason) => {
                        unresolved.push(
                            UnresolvedRow {
                                raw_name: detail.raw_name,
                                typo_corrected: detail.typo_corrected,
                                norm_primary: detail.norm_primary,
                                norm_reordered: detail.norm_reordered,
                                reason: reason_label(reason).to_string(),
                                source_bucket: "vote_result_members".to_string(),
                                role: position.clone(),
                                context_id: result_id.clone(),
                                context_label: format!("vote result {result_id}"),
                                raw_field: name.clone(),
                                ..UnresolvedRow::default()
                            }
                            .with_provenance(prov),
                        );
                    }
                }
            }
        }

        let mut result_ids = result_rows.keys().cloned().collect::<Vec<_>>();
        result_ids.sort();
        for result_id in result_ids {
            let result = &result_rows[&result_id];
            if result.allows_named_casts() {
                let tallies = tally_map.get(&result_id);
                let yes_count = tallies.and_then(|t| t.get("yes")).copied().unwrap_or(0);
                let no_count = tallies.and_then(|t| t.get("no")).copied().unwrap_or(0);
                let abstain_count = tallies.and_then(|t| t.get("abstain")).copied().unwrap_or(0);
                let counts = member_counts.get(&result_id);
                let members_yes_count = counts.and_then(|c| c.get("yes")).copied().unwrap_or(0);
                let members_no_count = counts.and_then(|c| c.get("no")).copied().unwrap_or(0);
                let members_abstain_count =
                    counts.and_then(|c| c.get("abstain")).copied().unwrap_or(0);

                let reconciled = yes_count == members_yes_count
                    && no_count == members_no_count
                    && abstain_count == members_abstain_count;

                reconciliation.push(VoteReconciliationRow {
                    result_id: result_id.clone(),
                    session_id: result.session_id.to_string(),
                    meeting_id: result.meeting_id.to_string(),
                    yes: yes_count.to_string(),
                    no: no_count.to_string(),
                    abstain: abstain_count.to_string(),
                    members_yes_count: members_yes_count.to_string(),
                    members_no_count: members_no_count.to_string(),
                    members_abstain_count: members_abstain_count.to_string(),
                    reconciled: reconciled.to_string(),
                    source_url: result.source_url.clone(),
                    cache_path: result.cache_path.clone(),
                });
            }
        }
    }

    casts.sort_by(|a, b| {
        a.result_id
            .cmp(&b.result_id)
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
    let mut fields = vec![
        utf8_field("vote_cast_id", false),
        utf8_field("result_id", false),
        Field::new("session_id", DataType::UInt32, false),
        Field::new("meeting_id", DataType::UInt32, false),
        utf8_field("person_id", false),
        utf8_field("position", false),
        utf8_field("raw_name", false),
    ];
    fields.extend(provenance_fields());
    let schema = Schema::new(fields);

    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }

    let mut columns = vec![
        col!(|r| r.vote_cast_id.clone()),
        col!(|r| r.result_id.clone()),
        Arc::new(UInt32Array::from(
            rows.iter().map(|r| r.session_id).collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(UInt32Array::from(
            rows.iter().map(|r| r.meeting_id).collect::<Vec<_>>(),
        )) as ArrayRef,
        col!(|r| r.person_id.clone()),
        col!(|r| r.position.clone()),
        col!(|r| r.raw_name.clone()),
    ];
    columns.extend(provenance_columns(rows.iter().map(|r| {
        provenance_of(
            &r.source_url,
            &r.cache_path,
            &r.source_artifact_id,
            &r.source_content_hash,
            &r.block_parser_version,
            &r.extractor_version,
            r.confidence,
        )
    })));
    write_parquet(path, schema, columns)
}

pub fn write_vote_reconciliation(
    path: &Path,
    rows: &[VoteReconciliationRow],
) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("result_id", false),
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
            col!(|r| r.result_id.clone()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crawl::vote_io::{
        write_vote_result_members_parquet, write_vote_results_parquet, write_vote_tallies_parquet,
    };
    use crawl::vote_types::{VoteResultDraft, VoteResultMemberDraft, VoteTallyDraft};
    use identity::resolver::PersonRecord;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("normalize_vote_casts_{name}_{nonce}"))
    }

    fn result(result_id: &str, method: &str, named: bool) -> VoteResultDraft {
        VoteResultDraft {
            result_id: result_id.into(),
            session_id: 56,
            meeting_id: 1,
            seq: 1,
            method: method.into(),
            named,
            status: "complete".into(),
            outcome: "adopted".into(),
            source_roll_call_number: "1".into(),
            source_url: "https://example.test/report".into(),
            cache_path: String::new(),
        }
    }

    fn member(result_id: &str, position: &str, seq: u32, name: &str) -> VoteResultMemberDraft {
        VoteResultMemberDraft {
            result_id: result_id.into(),
            position: position.into(),
            seq,
            raw_name: name.into(),
        }
    }

    fn resolver() -> Resolver {
        Resolver::build(
            &[PersonRecord {
                person_id: "p1".into(),
                first_name: "Known".into(),
                last_name: "Member".into(),
            }],
            &[],
        )
    }

    fn write_inputs(
        root: &Path,
        results: &[VoteResultDraft],
        members: &[VoteResultMemberDraft],
        tallies: &[VoteTallyDraft],
    ) {
        let session = root.join("sessions/56/plenary");
        std::fs::create_dir_all(&session).unwrap();
        write_vote_results_parquet(&session.join("vote_results.parquet"), results).unwrap();
        write_vote_result_members_parquet(&session.join("vote_result_members.parquet"), members)
            .unwrap();
        write_vote_tallies_parquet(&session.join("vote_tallies.parquet"), tallies).unwrap();
    }

    #[test]
    fn deduplicates_members_per_result_and_reconciles_raw_positions() {
        let root = test_dir("dedup");
        let rows = [
            member("56-1-r1", "yes", 0, "Known Member"),
            member("56-1-r1", "yes", 0, "Known Member"),
            member("56-1-r1", "no", 0, "Unknown Member"),
            member("56-1-r1", "no", 0, "Unknown Member"),
        ];
        let tallies = [
            VoteTallyDraft {
                result_id: "56-1-r1".into(),
                tally_kind: "position".into(),
                option_key: "yes".into(),
                label_nl: "ja".into(),
                label_fr: "oui".into(),
                dimension: "overall".into(),
                count: 1,
                selected: false,
            },
            VoteTallyDraft {
                result_id: "56-1-r1".into(),
                tally_kind: "position".into(),
                option_key: "no".into(),
                label_nl: "nee".into(),
                label_fr: "non".into(),
                dimension: "overall".into(),
                count: 1,
                selected: false,
            },
        ];
        write_inputs(
            &root,
            &[result("56-1-r1", "roll_call", true)],
            &rows,
            &tallies,
        );

        let output = normalize_vote_casts(&root, &resolver()).unwrap();
        assert_eq!(output.casts.len(), 1);
        assert_eq!(output.unresolved.len(), 1);
        assert_eq!(output.reconciliation.len(), 1);
        assert_eq!(output.reconciliation[0].members_yes_count, "1");
        assert_eq!(output.reconciliation[0].members_no_count, "1");
        assert_eq!(output.reconciliation[0].reconciled, "true");
        assert_eq!(output.casts[0].confidence, 1.0);
        assert_eq!(
            output.casts[0].source_artifact_id,
            output.unresolved[0].source_artifact_id
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_named_roll_call_methods_emit_casts() {
        let root = test_dir("methods");
        let results = [
            result("56-1-r1", "roll_call", true),
            result("56-1-r2", "language_group_roll_call", true),
            result("56-1-r3", "secret_ballot", false),
            result("56-1-r4", "sitting_standing", false),
            result("56-1-r5", "no_quorum", false),
        ];
        let members = results
            .iter()
            .map(|row| member(&row.result_id, "yes", 0, "Known Member"))
            .collect::<Vec<_>>();
        write_inputs(&root, &results, &members, &[]);

        let output = normalize_vote_casts(&root, &resolver()).unwrap();
        assert_eq!(
            output
                .casts
                .iter()
                .map(|cast| cast.result_id.as_str())
                .collect::<Vec<_>>(),
            vec!["56-1-r1", "56-1-r2"]
        );
        assert!(
            output
                .casts
                .iter()
                .all(|cast| !matches!(cast.result_id.as_str(), "56-1-r3" | "56-1-r4" | "56-1-r5"))
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
