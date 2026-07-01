use crawl::paths::data_dir;
use identity::resolver::Resolver;
use normalize::authored::{normalize_authored, write_authored};
use normalize::common::{dedupe_unresolved, verify_staging, write_unresolved_persons, UnresolvedRow};
use normalize::questions::{normalize_asked, write_asked};
use normalize::roles::{normalize_holds_role, write_holds_role};
use normalize::utterances::{normalize_utterances, write_utterances};
use normalize::vote_casts::{
    normalize_vote_casts, write_vote_casts, write_vote_reconciliation,
};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let root = data_dir();
    verify_staging(&root)?;

    let out_dir = root.join("normalized");
    std::fs::create_dir_all(&out_dir)?;

    let resolver = Resolver::load(&root)?;

    let vote_out = normalize_vote_casts(&root, &resolver)?;
    write_vote_casts(&out_dir.join("vote_casts.parquet"), &vote_out.casts)?;
    write_vote_reconciliation(
        &out_dir.join("vote_reconciliation.parquet"),
        &vote_out.reconciliation,
    )?;

    let authored_out = normalize_authored(&root, &resolver)?;
    write_authored(&out_dir.join("authored.parquet"), &authored_out.rows)?;

    let asked_out = normalize_asked(&root, &resolver)?;
    write_asked(&out_dir.join("asked.parquet"), &asked_out.asked)?;

    let roles_out = normalize_holds_role(&root, &resolver)?;
    write_holds_role(&out_dir.join("holds_role.parquet"), &roles_out.rows)?;

    let utterances_out = normalize_utterances(&root, &resolver)?;
    write_utterances(&out_dir.join("utterances.parquet"), &utterances_out.rows)?;

    let mut unresolved: Vec<UnresolvedRow> = Vec::new();
    unresolved.extend(vote_out.unresolved);
    unresolved.extend(authored_out.unresolved);
    unresolved.extend(asked_out.unresolved);
    unresolved.extend(roles_out.unresolved);
    unresolved.extend(utterances_out.unresolved);
    dedupe_unresolved(&mut unresolved);
    write_unresolved_persons(&out_dir.join("unresolved_persons.parquet"), &unresolved)?;

    let mismatches = vote_out
        .reconciliation
        .iter()
        .filter(|row| row.reconciled != "true")
        .count();

    print_summary(
        vote_out.casts.len(),
        authored_out.rows.len(),
        asked_out.asked.len(),
        roles_out.rows.len(),
        utterances_out.rows.len(),
        unresolved.len(),
        mismatches,
        &out_dir,
    );

    Ok(())
}

fn print_summary(
    vote_casts: usize,
    authored: usize,
    asked: usize,
    holds_role: usize,
    utterances: usize,
    unresolved: usize,
    vote_mismatches: usize,
    out_dir: &std::path::Path,
) {
    println!("[normalize] vote_casts: {vote_casts}");
    println!("[normalize] authored: {authored}");
    println!("[normalize] asked: {asked}");
    println!("[normalize] holds_role: {holds_role}");
    println!("[normalize] utterances: {utterances}");
    println!("[normalize] unresolved persons: {unresolved}");
    println!("[normalize] vote reconciliation mismatches: {vote_mismatches}");
    println!("[normalize] output dir: {}", out_dir.display());
}
