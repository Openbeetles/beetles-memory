use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

#[derive(Debug)]
pub struct EntryIdempotencyCache {
    max_keys: usize,
    state: Mutex<IdempotencyState>,
}

#[derive(Debug, PartialEq, Eq)]
struct IdempotencyRecord {
    digest: String,
}

#[derive(Debug, Default)]
struct IdempotencyState {
    records: HashMap<String, IdempotencyRecord>,
}

pub(crate) enum EntryIdempotencyReservationOutcome<'a> {
    Reserved(EntryIdempotencyReservation<'a>),
    InFlight,
    Conflict,
    CapacityExhausted,
}

pub(crate) struct EntryIdempotencyReservation<'a> {
    cache: &'a EntryIdempotencyCache,
    key: Option<String>,
    digest: String,
}

impl EntryIdempotencyCache {
    pub fn new(max_keys: usize) -> Self {
        Self {
            max_keys,
            state: Mutex::new(IdempotencyState::default()),
        }
    }

    pub(crate) fn reserve<'a>(
        &'a self,
        key: &str,
        digest: &str,
    ) -> EntryIdempotencyReservationOutcome<'a> {
        if self.max_keys == 0 || key.trim().is_empty() {
            return EntryIdempotencyReservationOutcome::Reserved(EntryIdempotencyReservation {
                cache: self,
                key: None,
                digest: digest.to_string(),
            });
        }
        let mut state = self.lock_state();
        if let Some(record) = state.records.get(key) {
            return if record.digest != digest {
                EntryIdempotencyReservationOutcome::Conflict
            } else {
                EntryIdempotencyReservationOutcome::InFlight
            };
        }
        if state.records.len() >= self.max_keys {
            return EntryIdempotencyReservationOutcome::CapacityExhausted;
        }
        state.records.insert(
            key.to_string(),
            IdempotencyRecord {
                digest: digest.to_string(),
            },
        );
        EntryIdempotencyReservationOutcome::Reserved(EntryIdempotencyReservation {
            cache: self,
            key: Some(key.to_string()),
            digest: digest.to_string(),
        })
    }

    fn lock_state(&self) -> MutexGuard<'_, IdempotencyState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for EntryIdempotencyReservation<'_> {
    fn drop(&mut self) {
        let Some(key) = self.key.as_ref() else {
            return;
        };
        let mut state = self.cache.lock_state();
        if state
            .records
            .get(key)
            .is_some_and(|record| record.digest == self.digest)
        {
            state.records.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_reservation_releases_key_without_retaining_committed_truth() {
        let cache = EntryIdempotencyCache::new(2);
        let EntryIdempotencyReservationOutcome::Reserved(reservation) =
            cache.reserve("key", "digest-a")
        else {
            panic!("first reservation");
        };
        drop(reservation);
        let EntryIdempotencyReservationOutcome::Reserved(reservation) =
            cache.reserve("key", "digest-b")
        else {
            panic!("released key must be reusable");
        };
        drop(reservation);
        assert!(matches!(
            cache.reserve("key", "digest-b"),
            EntryIdempotencyReservationOutcome::Reserved(_)
        ));
    }

    #[test]
    fn active_reservation_is_not_reported_as_a_committed_duplicate() {
        let cache = EntryIdempotencyCache::new(2);
        let EntryIdempotencyReservationOutcome::Reserved(first) = cache.reserve("key", "digest")
        else {
            panic!("first reservation");
        };

        assert!(matches!(
            cache.reserve("key", "digest"),
            EntryIdempotencyReservationOutcome::InFlight
        ));
        drop(first);
        assert!(matches!(
            cache.reserve("key", "digest"),
            EntryIdempotencyReservationOutcome::Reserved(_)
        ));
    }

    #[test]
    fn active_reservations_are_bounded_by_capacity() {
        let cache = EntryIdempotencyCache::new(1);
        let EntryIdempotencyReservationOutcome::Reserved(first) = cache.reserve("first", "digest")
        else {
            panic!("first reservation");
        };

        assert!(matches!(
            cache.reserve("second", "digest"),
            EntryIdempotencyReservationOutcome::CapacityExhausted
        ));
        drop(first);
        assert!(matches!(
            cache.reserve("second", "digest"),
            EntryIdempotencyReservationOutcome::Reserved(_)
        ));
    }

    #[test]
    fn completed_reservations_release_capacity() {
        let cache = EntryIdempotencyCache::new(1);
        let EntryIdempotencyReservationOutcome::Reserved(first) = cache.reserve("first", "digest")
        else {
            panic!("first reservation");
        };
        drop(first);

        let EntryIdempotencyReservationOutcome::Reserved(second) =
            cache.reserve("second", "digest")
        else {
            panic!("completed reservation must release capacity");
        };
        drop(second);
    }
}
