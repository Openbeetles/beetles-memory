use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

#[derive(Debug)]
pub struct EntryIdempotencyCache {
    max_keys: usize,
    state: Mutex<IdempotencyState>,
}

#[derive(Debug, Default)]
struct IdempotencyState {
    order: VecDeque<String>,
    keys: HashSet<String>,
}

impl EntryIdempotencyCache {
    pub fn new(max_keys: usize) -> Self {
        Self {
            max_keys,
            state: Mutex::new(IdempotencyState::default()),
        }
    }

    pub fn remember(&self, key: &str) -> bool {
        if self.max_keys == 0 || key.trim().is_empty() {
            return true;
        }
        let mut state = self.state.lock().expect("idempotency cache poisoned");
        if state.keys.contains(key) {
            return false;
        }
        state.keys.insert(key.to_string());
        state.order.push_back(key.to_string());
        while state.order.len() > self.max_keys {
            if let Some(oldest) = state.order.pop_front() {
                state.keys.remove(&oldest);
            }
        }
        true
    }
}
