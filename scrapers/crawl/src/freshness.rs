//! Documented refresh intervals for mutable sources (Phase C freshness QA).

/// Maximum age in days before a mutable source is considered stale.
#[derive(Debug, Clone, Copy)]
pub struct FreshnessPolicy {
    pub source: &'static str,
    pub max_age_days: i64,
}

pub const FRESHNESS_POLICIES: &[FreshnessPolicy] = &[
    FreshnessPolicy {
        source: "sessions",
        max_age_days: 7,
    },
    FreshnessPolicy {
        source: "members",
        max_age_days: 7,
    },
    FreshnessPolicy {
        source: "commissions",
        max_age_days: 7,
    },
    FreshnessPolicy {
        source: "lobby",
        max_age_days: 30,
    },
    FreshnessPolicy {
        source: "remunerations",
        max_age_days: 30,
    },
    FreshnessPolicy {
        source: "dossiers",
        max_age_days: 7,
    },
];

pub fn freshness_policy(source: &str) -> Option<&'static FreshnessPolicy> {
    FRESHNESS_POLICIES.iter().find(|p| p.source == source)
}

/// Parse an RFC3339-ish timestamp (`YYYY-MM-DDTHH:MM:SSZ`) into unix days since epoch.
pub fn rfc3339_to_unix_days(ts: &str) -> Option<i64> {
    let date = ts.get(..10)?;
    let y: i64 = date.get(..4)?.parse().ok()?;
    let m: i64 = date.get(5..7)?.parse().ok()?;
    let d: i64 = date.get(8..10)?.parse().ok()?;
    Some(days_from_civil(y, m, d))
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // Howard Hinnant civil_from_days inverse.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as u64;
    (era * 146_097 + doe as i64) - 719_468
}

pub fn days_between_rfc3339(earlier: &str, later: &str) -> Option<i64> {
    Some(rfc3339_to_unix_days(later)? - rfc3339_to_unix_days(earlier)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_lookup() {
        assert_eq!(freshness_policy("lobby").unwrap().max_age_days, 30);
        assert!(freshness_policy("unknown").is_none());
    }

    #[test]
    fn day_delta() {
        assert_eq!(
            days_between_rfc3339("2026-07-01T00:00:00Z", "2026-07-08T12:00:00Z"),
            Some(7)
        );
    }
}
