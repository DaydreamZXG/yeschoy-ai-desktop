//! Startup recovery for keys minted by an activation that never finished.
//!
//! Evidence chain (no heuristics):
//!   1. the persisted ledger assigns each activation attempt a monotonic
//!      sequence and writes the exact token name before the create is sent;
//!   2. the minted name carries this device's scope and that sequence;
//!   3. a later attempt only ever mentions sequences above the persisted
//!      counter, so accepting one of the candidate names would have required
//!      an earlier, still-running attempt to already own a lower key.
//!
//! This module only decides; it never talks to the relay. Keys for other tools
//! are adopted by the normal activation reuse path, so deleting them here would
//! revoke a working connection without leaving anything to recover from.
use std::collections::BTreeSet;

use crate::infrastructure::secrets::issuance::IssuanceRecord;

/// Names that provably ended in a crash before their local commit.
pub(crate) fn provably_abandoned<'a>(
    ledger: &'a [IssuanceRecord],
    minted_names: &BTreeSet<String>,
    device_scope: &str,
) -> Vec<&'a IssuanceRecord> {
    ledger
        .iter()
        .filter(|record| minted_names.contains(&record.name))
        .filter(|record| name_carries_scope(&record.name, device_scope))
        .filter(|record| {
            // The attempted key is the one whose sequence this name encodes; a
            // later attempt sequestering the name is impossible.
            !ledger
                .iter()
                .any(|other| other.sequence > record.sequence && other.name == record.name)
        })
        .collect()
}

/// The on-disk name is `<token prefix>-<device scope><8 hex nonce>`; the scope
/// precedes the 16-hex suffix, so match on the whole suffix, not its last dash.
fn name_carries_scope(name: &str, device_scope: &str) -> bool {
    let Some((_head, suffix)) = name.rsplit_once('-') else {
        return false;
    };
    suffix.len() == 16 && suffix.starts_with(device_scope)
}

/// Delete candidate keys through the relay. Returns how many deletions the relay
/// confirmed; transport failures leave the records in place for the next run.
pub(crate) async fn release(
    api: &impl super::tokens::TokenApi,
    candidates: &[&IssuanceRecord],
) -> usize {
    let mut removed = 0;
    for record in candidates {
        let Ok((status, value)) = api
            .request(reqwest::Method::GET, "/api/token/?p=1&size=100", None)
            .await
        else {
            continue;
        };
        if !(200..300).contains(&status) {
            continue;
        }
        let id = value["data"]["items"].as_array().and_then(|items| {
            items.iter().find_map(|item| {
                (item["name"].as_str() == Some(record.name.as_str()))
                    .then(|| item["id"].as_u64())
                    .flatten()
            })
        });
        if let Some(id) = id {
            if matches!(
                api.request(reqwest::Method::DELETE, &format!("/api/token/{id}"), None)
                    .await,
                Ok((status, ref value)) if (200..300).contains(&status) && value["success"] == true
            ) {
                removed += 1;
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, sequence: u64) -> IssuanceRecord {
        IssuanceRecord {
            name: name.into(),
            tool_id: "pi".into(),
            billing_group: "default".into(),
            recorded_at_epoch_ms: 0,
            sequence,
        }
    }

    fn name_on(scope: &str, nonce: u32) -> String {
        crate::application::activation::tokens_tests_support::minted_name_for(
            "野菜API Pi",
            nonce,
            scope,
        )
    }

    #[test]
    fn only_exact_known_names_on_this_device_are_candidates() {
        let name = name_on("deadbeef", 2);
        let ledger = vec![record(&name, 2)];
        let minted: BTreeSet<String> = [name.clone()].into();
        assert_eq!(provably_abandoned(&ledger, &minted, "deadbeef").len(), 1);
        assert!(provably_abandoned(&ledger, &minted, "feedface").is_empty());
        assert!(provably_abandoned(&ledger, &BTreeSet::new(), "deadbeef").is_empty());
    }

    #[test]
    fn a_name_the_ledger_never_recorded_is_never_deleted() {
        let ledger = vec![record(&name_on("deadbeef", 2), 2)];
        let minted: BTreeSet<String> = [name_on("deadbeef", 9)].into();
        assert!(provably_abandoned(&ledger, &minted, "deadbeef").is_empty());
    }

    #[test]
    fn legacy_names_without_this_devices_scope_are_left_alone() {
        let ledger = vec![record("野菜API cx-e8039bc07a1b2c3d-0011223344556677", 1)];
        let minted: BTreeSet<String> =
            ["野菜API cx-e8039bc07a1b2c3d-0011223344556677".to_string()].into();
        assert!(provably_abandoned(&ledger, &minted, "deadbeef").is_empty());
    }

    /// The decision plus the relay calls it drives. The station transport
    /// wrapper (origin-bound relative paths) is covered by the token tests.
    #[tokio::test]
    async fn release_deletes_only_the_exact_listed_name_and_reports_successes() {
        use crate::application::activation::tokens_tests_support::{fake_key, FakeTokens};
        let api = FakeTokens::default();
        let own = name_on("deadbeef", 2);
        api.seed(fake_key(7, &own, "default"));
        api.seed(fake_key(8, &name_on("deadbeef", 9), "default"));
        let record = record(&own, 2);
        let removed = release(&api, &[&record]).await;
        assert_eq!(removed, 1);
        let left: Vec<u64> = api
            .tokens
            .lock()
            .unwrap()
            .iter()
            .filter_map(|t| t["id"].as_u64())
            .collect();
        assert_eq!(left, vec![8], "an unrecorded key of the same device stays");
    }
}
