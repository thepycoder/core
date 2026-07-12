use normalize::answered::{normalize_answered, write_answered};
use normalize::hearings::{normalize_invited, write_invited};
use normalize::interpellations::{
    normalize_interpellations, write_interpellated, write_interpellation_responded,
};
use normalize::authored::{normalize_authored, write_authored};
use normalize::common::{dedupe_unresolved, verify_staging, write_unresolved_persons, UnresolvedRow};
use normalize::questions::{normalize_asked, write_asked};
use normalize::roles::{normalize_holds_role, write_holds_role};
use normalize::utterances::{normalize_utterances, write_utterances};
use normalize::vote_casts::{
    normalize_vote_casts, write_vote_casts, write_vote_reconciliation,
};
use crawl::paths::data_dir;
use identity::actor_resolver::ActorResolver;
use identity::resolver::Resolver;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let root = data_dir();
    verify_staging(&root)?;

    let out_dir = root.join("normalized");
    std::fs::create_dir_all(&out_dir)?;

    let resolver = Resolver::load(&root)?;
    let actor_resolver = ActorResolver::load(&root)?;

    let vote_out = normalize_vote_casts(&root, &resolver)?;
    write_vote_casts(&out_dir.join("vote_casts.parquet"), &vote_out.casts)?;
    write_vote_reconciliation(
        &out_dir.join("vote_reconciliation.parquet"),
        &vote_out.reconciliation,
    )?;

    let authored_out = normalize_authored(&root, &actor_resolver)?;
    write_authored(&out_dir.join("authored.parquet"), &authored_out.rows)?;

    let asked_out = normalize_asked(&root, &resolver)?;
    write_asked(&out_dir.join("asked.parquet"), &asked_out.asked)?;

    let answered_out = normalize_answered(&root, &actor_resolver)?;
    write_answered(&out_dir.join("answered.parquet"), &answered_out.rows)?;

    let roles_out = normalize_holds_role(&root, &resolver)?;
    write_holds_role(&out_dir.join("holds_role.parquet"), &roles_out.rows)?;

    let invited_out = normalize_invited(&root, &actor_resolver)?;
    write_invited(&out_dir.join("invited.parquet"), &invited_out.rows)?;

    let interpellation_out = normalize_interpellations(&root, &resolver, &actor_resolver)?;
    write_interpellated(
        &out_dir.join("interpellated.parquet"),
        &interpellation_out.interpellated,
    )?;
    write_interpellation_responded(
        &out_dir.join("interpellation_responded.parquet"),
        &interpellation_out.responded,
    )?;

    let utterances_out = normalize_utterances(&root, &actor_resolver)?;
    write_utterances(&out_dir.join("utterances.parquet"), &utterances_out.rows)?;

    let mut unresolved: Vec<UnresolvedRow> = Vec::new();
    unresolved.extend(vote_out.unresolved);
    unresolved.extend(authored_out.unresolved);
    unresolved.extend(asked_out.unresolved);
    unresolved.extend(answered_out.unresolved);
    unresolved.extend(roles_out.unresolved);
    unresolved.extend(invited_out.unresolved);
    unresolved.extend(interpellation_out.unresolved);
    unresolved.extend(utterances_out.unresolved);
    dedupe_unresolved(&mut unresolved);
    write_unresolved_persons(&out_dir.join("unresolved_persons.parquet"), &unresolved)?;

    eprintln!(
        "Normalized: {} vote casts, {} authored, {} asked, {} answered, {} holds_role, {} invited, {} interpellated, {} interpellation_responded, {} utterances; {} unresolved",
        vote_out.casts.len(),
        authored_out.rows.len(),
        asked_out.asked.len(),
        answered_out.rows.len(),
        roles_out.rows.len(),
        invited_out.rows.len(),
        interpellation_out.interpellated.len(),
        interpellation_out.responded.len(),
        utterances_out.rows.len(),
        unresolved.len()
    );

    Ok(())
}
