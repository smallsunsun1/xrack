use xrack_runtime::ui_trace::TARGET;

/// Applies the same completion rules in the live demo and the replay fixture.
pub enum Completion {
    Superseded,
    QueryChanged,
    Current,
}

pub fn completion(request: usize, latest: usize, query: &str, current_query: &str) -> Completion {
    if request != latest {
        Completion::Superseded
    } else if query != current_query {
        Completion::QueryChanged
    } else {
        Completion::Current
    }
}
pub fn request_span(request: usize, generation: u64) -> tracing::Span {
    tracing::debug_span!(
        target: TARGET,
        "search",
        request_id = request as u64,
        dispatched_generation = generation,
    )
}

pub fn started() {
    tracing::debug!(target: TARGET, event = "search_started");
}

pub fn finished(outcome: &str, generation: u64) {
    tracing::debug!(
        target: TARGET,
        event = "search_finished",
        outcome,
        processed_generation = generation,
    );
}

#[track_caller]
pub fn property_write(property: &str, write: impl FnOnce()) {
    write();
    property_written(property);
}

#[track_caller]
pub fn property_written(property: &str) {
    let location = std::panic::Location::caller();
    tracing::debug!(
        target: TARGET,
        event = "property_write",
        property,
        file = location.file(),
        line = u64::from(location.line()),
    );
}
