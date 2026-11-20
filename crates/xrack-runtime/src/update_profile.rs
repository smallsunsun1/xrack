//! Counters for an opt-in native UI update on the current thread.
use std::{cell::RefCell, time::Instant};

const MAX_VIEWS: usize = 24;
const MAX_BINDINGS: usize = 48;

#[derive(Clone, Debug, Default)]
pub struct UpdateSample {
    pub property_evaluations: usize,
    pub property_cache_hits: usize,
    pub layout_runs: usize,
    pub layout_reuses: usize,
    pub materialized_rows: usize,
    pub cpu_micros: u64,
    pub view_rows: Vec<(String, usize)>,
    pub visual_binding_reads: Vec<(String, &'static str, usize)>,
    pub visual_binding_reads_truncated: bool,
}

thread_local! {
    static CURRENT: RefCell<Option<UpdateSample>> = const { RefCell::new(None) };
}

pub fn measure(update: impl FnOnce()) -> Option<UpdateSample> {
    CURRENT.with(|current| {
        if current.borrow().is_some() {
            update();
            return None;
        }
        struct Clear<'a>(&'a RefCell<Option<UpdateSample>>);
        impl Drop for Clear<'_> {
            fn drop(&mut self) {
                self.0.replace(None);
            }
        }
        let _clear = Clear(current);
        current.replace(Some(UpdateSample::default()));
        let start = Instant::now();
        update();
        let mut sample = current.take()?;
        sample.cpu_micros = u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX);
        tracing::debug!(
            target: crate::ui_trace::TARGET,
            event = "ui_update",
            evaluations = sample.property_evaluations as u64,
            cache_hits = sample.property_cache_hits as u64,
            layout_runs = sample.layout_runs as u64,
            layout_reuses = sample.layout_reuses as u64,
            materialized_rows = sample.materialized_rows as u64,
            cpu_micros = sample.cpu_micros,
        );
        Some(sample)
    })
}

#[inline]
fn count(update: impl FnOnce(&mut UpdateSample)) {
    CURRENT.with(|current| {
        if let Some(sample) = current.borrow_mut().as_mut() {
            update(sample);
        }
    });
}

pub(crate) fn property(evaluated: bool) {
    count(|sample| {
        if evaluated {
            sample.property_evaluations += 1;
        } else {
            sample.property_cache_hits += 1;
        }
    });
}

pub fn layout(reused: bool) {
    count(|sample| {
        if reused {
            sample.layout_reuses += 1;
        } else {
            sample.layout_runs += 1;
        }
    });
}

pub fn rows(view: Option<&str>, counted: usize) {
    count(|sample| {
        sample.materialized_rows += counted;
        if let Some(view) = view {
            if let Some((_, rows)) = sample.view_rows.iter_mut().find(|(name, _)| name == view) {
                *rows += counted;
            } else if sample.view_rows.len() < MAX_VIEWS {
                sample.view_rows.push((view.to_owned(), counted));
            }
        }
    });
}

pub fn visual_binding(element: &str, property: &'static str) {
    if element.is_empty() {
        return;
    }
    count(|sample| {
        let entries = &mut sample.visual_binding_reads;
        if let Some((_, _, reads)) = entries
            .iter_mut()
            .find(|(id, name, _)| id == element && *name == property)
        {
            *reads += 1;
        } else if entries.len() < MAX_BINDINGS {
            entries.push((element.to_owned(), property, 1));
        } else {
            sample.visual_binding_reads_truncated = true;
        }
    });
}
