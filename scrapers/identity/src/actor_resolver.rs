use crate::external::{
    alias_norms_for, department_external_id, institutional_external_id, is_institutional_label,
    is_procedural_role, procedural_external_id, strip_party_suffix,
};
use crate::normalize::{apply_typo_fix, clean_raw_name, normalize_name, typo_corrections};
use crate::parquet_io::{read_all_rows, read_string_column};
use crate::resolver::{Bucket, Resolution, Resolver, UnresolvedReason};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorResolution {
    Person(String),
    ExternalPerson(String),
    Unresolved(UnresolvedReason),
}

#[derive(Debug, Clone)]
pub struct ActorResolveDetail {
    pub resolution: ActorResolution,
    pub raw_name: String,
    pub typo_corrected: String,
    pub norm_primary: String,
    pub norm_reordered: String,
    pub reason: Option<UnresolvedReason>,
}

pub struct ActorResolver {
    person_resolver: Resolver,
    external_lookup: HashMap<String, String>,
    external_ambiguous: HashSet<String>,
    institutional_map: HashMap<String, String>,
    procedural_map: HashMap<String, String>,
}

impl ActorResolver {
    pub fn load(data_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let person_resolver = Resolver::load(data_dir)?;
        let external_path = data_dir.join("identity/external_persons.parquet");
        let aliases_path = data_dir.join("identity/external_person_aliases.parquet");

        let mut external_lookup: HashMap<String, String> = HashMap::new();
        let mut external_ambiguous: HashSet<String> = HashSet::new();
        let mut institutional_map: HashMap<String, String> = HashMap::new();
        let mut procedural_map: HashMap<String, String> = HashMap::new();

        if external_path.exists() {
            for batch in read_all_rows(&external_path)? {
                let ids = read_string_column(&batch, "external_person_id")?;
                let names = read_string_column(&batch, "display_name")?;
                let kinds = read_string_column(&batch, "kind")?;
                for i in 0..batch.num_rows() {
                    let id = &ids[i];
                    let norm = normalize_name(&names[i]);
                    if kinds[i] == "institutional" {
                        institutional_map.insert(norm, id.clone());
                    } else if kinds[i] == "procedural_role" {
                        procedural_map.insert(norm, id.clone());
                    }
                }
            }
        }

        if aliases_path.exists() {
            let mut register = |norm: String, ext_id: &str| {
                if norm.is_empty() || external_ambiguous.contains(&norm) {
                    return;
                }
                if let Some(existing) = external_lookup.get(&norm) {
                    if existing != ext_id {
                        external_lookup.remove(&norm);
                        external_ambiguous.insert(norm);
                    }
                } else {
                    external_lookup.insert(norm, ext_id.to_string());
                }
            };
            for batch in read_all_rows(&aliases_path)? {
                let norms = read_string_column(&batch, "alias_norm")?;
                let ids = read_string_column(&batch, "external_person_id")?;
                for i in 0..batch.num_rows() {
                    register(norms[i].clone(), &ids[i]);
                }
            }
        }

        Ok(Self {
            person_resolver,
            external_lookup,
            external_ambiguous,
            institutional_map,
            procedural_map,
        })
    }

    pub fn person_resolver(&self) -> &Resolver {
        &self.person_resolver
    }

    pub fn resolve_actor(&self, raw: &str, bucket: Bucket) -> ActorResolution {
        self.resolve_actor_detail(raw, bucket).resolution
    }

    pub fn resolve_actor_detail(&self, raw: &str, bucket: Bucket) -> ActorResolveDetail {
        match bucket {
            Bucket::Vote | Bucket::Questioner | Bucket::CommissionMember => {
                let detail = self.person_resolver.resolve_detail(raw, bucket);
                ActorResolveDetail {
                    resolution: match detail.resolution {
                        Resolution::Resolved(id) => ActorResolution::Person(id),
                        Resolution::Unresolved(r) => ActorResolution::Unresolved(r),
                    },
                    raw_name: detail.raw_name,
                    typo_corrected: detail.typo_corrected,
                    norm_primary: detail.norm_primary,
                    norm_reordered: detail.norm_reordered,
                    reason: detail.reason,
                }
            }
            Bucket::Respondent | Bucket::Speaker | Bucket::Author => {
                self.resolve_external_bucket(raw, bucket)
            }
        }
    }

    fn resolve_external_bucket(&self, raw: &str, bucket: Bucket) -> ActorResolveDetail {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return unresolved_detail(raw, UnresolvedReason::Empty, "", "", "");
        }

        if is_institutional_label(trimmed) {
            if let Some((id, _)) = institutional_external_id(trimmed) {
                return resolved_external_detail(raw, id);
            }
            let norm = normalize_name(trimmed);
            if let Some(id) = self.institutional_map.get(&norm) {
                return resolved_external_detail(raw, id);
            }
        }

        if is_procedural_role(trimmed) {
            if let Some((id, _)) = procedural_external_id(trimmed) {
                return resolved_external_detail(raw, id);
            }
            let norm = normalize_name(trimmed);
            if let Some(id) = self.procedural_map.get(&norm) {
                return resolved_external_detail(raw, id);
            }
        }

        let cleaned = clean_raw_name(trimmed);
        let typo_map = typo_corrections();
        let corrected = apply_typo_fix(&cleaned, &typo_map);
        let stripped = strip_party_suffix(&corrected);
        let norm_primary = normalize_name(&stripped);
        let norm_reordered = normalize_name(&crate::normalize::reorder_name(&stripped));

        // Prefer Chamber MP when in index.
        for candidate in [&stripped, &corrected] {
            let person_detail = self.person_resolver.resolve_detail(candidate, bucket);
            if let Resolution::Resolved(person_id) = person_detail.resolution {
                return ActorResolveDetail {
                    resolution: ActorResolution::Person(person_id),
                    raw_name: trimmed.to_string(),
                    typo_corrected: corrected.clone(),
                    norm_primary: norm_primary.clone(),
                    norm_reordered: norm_reordered.clone(),
                    reason: None,
                };
            }
        }

        for norm in [&norm_primary, &norm_reordered] {
            if norm.is_empty() {
                continue;
            }
            if self.external_ambiguous.contains(norm) {
                return unresolved_detail(
                    raw,
                    UnresolvedReason::Ambiguous,
                    &corrected,
                    &norm_primary,
                    &norm_reordered,
                );
            }
            if let Some(ext_id) = self.external_lookup.get(norm) {
                return ActorResolveDetail {
                    resolution: ActorResolution::ExternalPerson(ext_id.clone()),
                    raw_name: trimmed.to_string(),
                    typo_corrected: corrected,
                    norm_primary: norm_primary.clone(),
                    norm_reordered: norm_reordered.clone(),
                    reason: None,
                };
            }
        }

        // Also try raw alias norms
        for norm in alias_norms_for(trimmed) {
            if let Some(ext_id) = self.external_lookup.get(&norm) {
                return ActorResolveDetail {
                    resolution: ActorResolution::ExternalPerson(ext_id.clone()),
                    raw_name: trimmed.to_string(),
                    typo_corrected: corrected.clone(),
                    norm_primary: norm_primary.clone(),
                    norm_reordered: norm_reordered.clone(),
                    reason: None,
                };
            }
        }

        unresolved_detail(
            raw,
            UnresolvedReason::NotInIndex,
            &corrected,
            &norm_primary,
            &norm_reordered,
        )
    }

    pub fn resolve_department(
        &self,
        deptnum: &str,
        title_nl: &str,
        title_fr: &str,
    ) -> ActorResolveDetail {
        let display = if !title_nl.is_empty() {
            title_nl
        } else {
            title_fr
        };
        resolved_external_detail(display, &department_external_id(deptnum))
    }
}

fn resolved_external_detail(raw: &str, id: &str) -> ActorResolveDetail {
    ActorResolveDetail {
        resolution: ActorResolution::ExternalPerson(id.to_string()),
        raw_name: raw.trim().to_string(),
        typo_corrected: raw.trim().to_string(),
        norm_primary: normalize_name(raw),
        norm_reordered: String::new(),
        reason: None,
    }
}

fn unresolved_detail(
    raw: &str,
    reason: UnresolvedReason,
    corrected: &str,
    norm_primary: &str,
    norm_reordered: &str,
) -> ActorResolveDetail {
    ActorResolveDetail {
        resolution: ActorResolution::Unresolved(reason.clone()),
        raw_name: raw.trim().to_string(),
        typo_corrected: corrected.to_string(),
        norm_primary: norm_primary.to_string(),
        norm_reordered: norm_reordered.to_string(),
        reason: Some(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::{AliasRecord, PersonRecord};

    fn test_actor_resolver(persons: Vec<PersonRecord>, aliases: Vec<AliasRecord>) -> ActorResolver {
        let person_resolver = Resolver::build(&persons, &aliases);
        ActorResolver {
            person_resolver,
            external_lookup: HashMap::from([(
                "jan jambon".to_string(),
                "ext:person:jan-jambon".to_string(),
            )]),
            external_ambiguous: HashSet::new(),
            institutional_map: HashMap::from([(
                "greffe/griffie (auteur)".to_string(),
                "ext:org:greffe-griffie".to_string(),
            )]),
            procedural_map: HashMap::from([(
                "voorzitter".to_string(),
                "ext:role:voorzitter".to_string(),
            )]),
        }
    }

    #[test]
    fn prefers_mp_over_external() {
        let resolver = test_actor_resolver(
            vec![PersonRecord {
                person_id: "01200".to_string(),
                first_name: "Bart".to_string(),
                last_name: "De Wever".to_string(),
            }],
            vec![],
        );
        assert_eq!(
            resolver.resolve_actor("Bart De Wever", Bucket::Respondent),
            ActorResolution::Person("01200".to_string())
        );
    }

    #[test]
    fn resolves_minister_as_external() {
        let resolver = test_actor_resolver(vec![], vec![]);
        assert_eq!(
            resolver.resolve_actor("Jan Jambon", Bucket::Respondent),
            ActorResolution::ExternalPerson("ext:person:jan-jambon".to_string())
        );
    }

    #[test]
    fn resolves_voorzitter_as_external() {
        let resolver = test_actor_resolver(vec![], vec![]);
        assert_eq!(
            resolver.resolve_actor("Voorzitter", Bucket::Speaker),
            ActorResolution::ExternalPerson("ext:role:voorzitter".to_string())
        );
    }

    #[test]
    fn vote_stays_person_only() {
        let resolver = test_actor_resolver(vec![], vec![]);
        assert!(matches!(
            resolver.resolve_actor("Jan Jambon", Bucket::Vote),
            ActorResolution::Unresolved(_)
        ));
    }
}
