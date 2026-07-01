use arrow::array::{ArrayRef, StringArray};
use crawl::paths::{cache_dir, data_dir};
use identity::normalize::{normalize_name, typo_corrections, PersonName};
use identity::parquet_io::{
    read_all_rows, read_optional_string_column, read_string_column, utf8_field, write_parquet,
};
use identity::resolver::{
    AliasRecord, Bucket, PersonRecord, Resolution, Resolver, UnresolvedReason,
};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
struct MemberRow {
    person_id: String,
    first_name: String,
    last_name: String,
    date_of_birth: String,
    place_of_birth: String,
    language: String,
    fraction: String,
    active: String,
    start: Option<String>,
    source_url: String,
    cache_path: String,
}

#[derive(Debug, Clone)]
struct CommissionRow {
    name: String,
    ctype: String,
    chairs: String,
    subchairs: String,
    permanent_members: String,
    replacement_members: String,
    source_url: String,
    cache_path: String,
}

#[derive(Debug, Clone)]
struct PartyMembership {
    person_id: String,
    org_id: String,
    role: String,
    start_date: Option<String>,
    active: String,
    source_url: String,
}

#[derive(Debug, Clone)]
struct CommissionMembership {
    person_id: String,
    org_id: String,
    role: String,
    source_url: String,
}

#[derive(Debug, Clone)]
struct UnresolvedCase {
    raw_name: String,
    typo_corrected: String,
    norm_primary: String,
    norm_reordered: String,
    reason: String,
    source_bucket: String,
    role: String,
    commission_id: String,
    commission_name: String,
    raw_field: String,
    source_url: String,
    cache_path: String,
    cache_file: String,
}

#[derive(Debug, Clone)]
struct SkippedMember {
    first_name: String,
    last_name: String,
    fraction: String,
    active: String,
    source_url: String,
    cache_path: String,
    cache_file: String,
    reason: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let root = data_dir();
    let out_dir = root.join("identity");
    let members_path = root.join("sessions/56/members.parquet");
    let commissions_path = root.join("commissions.parquet");

    let members = load_members(&members_path)?;
    let skipped_members = find_skipped_members(&members);
    let persons = build_persons(&members);
    let parties = build_parties(&members);
    let party_memberships = build_party_memberships(&members);
    let aliases = build_aliases(&persons);
    let resolver = Resolver::build(&persons, &aliases);

    let commission_rows = load_commissions(&commissions_path)?;
    let canonical_commissions = build_canonical_commissions(&commission_rows);
    let (commission_memberships, unresolved) =
        build_commission_memberships(&commission_rows, &resolver);

    write_persons(&out_dir.join("persons.parquet"), &persons)?;
    write_parties(&out_dir.join("parties.parquet"), &parties)?;
    write_aliases(&out_dir.join("person_aliases.parquet"), &aliases)?;
    write_commissions(&out_dir.join("commissions.parquet"), &canonical_commissions)?;
    write_memberships(
        &out_dir.join("memberships.parquet"),
        &party_memberships,
        &commission_memberships,
    )?;
    write_unresolved_persons(&out_dir.join("unresolved_persons.parquet"), &unresolved)?;
    write_unresolved_report(
        &out_dir.join("unresolved_report.md"),
        &unresolved,
        &skipped_members,
    )?;

    print_summary(
        &persons,
        &parties,
        &party_memberships,
        &commission_memberships,
        &unresolved,
        &skipped_members,
        &out_dir,
    );

    Ok(())
}

fn load_members(path: &Path) -> Result<Vec<MemberRow>, Box<dyn Error>> {
    let mut rows = Vec::new();
    for batch in read_all_rows(path)? {
        let person_ids = read_string_column(&batch, "member_id")?;
        let first_names = read_string_column(&batch, "first_name")?;
        let last_names = read_string_column(&batch, "last_name")?;
        let dobs = read_string_column(&batch, "date_of_birth")?;
        let pobs = read_string_column(&batch, "place_of_birth")?;
        let languages = read_string_column(&batch, "language")?;
        let fractions = read_string_column(&batch, "fraction")?;
        let actives = read_string_column(&batch, "active")?;
        let starts = read_optional_string_column(&batch, "start")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            rows.push(MemberRow {
                person_id: person_ids[i].clone(),
                first_name: first_names[i].clone(),
                last_name: last_names[i].clone(),
                date_of_birth: dobs[i].clone(),
                place_of_birth: pobs[i].clone(),
                language: languages[i].clone(),
                fraction: fractions[i].clone(),
                active: actives[i].clone(),
                start: starts[i].clone(),
                source_url: source_urls[i].clone(),
                cache_path: cache_paths[i].clone(),
            });
        }
    }
    Ok(rows)
}

fn cache_file_path(relative: &str) -> String {
    if relative.is_empty() {
        return String::new();
    }
    cache_dir().join(relative).display().to_string()
}

fn reason_label(reason: &UnresolvedReason) -> &'static str {
    match reason {
        UnresolvedReason::Empty => "empty",
        UnresolvedReason::NotInIndex => "not_in_index",
        UnresolvedReason::Ambiguous => "ambiguous",
    }
}

fn find_skipped_members(members: &[MemberRow]) -> Vec<SkippedMember> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut out = Vec::new();

    for member in members {
        if !member.person_id.is_empty() {
            continue;
        }
        let key = (member.first_name.clone(), member.last_name.clone());
        if !seen.insert(key) {
            continue;
        }
        out.push(SkippedMember {
            first_name: member.first_name.clone(),
            last_name: member.last_name.clone(),
            fraction: member.fraction.clone(),
            active: member.active.clone(),
            source_url: member.source_url.clone(),
            cache_path: member.cache_path.clone(),
            cache_file: cache_file_path(&member.cache_path),
            reason: "empty_person_id".to_string(),
        });
    }

    out.sort_by(|a, b| {
        a.last_name
            .cmp(&b.last_name)
            .then(a.first_name.cmp(&b.first_name))
    });
    out
}

fn build_persons(members: &[MemberRow]) -> Vec<PersonRecord> {
    let mut by_id: HashMap<String, MemberRow> = HashMap::new();

    for member in members {
        if member.person_id.is_empty() {
            continue;
        }
        match by_id.get(&member.person_id) {
            None => {
                by_id.insert(member.person_id.clone(), member.clone());
            }
            Some(existing) => {
                let prefer_new = member.active == "true" && existing.active != "true";
                if prefer_new {
                    by_id.insert(member.person_id.clone(), member.clone());
                }
            }
        }
    }

    let mut persons: Vec<PersonRecord> = by_id
        .into_values()
        .map(|m| PersonRecord {
            person_id: m.person_id,
            first_name: m.first_name,
            last_name: m.last_name,
        })
        .collect();
    persons.sort_by(|a, b| a.person_id.cmp(&b.person_id));
    persons
}

fn build_parties(members: &[MemberRow]) -> Vec<(String, String)> {
    let mut slugs: HashSet<String> = HashSet::new();
    for member in members {
        let slug = member.fraction.trim().to_lowercase();
        if !slug.is_empty() {
            slugs.insert(slug);
        }
    }
    let mut parties: Vec<(String, String)> = slugs
        .into_iter()
        .map(|slug| (slug.clone(), "members".to_string()))
        .collect();
    parties.sort_by(|a, b| a.0.cmp(&b.0));
    parties
}

fn build_party_memberships(members: &[MemberRow]) -> Vec<PartyMembership> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut out = Vec::new();

    for member in members {
        if member.person_id.is_empty() || member.fraction.trim().is_empty() {
            continue;
        }
        let key = (member.person_id.clone(), member.fraction.clone());
        if !seen.insert(key) {
            continue;
        }
        out.push(PartyMembership {
            person_id: member.person_id.clone(),
            org_id: member.fraction.trim().to_lowercase(),
            role: "member".to_string(),
            start_date: member.start.clone(),
            active: member.active.clone(),
            source_url: member.source_url.clone(),
        });
    }

    out.sort_by(|a, b| {
        a.person_id
            .cmp(&b.person_id)
            .then(a.org_id.cmp(&b.org_id))
    });
    out
}

fn build_aliases(persons: &[PersonRecord]) -> Vec<AliasRecord> {
    let mut aliases: Vec<AliasRecord> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    let mut push_alias = |alias_text: &str, person_id: &str, source: &str| {
        let norm = normalize_name(alias_text);
        if norm.is_empty() {
            return;
        }
        if seen.insert((norm.clone(), person_id.to_string())) {
            aliases.push(AliasRecord {
                alias_norm: norm,
                person_id: person_id.to_string(),
                source: source.to_string(),
            });
        }
    };

    for person in persons {
        let name = PersonName {
            first_name: person.first_name.clone(),
            last_name: person.last_name.clone(),
        };
        push_alias(&name.full(), &person.person_id, "self");
        push_alias(&name.reversed(), &person.person_id, "reorder");
    }

    let typo_map = typo_corrections();
    for person in persons {
        let full = PersonName {
            first_name: person.first_name.clone(),
            last_name: person.last_name.clone(),
        }
        .full();
        for (wrong, correct) in &typo_map {
            if correct.eq_ignore_ascii_case(&full) {
                push_alias(wrong, &person.person_id, "typo_map");
            }
        }
    }

    aliases.sort_by(|a, b| a.alias_norm.cmp(&b.alias_norm).then(a.person_id.cmp(&b.person_id)));
    aliases
}

fn load_commissions(path: &Path) -> Result<Vec<CommissionRow>, Box<dyn Error>> {
    let mut rows = Vec::new();
    for batch in read_all_rows(path)? {
        let names = read_string_column(&batch, "name")?;
        let ctypes = read_string_column(&batch, "type")?;
        let chairs = read_string_column(&batch, "chairs")?;
        let subchairs = read_string_column(&batch, "subchairs")?;
        let permanent = read_string_column(&batch, "permanent_members")?;
        let replacement = read_string_column(&batch, "replacement_members")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            rows.push(CommissionRow {
                name: names[i].clone(),
                ctype: ctypes[i].clone(),
                chairs: chairs[i].clone(),
                subchairs: subchairs[i].clone(),
                permanent_members: permanent[i].clone(),
                replacement_members: replacement[i].clone(),
                source_url: source_urls[i].clone(),
                cache_path: cache_paths[i].clone(),
            });
        }
    }
    Ok(rows)
}

fn commission_slug(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .replace(' ', "_")
        .replace('/', "_")
}

fn build_canonical_commissions(rows: &[CommissionRow]) -> Vec<(String, CommissionRow)> {
    rows.iter()
        .map(|row| (commission_slug(&row.name), row.clone()))
        .collect()
}

fn split_csv_names(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn build_commission_memberships(
    rows: &[CommissionRow],
    resolver: &Resolver,
) -> (Vec<CommissionMembership>, Vec<UnresolvedCase>) {
    let mut memberships = Vec::new();
    let mut unresolved = Vec::new();
    let mut seen: HashSet<(String, String, String)> = HashSet::new();

    let role_fields: [(&str, fn(&CommissionRow) -> &str); 4] = [
        ("chair", |r| r.chairs.as_str()),
        ("subchair", |r| r.subchairs.as_str()),
        ("permanent", |r| r.permanent_members.as_str()),
        ("replacement", |r| r.replacement_members.as_str()),
    ];

    for row in rows {
        let org_id = commission_slug(&row.name);
        let cache_file = cache_file_path(&row.cache_path);
        for (role, getter) in role_fields {
            let raw_field = getter(row).to_string();
            for name in split_csv_names(&raw_field) {
                let detail = resolver.resolve_detail(&name, Bucket::CommissionMember);
                match detail.resolution {
                    Resolution::Resolved(person_id) => {
                        let key = (person_id.clone(), org_id.clone(), role.to_string());
                        if seen.insert(key) {
                            memberships.push(CommissionMembership {
                                person_id,
                                org_id: org_id.clone(),
                                role: role.to_string(),
                                source_url: row.source_url.clone(),
                            });
                        }
                    }
                    Resolution::Unresolved(reason) => {
                        unresolved.push(UnresolvedCase {
                            raw_name: detail.raw_name,
                            typo_corrected: detail.typo_corrected,
                            norm_primary: detail.norm_primary,
                            norm_reordered: detail.norm_reordered,
                            reason: reason_label(&reason).to_string(),
                            source_bucket: "commission_members".to_string(),
                            role: role.to_string(),
                            commission_id: org_id.clone(),
                            commission_name: row.name.clone(),
                            raw_field: raw_field.clone(),
                            source_url: row.source_url.clone(),
                            cache_path: row.cache_path.clone(),
                            cache_file: cache_file.clone(),
                        });
                    }
                }
            }
        }
    }

    memberships.sort_by(|a, b| {
        a.org_id
            .cmp(&b.org_id)
            .then(a.role.cmp(&b.role))
            .then(a.person_id.cmp(&b.person_id))
    });
    unresolved.sort_by(|a, b| {
        a.raw_name
            .cmp(&b.raw_name)
            .then(a.commission_id.cmp(&b.commission_id))
            .then(a.role.cmp(&b.role))
    });
    (memberships, unresolved)
}

fn write_persons(path: &Path, persons: &[PersonRecord]) -> Result<(), Box<dyn Error>> {
    let members_path = data_dir().join("sessions/56/members.parquet");
    let mut meta: HashMap<String, MemberRow> = HashMap::new();
    for member in load_members(&members_path)? {
        if member.person_id.is_empty() {
            continue;
        }
        meta.entry(member.person_id.clone())
            .or_insert(member.clone());
    }

    let schema = arrow::datatypes::Schema::new(vec![
        utf8_field("person_id", false),
        utf8_field("first_name", false),
        utf8_field("last_name", false),
        utf8_field("date_of_birth", false),
        utf8_field("place_of_birth", false),
        utf8_field("language", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
    ]);

    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(persons.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }

    let columns = vec![
        col!(|p| p.person_id.clone()),
        col!(|p| p.first_name.clone()),
        col!(|p| p.last_name.clone()),
        col!(|p| meta
            .get(&p.person_id)
            .map(|m| m.date_of_birth.clone())
            .unwrap_or_default()),
        col!(|p| meta
            .get(&p.person_id)
            .map(|m| m.place_of_birth.clone())
            .unwrap_or_default()),
        col!(|p| meta
            .get(&p.person_id)
            .map(|m| m.language.clone())
            .unwrap_or_default()),
        col!(|p| meta
            .get(&p.person_id)
            .map(|m| m.source_url.clone())
            .unwrap_or_default()),
        col!(|p| meta
            .get(&p.person_id)
            .map(|m| m.cache_path.clone())
            .unwrap_or_default()),
    ];

    write_parquet(path, schema, columns)
}

fn write_parties(path: &Path, parties: &[(String, String)]) -> Result<(), Box<dyn Error>> {
    let schema = arrow::datatypes::Schema::new(vec![
        utf8_field("party_slug", false),
        utf8_field("source", false),
    ]);

    let columns = vec![
        Arc::new(StringArray::from(
            parties.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(StringArray::from(
            parties
                .iter()
                .map(|(_, src)| src.clone())
                .collect::<Vec<_>>(),
        )) as ArrayRef,
    ];

    write_parquet(path, schema, columns)
}

fn write_aliases(path: &Path, aliases: &[AliasRecord]) -> Result<(), Box<dyn Error>> {
    let schema = arrow::datatypes::Schema::new(vec![
        utf8_field("alias_norm", false),
        utf8_field("person_id", false),
        utf8_field("source", false),
        utf8_field("confidence", false),
    ]);

    let sources: Vec<String> = aliases.iter().map(|a| a.source.clone()).collect();
    let confidence: Vec<String> = aliases.iter().map(|_| "exact".to_string()).collect();

    let columns = vec![
        Arc::new(StringArray::from(
            aliases.iter().map(|a| a.alias_norm.clone()).collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(StringArray::from(
            aliases.iter().map(|a| a.person_id.clone()).collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(StringArray::from(sources)) as ArrayRef,
        Arc::new(StringArray::from(confidence)) as ArrayRef,
    ];

    write_parquet(path, schema, columns)
}

fn write_commissions(
    path: &Path,
    rows: &[(String, CommissionRow)],
) -> Result<(), Box<dyn Error>> {
    let schema = arrow::datatypes::Schema::new(vec![
        utf8_field("commission_id", false),
        utf8_field("name", false),
        utf8_field("type", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
    ]);

    let columns = vec![
        Arc::new(StringArray::from(
            rows.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(StringArray::from(
            rows.iter().map(|(_, r)| r.name.clone()).collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(StringArray::from(
            rows.iter().map(|(_, r)| r.ctype.clone()).collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(StringArray::from(
            rows.iter()
                .map(|(_, r)| r.source_url.clone())
                .collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(StringArray::from(
            rows.iter()
                .map(|(_, r)| r.cache_path.clone())
                .collect::<Vec<_>>(),
        )) as ArrayRef,
    ];

    write_parquet(path, schema, columns)
}

fn write_memberships(
    path: &Path,
    party: &[PartyMembership],
    commission: &[CommissionMembership],
) -> Result<(), Box<dyn Error>> {
    let schema = arrow::datatypes::Schema::new(vec![
        utf8_field("person_id", false),
        utf8_field("org_type", false),
        utf8_field("org_id", false),
        utf8_field("role", false),
        utf8_field("start_date", true),
        utf8_field("end_date", true),
        utf8_field("active", false),
        utf8_field("source", false),
        utf8_field("source_url", false),
        utf8_field("confidence", false),
    ]);

    let total = party.len() + commission.len();
    let mut person_ids = Vec::with_capacity(total);
    let mut org_types = Vec::with_capacity(total);
    let mut org_ids = Vec::with_capacity(total);
    let mut roles = Vec::with_capacity(total);
    let mut start_dates: Vec<Option<String>> = Vec::with_capacity(total);
    let mut end_dates: Vec<Option<String>> = Vec::with_capacity(total);
    let mut actives = Vec::with_capacity(total);
    let mut sources = Vec::with_capacity(total);
    let mut source_urls = Vec::with_capacity(total);
    let mut confidences = Vec::with_capacity(total);

    for m in party {
        person_ids.push(m.person_id.clone());
        org_types.push("party".to_string());
        org_ids.push(m.org_id.clone());
        roles.push(m.role.clone());
        start_dates.push(m.start_date.clone());
        end_dates.push(None);
        actives.push(m.active.clone());
        sources.push("members".to_string());
        source_urls.push(m.source_url.clone());
        confidences.push("exact".to_string());
    }

    for m in commission {
        person_ids.push(m.person_id.clone());
        org_types.push("commission".to_string());
        org_ids.push(m.org_id.clone());
        roles.push(m.role.clone());
        start_dates.push(None);
        end_dates.push(None);
        actives.push("true".to_string());
        sources.push("commissions".to_string());
        source_urls.push(m.source_url.clone());
        confidences.push("exact".to_string());
    }

    let columns = vec![
        Arc::new(StringArray::from(person_ids)) as ArrayRef,
        Arc::new(StringArray::from(org_types)) as ArrayRef,
        Arc::new(StringArray::from(org_ids)) as ArrayRef,
        Arc::new(StringArray::from(roles)) as ArrayRef,
        Arc::new(StringArray::from(start_dates)) as ArrayRef,
        Arc::new(StringArray::from(end_dates)) as ArrayRef,
        Arc::new(StringArray::from(actives)) as ArrayRef,
        Arc::new(StringArray::from(sources)) as ArrayRef,
        Arc::new(StringArray::from(source_urls)) as ArrayRef,
        Arc::new(StringArray::from(confidences)) as ArrayRef,
    ];

    write_parquet(path, schema, columns)
}

fn write_unresolved_persons(
    path: &Path,
    cases: &[UnresolvedCase],
) -> Result<(), Box<dyn Error>> {
    let schema = arrow::datatypes::Schema::new(vec![
        utf8_field("raw_name", false),
        utf8_field("typo_corrected", false),
        utf8_field("norm_primary", false),
        utf8_field("norm_reordered", false),
        utf8_field("reason", false),
        utf8_field("source_bucket", false),
        utf8_field("role", false),
        utf8_field("commission_id", false),
        utf8_field("commission_name", false),
        utf8_field("raw_field", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
        utf8_field("cache_file", false),
    ]);

    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(cases.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }

    let columns = vec![
        col!(|c| c.raw_name.clone()),
        col!(|c| c.typo_corrected.clone()),
        col!(|c| c.norm_primary.clone()),
        col!(|c| c.norm_reordered.clone()),
        col!(|c| c.reason.clone()),
        col!(|c| c.source_bucket.clone()),
        col!(|c| c.role.clone()),
        col!(|c| c.commission_id.clone()),
        col!(|c| c.commission_name.clone()),
        col!(|c| c.raw_field.clone()),
        col!(|c| c.source_url.clone()),
        col!(|c| c.cache_path.clone()),
        col!(|c| c.cache_file.clone()),
    ];

    write_parquet(path, schema, columns)
}

fn write_unresolved_report(
    path: &Path,
    cases: &[UnresolvedCase],
    skipped: &[SkippedMember],
) -> Result<(), Box<dyn Error>> {
    let mut out = String::new();
    out.push_str("# Identity unresolved report\n\n");
    out.push_str(&format!(
        "Cache root: `{}`\n\n",
        cache_dir().display()
    ));

    if !skipped.is_empty() {
        out.push_str("## Members skipped (empty person_id)\n\n");
        for s in skipped {
            out.push_str(&format!(
                "- **{} {}** ({}, active={}) — {}\n  - source: {}\n  - cache: `{}`\n  - html: `{}`\n\n",
                s.first_name,
                s.last_name,
                s.fraction,
                s.active,
                s.reason,
                s.source_url,
                s.cache_path,
                s.cache_file,
            ));
        }
    }

    if cases.is_empty() {
        out.push_str("## Commission member names\n\nAll names resolved.\n");
    } else {
        out.push_str(&format!(
            "## Commission member names ({} unresolved occurrences)\n\n",
            cases.len()
        ));

        let mut by_name: HashMap<String, Vec<&UnresolvedCase>> = HashMap::new();
        for case in cases {
            by_name.entry(case.raw_name.clone()).or_default().push(case);
        }

        let mut names: Vec<_> = by_name.keys().cloned().collect();
        names.sort();

        for name in names {
            let group = &by_name[&name];
            let first = group[0];
            out.push_str(&format!("### `{}`\n\n", name));
            out.push_str(&format!(
                "- reason: `{}`\n- normalized: `{}` / `{}`\n- typo corrected: `{}`\n- occurrences: {}\n\n",
                first.reason,
                first.norm_primary,
                first.norm_reordered,
                first.typo_corrected,
                group.len(),
            ));

            out.push_str("Occurrences:\n\n");
            for case in group {
                out.push_str(&format!(
                    "- role `{}` in commission `{}` (`{}`)\n  - raw field: `{}`\n  - source: {}\n  - cache: `{}`\n  - html: `{}`\n",
                    case.role,
                    case.commission_name,
                    case.commission_id,
                    case.raw_field,
                    case.source_url,
                    case.cache_path,
                    case.cache_file,
                ));
            }
            out.push('\n');
        }
    }

    fs::write(path, out)?;
    Ok(())
}

fn print_summary(
    persons: &[PersonRecord],
    parties: &[(String, String)],
    party_memberships: &[PartyMembership],
    commission_memberships: &[CommissionMembership],
    unresolved: &[UnresolvedCase],
    skipped: &[SkippedMember],
    out_dir: &Path,
) {
    let resolved_commission = commission_memberships.len();
    let total_commission_names = resolved_commission + unresolved.len();

    println!("[identity] persons: {}", persons.len());
    println!("[identity] parties: {}", parties.len());
    println!("[identity] party memberships: {}", party_memberships.len());
    println!(
        "[identity] commission memberships resolved: {}",
        resolved_commission
    );
    println!(
        "[identity] commission memberships unresolved: {}",
        unresolved.len()
    );
    if !skipped.is_empty() {
        println!(
            "[identity] members skipped (empty person_id): {}",
            skipped.len()
        );
    }

    if total_commission_names > 0 && !unresolved.is_empty() {
        let rate = (resolved_commission as f64 / total_commission_names as f64) * 100.0;
        println!(
            "[identity] commission name resolution rate: {:.1}%",
            rate
        );
    }

    let report_path = out_dir.join("unresolved_report.md");
    let parquet_path = out_dir.join("unresolved_persons.parquet");
    println!(
        "[identity] debug artifacts: {} | {}",
        report_path.display(),
        parquet_path.display()
    );
    println!(
        "[identity] cache root (prepend to cache_path): {}",
        cache_dir().display()
    );

    if !skipped.is_empty() {
        println!("\n[identity] skipped members (empty cvview key):");
        for s in skipped {
            println!(
                "  - {} {} (active={}) | html: {} | source: {}",
                s.first_name, s.last_name, s.active, s.cache_file, s.source_url
            );
        }
    }

    if unresolved.is_empty() {
        return;
    }

    let mut by_name: HashMap<String, Vec<&UnresolvedCase>> = HashMap::new();
    for case in unresolved {
        by_name.entry(case.raw_name.clone()).or_default().push(case);
    }

    let mut names: Vec<_> = by_name.keys().cloned().collect();
    names.sort();

    println!("\n[identity] unresolved commission member names ({} unique):", names.len());
    for name in names {
        let group = &by_name[&name];
        let first = group[0];
        println!(
            "\n  name: {:?}  reason: {}  norm: {:?} / {:?}  ({} occurrences)",
            name,
            first.reason,
            first.norm_primary,
            first.norm_reordered,
            group.len(),
        );
        for case in group {
            println!(
                "    - role={} commission={} ({})",
                case.role, case.commission_name, case.commission_id
            );
            println!("      raw_field: {:?}", case.raw_field);
            println!("      source_url: {}", case.source_url);
            println!("      cache_path: {}", case.cache_path);
            println!("      html: {}", case.cache_file);
        }
    }
}
