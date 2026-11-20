//! Bounded structured diagnostics shared by the inspector and development tools.
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
};

pub const TARGET: &str = "xrack::diagnostics";
const MAX_EVENTS: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Field {
    Number(u64),
    Text(String),
}

pub type Fields = BTreeMap<&'static str, Field>;

pub fn reload_committed(generation: u64) {
    tracing::debug!(target: TARGET, event = "reload_committed", generation);
}

#[derive(Clone, Default)]
pub struct Buffer(Arc<Mutex<VecDeque<Fields>>>);

impl Buffer {
    pub fn push(&self, fields: Fields) {
        if let Ok(mut events) = self.0.lock() {
            if events.len() == MAX_EVENTS {
                events.pop_front();
            }
            events.push_back(fields);
        }
    }

    pub fn snapshot(&self) -> Vec<Fields> {
        self.0
            .lock()
            .map_or_else(|_| Vec::new(), |events| events.iter().cloned().collect())
    }

    pub fn take(&self) -> Vec<Fields> {
        self.0
            .lock()
            .map_or_else(|_| Vec::new(), |mut events| events.drain(..).collect())
    }
}
