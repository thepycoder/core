use crate::normalize::{apply_typo_fix, normalize_name, typo_corrections, PersonName};
use crate::parquet_io::{read_all_rows, read_string_column};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Vote,
    Questioner,
    Respondent,
    Author,
    CommissionMember,
    Speaker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Resolved(String),
    Unresolved(UnresolvedReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnresolvedReason {
    Empty,
    NotInIndex,
    Ambiguous,
}

#[derive(Debug, Clone)]
pub struct ResolveDetail {
    pub resolution: Resolution,
    pub raw_name: String,
    pub typo_corrected: String,
    pub norm_primary: String,
    pub norm_reordered: String,
    pub reason: Option<UnresolvedReason>,
}

#[derive(Debug, Clone)]
pub struct PersonRecord {
    pub person_id: String,
    pub first_name: String,
    pub last_name: String,
}

#[derive(Debug, Clone)]
pub struct AliasRecord {
    pub alias_norm: String,
    pub person_id: String,
    pub source: String,
}

pub struct Resolver {
    typo_map: HashMap<String, String>,
    lookup: HashMap<String, String>,
    ambiguous: HashSet<String>,
}

impl Resolver {
    pub fn load(data_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let persons_path = data_dir.join("identity/persons.parquet");
        let aliases_path = data_dir.join("identity/person_aliases.parquet");

        let persons = load_persons(&persons_path)?;
        let aliases = load_aliases(&aliases_path)?;
        Ok(Self::build(&persons, &aliases))
    }

    pub fn build(persons: &[PersonRecord], aliases: &[AliasRecord]) -> Self {
        let typo_map = typo_corrections();
        let mut lookup: HashMap<String, String> = HashMap::new();
        let mut ambiguous: HashSet<String> = HashSet::new();

        let mut register = |norm: String, person_id: &str| {
            if norm.is_empty() {
                return;
            }
            if ambiguous.contains(&norm) {
                return;
            }
            if let Some(existing) = lookup.get(&norm) {
                if existing != person_id {
                    lookup.remove(&norm);
                    ambiguous.insert(norm);
                }
            } else {
                lookup.insert(norm, person_id.to_string());
            }
        };

        for person in persons {
            let name = PersonName {
                first_name: person.first_name.clone(),
                last_name: person.last_name.clone(),
            };
            register(name.normalized_full(), &person.person_id);
            let reversed = name.normalized_reversed();
            if reversed != name.normalized_full() {
                register(reversed, &person.person_id);
            }
        }

        for alias in aliases {
            register(alias.alias_norm.clone(), &alias.person_id);
        }

        Self {
            typo_map,
            lookup,
            ambiguous,
        }
    }

    pub fn resolve_person(&self, raw: &str, ctx: Bucket) -> Resolution {
        self.resolve_detail(raw, ctx).resolution
    }

    pub fn resolve_detail(&self, raw: &str, _ctx: Bucket) -> ResolveDetail {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return ResolveDetail {
                resolution: Resolution::Unresolved(UnresolvedReason::Empty),
                raw_name: raw.to_string(),
                typo_corrected: String::new(),
                norm_primary: String::new(),
                norm_reordered: String::new(),
                reason: Some(UnresolvedReason::Empty),
            };
        }

        let corrected = apply_typo_fix(trimmed, &self.typo_map);
        let norm_primary = normalize_name(&corrected);
        let norm_reordered = normalize_name(&crate::normalize::reorder_name(&corrected));

        let candidates = [norm_primary.clone(), norm_reordered.clone()];
        let mut ambiguous_hit = false;

        for norm in &candidates {
            if norm.is_empty() {
                continue;
            }
            if self.ambiguous.contains(norm) {
                ambiguous_hit = true;
                continue;
            }
            if let Some(person_id) = self.lookup.get(norm) {
                return ResolveDetail {
                    resolution: Resolution::Resolved(person_id.clone()),
                    raw_name: trimmed.to_string(),
                    typo_corrected: corrected,
                    norm_primary,
                    norm_reordered,
                    reason: None,
                };
            }
        }

        let reason = if ambiguous_hit {
            UnresolvedReason::Ambiguous
        } else {
            UnresolvedReason::NotInIndex
        };

        ResolveDetail {
            resolution: Resolution::Unresolved(reason.clone()),
            raw_name: trimmed.to_string(),
            typo_corrected: corrected,
            norm_primary,
            norm_reordered,
            reason: Some(reason),
        }
    }
}

fn load_persons(path: &Path) -> Result<Vec<PersonRecord>, Box<dyn std::error::Error>> {
    let mut persons = Vec::new();
    for batch in read_all_rows(path)? {
        let person_ids = read_string_column(&batch, "person_id")?;
        let first_names = read_string_column(&batch, "first_name")?;
        let last_names = read_string_column(&batch, "last_name")?;
        for i in 0..batch.num_rows() {
            persons.push(PersonRecord {
                person_id: person_ids[i].clone(),
                first_name: first_names[i].clone(),
                last_name: last_names[i].clone(),
            });
        }
    }
    Ok(persons)
}

fn load_aliases(path: &Path) -> Result<Vec<AliasRecord>, Box<dyn std::error::Error>> {
    let mut aliases = Vec::new();
    for batch in read_all_rows(path)? {
        let alias_norms = read_string_column(&batch, "alias_norm")?;
        let person_ids = read_string_column(&batch, "person_id")?;
        for i in 0..batch.num_rows() {
            aliases.push(AliasRecord {
                alias_norm: alias_norms[i].clone(),
                person_id: person_ids[i].clone(),
                source: read_string_column(&batch, "source")?[i].clone(),
            });
        }
    }
    Ok(aliases)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_by_full_name() {
        let persons = vec![PersonRecord {
            person_id: "O1234".to_string(),
            first_name: "Jan".to_string(),
            last_name: "Jambon".to_string(),
        }];
        let aliases = vec![];
        let resolver = Resolver::build(&persons, &aliases);
        assert_eq!(
            resolver.resolve_person("Jan Jambon", Bucket::Vote),
            Resolution::Resolved("O1234".to_string())
        );
    }

    #[test]
    fn resolves_reversed_name() {
        let persons = vec![PersonRecord {
            person_id: "O1234".to_string(),
            first_name: "Jan".to_string(),
            last_name: "Jambon".to_string(),
        }];
        let resolver = Resolver::build(&persons, &[]);
        assert_eq!(
            resolver.resolve_person("Jambon Jan", Bucket::CommissionMember),
            Resolution::Resolved("O1234".to_string())
        );
    }

    #[test]
    fn load_from_identity_parquet_when_present() {
        let data_dir = std::path::Path::new("data");
        if !data_dir.join("identity/persons.parquet").exists() {
            return;
        }
        let resolver = Resolver::load(data_dir).expect("load identity tables");
        assert_eq!(
            resolver.resolve_person("Jan Jambon", Bucket::Vote),
            Resolution::Unresolved(UnresolvedReason::NotInIndex)
        );
        // At least one known member should resolve once data is built.
        assert!(matches!(
            resolver.resolve_person("Jambon Jan", Bucket::CommissionMember),
            Resolution::Resolved(_)
        ));
    }
}
