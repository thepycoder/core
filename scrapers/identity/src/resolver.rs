use crate::normalize::{
    PersonName, apply_typo_fix, clean_raw_name, normalize_name, typo_corrections,
};
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
    actr_lookup: HashMap<String, String>,
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

        let mut actr_lookup: HashMap<String, String> = HashMap::new();

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
            if let Some(digits) = person
                .person_id
                .strip_prefix('O')
                .or_else(|| person.person_id.strip_prefix('o'))
            {
                actr_lookup.insert(digits.to_string(), person.person_id.clone());
                let trimmed = digits.trim_start_matches('0');
                if !trimmed.is_empty() {
                    actr_lookup.insert(trimmed.to_string(), person.person_id.clone());
                }
            }
        }

        for alias in aliases {
            register(alias.alias_norm.clone(), &alias.person_id);
        }

        Self {
            typo_map,
            lookup,
            ambiguous,
            actr_lookup,
        }
    }

    pub fn resolve_by_actr_id(&self, actr_id: &str) -> Resolution {
        let trimmed = actr_id.trim();
        if trimmed.is_empty() {
            return Resolution::Unresolved(UnresolvedReason::Empty);
        }
        if let Some(person_id) = self.actr_lookup.get(trimmed) {
            return Resolution::Resolved(person_id.clone());
        }
        let no_zeros = trimmed.trim_start_matches('0');
        if let Some(person_id) = self.actr_lookup.get(no_zeros) {
            return Resolution::Resolved(person_id.clone());
        }
        Resolution::Unresolved(UnresolvedReason::NotInIndex)
    }

    pub fn resolve_person(&self, raw: &str, ctx: Bucket) -> Resolution {
        self.resolve_detail(raw, ctx).resolution
    }

    pub fn resolve_detail(&self, raw: &str, _ctx: Bucket) -> ResolveDetail {
        let trimmed = clean_raw_name(raw);
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

        let corrected = apply_typo_fix(&trimmed, &self.typo_map);
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
                    raw_name: raw.trim().to_string(),
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
            raw_name: raw.trim().to_string(),
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
    fn resolves_vote_appendix_name_with_particles() {
        let persons = vec![PersonRecord {
            person_id: "06595".to_string(),
            first_name: "Wim".to_string(),
            last_name: "Van der Donckt".to_string(),
        }];
        let aliases = vec![AliasRecord {
            alias_norm: "van der donckt".to_string(),
            person_id: "06595".to_string(),
            source: "last_name".to_string(),
        }];
        let resolver = Resolver::build(&persons, &aliases);
        assert_eq!(
            resolver.resolve_person("Donckt Wim Van der", Bucket::Vote),
            Resolution::Resolved("06595".to_string())
        );
    }

    #[test]
    fn resolves_abbreviated_compound_surname() {
        let persons = vec![PersonRecord {
            person_id: "08151".to_string(),
            first_name: "Lydia".to_string(),
            last_name: "Mutyebele Ngoi".to_string(),
        }];
        let aliases = vec![AliasRecord {
            alias_norm: "mutyebele ngoi".to_string(),
            person_id: "08151".to_string(),
            source: "last_name".to_string(),
        }];
        let resolver = Resolver::build(&persons, &aliases);
        assert_eq!(
            resolver.resolve_person("Ngoi Mutyebele", Bucket::Vote),
            Resolution::Resolved("08151".to_string())
        );
    }

    #[test]
    fn resolves_speaker_with_title_prefix() {
        let persons = vec![PersonRecord {
            person_id: "06447".to_string(),
            first_name: "Alexander".to_string(),
            last_name: "De Croo".to_string(),
        }];
        let resolver = Resolver::build(&persons, &[]);
        assert_eq!(
            resolver.resolve_person("E erste minister  Alexander De Croo", Bucket::Speaker),
            Resolution::Resolved("06447".to_string())
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
