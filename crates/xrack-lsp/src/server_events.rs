use serde_json::Value;
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
};

pub(crate) type Reply = Result<Value, (i32, &'static str)>;
pub(crate) const CANCELLED: (i32, &str) = (-32800, "Request cancelled");

#[derive(Clone, Default)]
pub(crate) struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub(crate) fn same_request(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

pub(crate) enum IndexProgress {
    Begin,
    Report { done: usize, total: usize },
    End,
}

pub(crate) enum Event {
    Input(io::Result<Option<Value>>),
    Reply {
        id: Value,
        cancellation: Cancellation,
        reply: Reply,
    },
    Progress(u64, IndexProgress),
}

pub(crate) fn read_input(mut input: impl io::BufRead, events: Sender<Event>) {
    loop {
        let message = crate::protocol::read_message(&mut input);
        let end = !matches!(&message, Ok(Some(_)));
        if events.send(Event::Input(message)).is_err() || end {
            break;
        }
    }
}
