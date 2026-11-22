//! UUID de-duplication and order-preserving merge (pure).

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::Task;

/// UUIDs per Read Scraping Tasks request (open question 6).
pub const UUID_BATCH_SIZE: usize = 100;

/// Removes duplicates, keeping first occurrences in order.
pub(crate) fn unique(uuids: &[Uuid]) -> Vec<Uuid> {
    let mut seen = HashSet::new();
    uuids.iter().copied().filter(|u| seen.insert(*u)).collect()
}

/// The task for each key in key order (keys without a task are skipped).
pub(crate) fn in_order(keys: &[Uuid], tasks: Vec<Task>) -> Vec<Task> {
    let mut by_uuid: HashMap<Uuid, Task> = HashMap::new();
    for task in tasks {
        if let Some(u) = task.uuid {
            by_uuid.entry(u).or_insert(task);
        }
    }
    keys.iter()
        .filter_map(|k| by_uuid.get(k).cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn task(u: Uuid) -> Task {
        serde_json::from_value(serde_json::json!({ "uuid": u, "url": "x", "status": "SUCCESS" }))
            .unwrap()
    }

    proptest! {
        #[test]
        fn fr_3_merge_restores_input_order(n in 0usize..250, drop_every in 2usize..10) {
            let keys: Vec<Uuid> = (0..n).map(|i| Uuid::from_u128(i as u128)).collect();
            let found: Vec<Task> = keys.iter().rev().enumerate().filter(|(i, _)| i % drop_every != 0).map(|(_, k)| task(*k)).collect();
            let expected: Vec<Uuid> = keys.iter().copied().filter(|k| found.iter().any(|t| t.uuid == Some(*k))).collect();
            let merged: Vec<Uuid> = in_order(&keys, found).into_iter().filter_map(|t| t.uuid).collect();
            prop_assert_eq!(merged, expected);
        }

        #[test]
        fn fr_5_chunks_cover_the_input_exactly_once(n in 0u128..350) {
            let keys = unique(&(0..n).chain(0..n / 2).map(Uuid::from_u128).collect::<Vec<_>>());
            let chunks: Vec<&[Uuid]> = keys.chunks(UUID_BATCH_SIZE).collect();
            prop_assert!(chunks.iter().all(|c| !c.is_empty() && c.len() <= UUID_BATCH_SIZE));
            prop_assert_eq!(chunks.concat(), (0..n).map(Uuid::from_u128).collect::<Vec<_>>());
        }
    }

    #[test]
    fn ignores_tasks_without_uuid_and_duplicates() {
        let u = Uuid::from_u128(1);
        let mut first = task(u);
        first.url = "first".into();
        let no_uuid: Task =
            serde_json::from_value(serde_json::json!({ "url": "x", "status": "SUCCESS" })).unwrap();
        let merged = in_order(&[u, u], vec![no_uuid, first, task(u)]);
        assert_eq!(
            merged.iter().map(|t| t.url.as_str()).collect::<Vec<_>>(),
            ["first", "first"]
        );
    }
}
