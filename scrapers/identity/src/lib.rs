pub mod actor_resolver;
pub mod external;
pub mod normalize;
pub mod parquet_io;
pub mod resolver;

pub use actor_resolver::{ActorResolution, ActorResolveDetail, ActorResolver};
pub use external::{
    ExternalAliasRecord, ExternalContextRecord, ExternalKind, ExternalPersonRecord,
    alias_norms_for, classify_named_external, institutional_external_id, is_institutional_label,
    is_procedural_role, person_external_id, strip_party_suffix,
};
pub use normalize::{
    PersonName, apply_typo_fix, clean_raw_name, convert_name, normalize_name, reorder_name,
    typo_corrections,
};
pub use resolver::{Bucket, Resolution, ResolveDetail, Resolver, UnresolvedReason};
