use crate::{
    Error, Function, Value,
    live_inspect::{Source, describe_value, shortened},
};
use serde_json::{Value as Json, json};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    time::Instant,
};

const MAX_FRAMES: usize = 96;
const MAX_BYTES: usize = 192 * 1024;
const MAX_PARAMETERS: usize = 12;

pub(crate) struct Trace {
    epoch: Cell<u64>,
    next_id: Cell<u64>,
    active: Cell<Option<u64>>,
    frames: RefCell<VecDeque<(usize, Json)>>,
    bytes: Cell<usize>,
    values: bool,
}

impl Trace {
    pub(crate) fn new(values: bool) -> Self {
        Self {
            epoch: Cell::default(),
            next_id: Cell::new(1),
            active: Cell::default(),
            frames: RefCell::default(),
            bytes: Cell::default(),
            values,
        }
    }

    pub(crate) fn captures_values(&self) -> bool {
        self.values
    }

    pub(crate) fn generation(&self) -> u64 {
        self.epoch.get()
    }

    pub(crate) fn reloaded(&self) {
        self.epoch.set(self.epoch.get() + 1);
    }

    pub(crate) fn snapshot(&self) -> Json {
        json!({
            "generation": self.epoch.get(),
            "captureValues": self.values,
            "frames": self.frames.borrow().iter().map(|(_, frame)| frame).collect::<Vec<_>>(),
        })
    }

    pub(crate) fn record(
        &self,
        component: &str,
        function: &Function,
        call_site: Option<&Source>,
        args: &[Value],
        execute: impl FnOnce() -> Result<Option<Value>, Error>,
    ) -> Result<Option<Value>, Error> {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let generation = self.epoch.get();
        let parent = self.active.replace(Some(id));
        struct Restore<'a>(&'a Cell<Option<u64>>, Option<u64>);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                self.0.set(self.1);
            }
        }
        let restore = Restore(&self.active, parent);
        let parameters = function
            .parameter_names
            .iter()
            .zip(&function.parameters)
            .zip(args)
            .take(MAX_PARAMETERS)
            .map(|((name, kind), value)| {
                let mut parameter = json!({"name": name, "kind": format!("{kind:?}")});
                if self.values {
                    describe_value(&mut parameter, value, 3);
                }
                parameter
            })
            .collect::<Vec<_>>();
        let start = Instant::now();
        let result = execute();
        drop(restore);
        let mut frame = json!({
            "id": id,
            "parentId": parent,
            "generation": generation,
            "component": component,
            "name": function.name,
            "source": function.location,
            "callSite": call_site,
            "parameters": parameters,
            "parametersTruncated": args.len() > MAX_PARAMETERS,
            "elapsedMicros": start.elapsed().as_micros() as u64,
            "status": if result.is_ok() { "returned" } else { "error" },
        });
        if self.values
            && let Ok(Some(value)) = &result
        {
            let mut returned = json!({});
            if let Some(kind) = &function.result {
                returned["kind"] = json!(format!("{kind:?}"));
            }
            describe_value(&mut returned, value, 3);
            frame["result"] = returned;
        }
        if self.values
            && let Err(error) = &result
        {
            frame["error"] = json!(shortened(&error.to_string()));
        }
        let bytes = frame.to_string().len();
        let mut frames = self.frames.borrow_mut();
        frames.push_back((bytes, frame));
        let mut total = self.bytes.get() + bytes;
        while frames.len() > MAX_FRAMES || total > MAX_BYTES {
            let Some((bytes, _)) = frames.pop_front() else {
                break;
            };
            total -= bytes;
        }
        self.bytes.set(total);
        result
    }
}
