use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard};

#[derive(Debug)]
pub struct EntryIdempotencyCache {
    max_keys: usize,
    state: Mutex<IdempotencyState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdempotencyRecordState {
    Reserved,
    Committed,
}

#[derive(Debug, PartialEq, Eq)]
struct IdempotencyRecord {
    digest: String,
    state: IdempotencyRecordState,
}

#[derive(Debug, Default)]
struct IdempotencyState {
    order: VecDeque<String>,
    records: HashMap<String, IdempotencyRecord>,
}

pub(crate) enum EntryIdempotencyReservationOutcome<'a> {
    Reserved(EntryIdempotencyReservation<'a>),
    DuplicateCommitted,
    InFlight,
    Conflict,
    CapacityExhausted,
}

pub(crate) struct EntryIdempotencyReservation<'a> {
    cache: &'a EntryIdempotencyCache,
    key: Option<String>,
    digest: String,
    committed: bool,
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
                committed: false,
            });
        }
        let mut state = self.lock_state();
        if let Some(record) = state.records.get(key) {
            return if record.digest != digest {
                EntryIdempotencyReservationOutcome::Conflict
            } else if record.state == IdempotencyRecordState::Committed {
                EntryIdempotencyReservationOutcome::DuplicateCommitted
            } else {
                EntryIdempotencyReservationOutcome::InFlight
            };
        }
        while state.records.len() >= self.max_keys {
            let Some(oldest) = state.order.pop_front() else {
                return EntryIdempotencyReservationOutcome::CapacityExhausted;
            };
            if state
                .records
                .get(&oldest)
                .is_some_and(|record| record.state == IdempotencyRecordState::Committed)
            {
                state.records.remove(&oldest);
            }
        }
        state.records.insert(
            key.to_string(),
            IdempotencyRecord {
                digest: digest.to_string(),
                state: IdempotencyRecordState::Reserved,
            },
        );
        EntryIdempotencyReservationOutcome::Reserved(EntryIdempotencyReservation {
            cache: self,
            key: Some(key.to_string()),
            digest: digest.to_string(),
            committed: false,
        })
    }

    fn lock_state(&self) -> MutexGuard<'_, IdempotencyState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl EntryIdempotencyReservation<'_> {
    pub(crate) fn commit(mut self) {
        let Some(key) = self.key.as_ref() else {
            self.committed = true;
            return;
        };
        let mut state = self.cache.lock_state();
        if let Some(record) = state.records.get_mut(key) {
            if record.digest == self.digest && record.state == IdempotencyRecordState::Reserved {
                record.state = IdempotencyRecordState::Committed;
                state.order.push_back(key.clone());
            }
        }
        while state.order.len() > self.cache.max_keys {
            if let Some(oldest) = state.order.pop_front() {
                if state
                    .records
                    .get(&oldest)
                    .is_some_and(|record| record.state == IdempotencyRecordState::Committed)
                {
                    state.records.remove(&oldest);
                }
            }
        }
        self.committed = true;
    }
}

impl Drop for EntryIdempotencyReservation<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let Some(key) = self.key.as_ref() else {
            return;
        };
        let mut state = self.cache.lock_state();
        if state.records.get(key).is_some_and(|record| {
            record.digest == self.digest && record.state == IdempotencyRecordState::Reserved
        }) {
            state.records.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_reservation_releases_key_but_committed_reservation_deduplicates() {
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
        reservation.commit();
        assert!(matches!(
            cache.reserve("key", "digest-b"),
            EntryIdempotencyReservationOutcome::DuplicateCommitted
        ));
        assert!(matches!(
            cache.reserve("key", "digest-c"),
            EntryIdempotencyReservationOutcome::Conflict
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
    fn committed_records_are_evicted_before_new_reservations() {
        let cache = EntryIdempotencyCache::new(1);
        let EntryIdempotencyReservationOutcome::Reserved(first) = cache.reserve("first", "digest")
        else {
            panic!("first reservation");
        };
        first.commit();

        let EntryIdempotencyReservationOutcome::Reserved(second) =
            cache.reserve("second", "digest")
        else {
            panic!("committed record must be evicted for a new reservation");
        };
        assert!(matches!(
            cache.reserve("first", "digest"),
            EntryIdempotencyReservationOutcome::CapacityExhausted
        ));
        drop(second);
    }
}
