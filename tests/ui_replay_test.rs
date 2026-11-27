#![cfg(test)]

//! Replay named UI actions through the same interaction code used by desktop windows.
#![allow(dead_code)] // Only part of the shared driver is needed for this scenario.
use xrack_testing as ui_driver;

use async_ui::AsyncDemo;
use search_completion::{Completion, completion, finished, property_write, request_span, started};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};
use trace_capture::{Capture, events_json};
use tracing_subscriber::{filter::filter_fn, prelude::*};
use ui_driver::UiDriver;
use xrack_interpreter::{Program, ReloadableInstance};
use xrack_layout::{Size, TextMeasurer};
use xrack_runtime::{
    Component, Value,
    ui_trace::{Buffer, TARGET},
    update_profile,
};

const UI: &str = include_str!("../examples/async_demo/async.xr");

#[derive(Deserialize)]
struct Recording {
    version: u8,
    #[serde(default)]
    truncated: bool,
    steps: Vec<Step>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceEdit {
    before: String,
    after: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Step {
    Click {
        id: String,
    },
    Fill {
        id: String,
        text: String,
    },
    ExpectSearches {
        queries: Vec<String>,
    },
    ExpectProperty {
        name: String,
        value: Json,
    },
    ReloadLayout {
        #[serde(default)]
        generation: Option<u64>,
        #[serde(default)]
        edits: Vec<SourceEdit>,
        #[serde(default)]
        row_height: Option<f32>,
    },
    CompleteSearch {
        #[serde(default)]
        request: Option<usize>,
        rows: Vec<String>,
    },
    ExpectResults {
        rows: Vec<String>,
    },
}

#[derive(Serialize)]
struct Frame {
    label: String,
    generation: u64,
    query: Json,
    loading: Json,
    result_query: Json,
    results: Vec<String>,
    visible_labels: Vec<String>,
    row_height: Option<f32>,
    update: Option<Json>,
    events: Json,
}

fn visible_labels(node: &xrack_runtime::Node, labels: &mut Vec<String>) {
    if let Some(text) = node.text("text") {
        labels.push(text.to_owned());
    }
    for (_, child) in node.materialized_children() {
        visible_labels(child, labels);
    }
}

fn capture_frame<C: SearchApp>(
    driver: &UiDriver<C, Metrics>,
    label: String,
    generation: u64,
) -> Frame {
    let component = driver.component();
    let list = driver.node("results-view");
    let mut labels = Vec::new();
    visible_labels(list, &mut labels);
    Frame {
        label,
        generation,
        query: component.property("query"),
        loading: component.property("loading"),
        result_query: component.property("result-query"),
        results: component.results(),
        visible_labels: labels,
        row_height: list.length("row-height"),
        update: None,
        events: json!([]),
    }
}

#[derive(Default)]
struct Metrics;
impl TextMeasurer for Metrics {
    fn measure(&mut self, text: &str, font_size: f32, _max_width: f32) -> Size {
        Size {
            width: text.chars().count() as f32 * font_size * 0.5,
            height: font_size,
        }
    }
}

struct Request {
    query: String,
    span: tracing::Span,
}

type Queries = Rc<RefCell<Vec<Request>>>;

fn dispatch_request(queries: &Queries, query: String, generation: u64) -> tracing::Span {
    let span = request_span(queries.borrow().len() + 1, generation);
    queries.borrow_mut().push(Request {
        query,
        span: span.clone(),
    });
    span
}

fn requested_queries(queries: &Queries) -> Vec<String> {
    queries
        .borrow()
        .iter()
        .map(|request| request.query.clone())
        .collect()
}

trait SearchApp: Component {
    const LIVE_RELOAD: bool;
    fn set_loading(&mut self, loading: bool);
    fn complete(&mut self, query: String, results: Vec<String>);
    fn property(&self, name: &str) -> Json;
    fn results(&self) -> Vec<String>;
    fn update_layout(&mut self, _revised: &str) {}
}

impl SearchApp for AsyncDemo {
    const LIVE_RELOAD: bool = false;

    fn set_loading(&mut self, loading: bool) {
        AsyncDemo::set_loading(self, loading);
    }

    fn complete(&mut self, query: String, results: Vec<String>) {
        property_write("suggestions", || {
            self.set_suggestions(results.iter().take(8).cloned().collect())
        });
        property_write("results", || self.set_results(results));
        property_write("result-query", || self.set_result_query(query));
    }

    fn property(&self, name: &str) -> Json {
        match name {
            "loading" => json!(self.get_loading()),
            "query" => json!(self.get_query()),
            "result-query" => json!(self.get_result_query()),
            _ => panic!("unsupported scenario property {name}"),
        }
    }

    fn results(&self) -> Vec<String> {
        self.model_results().snapshot()
    }
}

impl SearchApp for ReloadableInstance {
    const LIVE_RELOAD: bool = true;

    fn set_loading(&mut self, loading: bool) {
        self.current().set("loading", loading.into()).unwrap();
    }

    fn complete(&mut self, query: String, results: Vec<String>) {
        let current = self.current();
        let suggestions = results.iter().take(8).cloned().collect();
        property_write("suggestions", || {
            current
                .set("suggestions", Value::StringList(suggestions))
                .unwrap()
        });
        property_write("results", || {
            current.set("results", Value::StringList(results)).unwrap()
        });
        property_write("result-query", || {
            current.set("result-query", query.into()).unwrap()
        });
    }

    fn property(&self, name: &str) -> Json {
        match self.current().get(name).unwrap() {
            Value::Bool(value) => json!(value),
            Value::String(value) => json!(value),
            value => panic!("unsupported scenario property {name}: {value:?}"),
        }
    }

    fn results(&self) -> Vec<String> {
        self.current().string_model("results").unwrap().snapshot()
    }

    fn update_layout(&mut self, revised: &str) {
        self.reload(&program(revised)).unwrap();
    }
}

fn program(source: &str) -> Program {
    let preview = xrack_compiler::prepare_preview(source, "async.xr", Default::default()).unwrap();
    Program::from_module(&preview.module).unwrap()
}

fn interpreted() -> (ReloadableInstance, Queries) {
    let ui = ReloadableInstance::new(program(UI).create("AsyncDemo").unwrap());
    let weak = ui.as_weak();
    let queries: Queries = Rc::default();
    let output = queries.clone();
    ui.on(
        "search",
        Rc::new(move |args| {
            let [Value::String(query)] = args else {
                return Err("expected search query".into());
            };
            let Some(mut ui) = weak.upgrade() else {
                return Ok(None);
            };
            dispatch_request(&output, query.clone(), ui.source_generation()).in_scope(|| {
                started();
                property_write("loading", || ui.set_loading(true));
            });
            Ok(None)
        }),
    )
    .unwrap();
    (ui, queries)
}

fn compiled() -> (AsyncDemo, Queries) {
    let mut ui = AsyncDemo::new();
    let weak = ui.as_weak();
    let queries: Queries = Rc::default();
    let output = queries.clone();
    ui.on_search(move |query| {
        if let Some(mut ui) = weak.upgrade() {
            dispatch_request(&output, query, ui.source_generation()).in_scope(|| {
                started();
                property_write("loading", || ui.set_loading(true));
            });
        }
    });
    (ui, queries)
}

fn replay<C: SearchApp>(
    recording: &Recording,
    ui: C,
    queries: Queries,
    buffer: &Buffer,
) -> (Vec<String>, Vec<Frame>) {
    assert!(
        matches!(recording.version, 1 | 2),
        "unsupported XRack recording version"
    );
    assert!(
        !recording.truncated,
        "an incomplete recording cannot be replayed"
    );
    let mut driver = UiDriver::new(
        ui,
        Metrics,
        Size {
            width: 540.0,
            height: 350.0,
        },
    );
    let mut source_generation = 0;
    let mut source = UI.to_owned();
    let mut completed_requests = BTreeSet::new();
    let initial_height = driver.node("results-view").length("row-height");
    let mut frames = vec![capture_frame(&driver, "Initial state".into(), 0)];
    for (index, step) in recording.steps.iter().enumerate() {
        let number = index as u64 + 1;
        let generation = driver.component().source_generation();
        let request_span = match step {
            Step::CompleteSearch { request, .. } => {
                let requests = queries.borrow();
                request
                    .unwrap_or(requests.len())
                    .checked_sub(1)
                    .and_then(|index| requests.get(index))
                    .map(|request| request.span.clone())
            }
            _ => None,
        };
        let span = match request_span {
            Some(request) => {
                tracing::debug_span!(target: TARGET, parent: &request, "replay_step", step = number, generation)
            }
            None => tracing::debug_span!(target: TARGET, "replay_step", step = number, generation),
        };
        let mut label = String::new();
        let sample = span.in_scope(|| {
            update_profile::measure(|| {
                label = match step {
                    Step::Click { id } => {
                        driver.click(id);
                        format!("Click {id}")
                    }
                    Step::Fill { id, text } => {
                        if driver.focused() != Some(id.as_str()) {
                            driver.click(id);
                        }
                        assert_eq!(driver.focused(), Some(id.as_str()));
                        driver.fill_focused(text);
                        format!("Fill {id}")
                    }
                    Step::ExpectSearches { queries: expected } => {
                        assert_eq!(&requested_queries(&queries), expected);
                        "Check dispatched requests".into()
                    }
                    Step::ExpectProperty { name, value } => {
                        assert_eq!(&driver.component().property(name), value);
                        format!("Check {name}")
                    }
                    Step::ReloadLayout {
                        generation,
                        edits,
                        row_height,
                    } => {
                        source_generation += 1;
                        if let Some(generation) = generation {
                            assert_eq!(*generation, source_generation);
                        }
                        let expected_height = if recording.version == 1 {
                            source = UI.replace("row-height <- 30px;", "row-height <- 34px;");
                            Some(34.0)
                        } else {
                            apply_edits(&mut source, edits)
                                .expect("scene reload has valid source edits");
                            *row_height
                        };
                        driver.update(|ui| ui.update_layout(&source));
                        let expected_height = if C::LIVE_RELOAD {
                            expected_height
                        } else {
                            initial_height
                        };
                        if let Some(height) = expected_height {
                            assert_eq!(
                                driver.node("results-view").length("row-height"),
                                Some(height)
                            );
                        }
                        format!("Reload source revision {source_generation}")
                    }
                    Step::CompleteSearch { request, rows } => {
                        let latest = queries.borrow().len();
                        let request = request.unwrap_or_else(|| {
                            assert_eq!(
                                recording.version, 1,
                                "scene completions require a request number"
                            );
                            latest
                        });
                        assert!(
                            (1..=latest).contains(&request),
                            "request {request} was never dispatched"
                        );
                        assert!(
                            recording.version == 1 || completed_requests.insert(request),
                            "request {request} already completed"
                        );
                        let query = queries.borrow()[request - 1].query.clone();
                        let current = driver.component().property("query");
                        let decision =
                            completion(request, latest, &query, current.as_str().unwrap());
                        let reason = match decision {
                            Completion::Superseded => "superseded",
                            Completion::QueryChanged => {
                                driver.update(|ui| {
                                    property_write("loading", || ui.set_loading(false))
                                });
                                "query_changed"
                            }
                            Completion::Current => {
                                driver.update(|ui| {
                                    property_write("loading", || ui.set_loading(false));
                                    ui.complete(query.clone(), rows.clone());
                                });
                                "applied"
                            }
                        };
                        finished(reason, driver.component().source_generation());
                        format!("Complete request {request} ({query}) · {reason}")
                    }
                    Step::ExpectResults { rows } => {
                        assert_eq!(&driver.component().results(), rows);
                        let visible = driver.node("results-view");
                        assert_eq!(visible.list.map(|list| list.row_count), Some(rows.len()));
                        if let Some(first) = rows.first() {
                            let mut labels = Vec::new();
                            visible_labels(visible, &mut labels);
                            assert!(labels.contains(first), "result row was not rendered");
                        }
                        "Check results".into()
                    }
                };
            })
        });
        let mut frame = capture_frame(
            &driver,
            label,
            if C::LIVE_RELOAD { source_generation } else { 0 },
        );
        frame.update = sample.map(|sample| {
            json!({
                "evaluations": sample.property_evaluations,
                "cache_hits": sample.property_cache_hits,
                "layout_runs": sample.layout_runs,
                "materialized_rows": sample.materialized_rows,
                "binding_reads": sample.visual_binding_reads,
                "binding_reads_truncated": sample.visual_binding_reads_truncated,
            })
        });
        frame.events = events_json(buffer.take());
        frames.push(frame);
    }
    let actions = requested_queries(&queries);
    (actions, frames)
}

fn apply_edits(source: &mut String, edits: &[SourceEdit]) -> Result<(), &'static str> {
    if edits.is_empty() {
        return Err("a source revision must contain at least one edit");
    }
    let mut revised = source.clone();
    for edit in edits {
        if edit.before.is_empty() || revised.match_indices(&edit.before).count() != 1 {
            return Err("edit.before must identify exactly one source span");
        }
        revised = revised.replacen(&edit.before, &edit.after, 1);
    }
    *source = revised;
    Ok(())
}

fn verify(recording: &Recording) -> (Vec<Frame>, Vec<Frame>) {
    let buffer = Buffer::default();
    let subscriber = tracing_subscriber::registry().with(
        Capture(buffer.clone()).with_filter(filter_fn(|metadata| metadata.target() == TARGET)),
    );
    let ((calls, interpreter), (aot_calls, aot)) =
        tracing::subscriber::with_default(subscriber, || {
            let (ui, queries) = interpreted();
            let interpreter = replay(recording, ui, queries, &buffer);
            let (ui, queries) = compiled();
            let aot = replay(recording, ui, queries, &buffer);
            (interpreter, aot)
        });
    assert_eq!(calls, aot_calls);
    let state = |frame: &Frame| {
        json!([
            frame.query,
            frame.loading,
            frame.result_query,
            frame.results
        ])
    };
    for (index, (live, built)) in interpreter.iter().zip(&aot).enumerate() {
        assert_eq!(
            state(live),
            state(built),
            "backend state differs at step {index}"
        );
    }
    (interpreter, aot)
}

#[test]
fn search_completion_replays_on_both_backends() {
    let recording = serde_json::from_str(include_str!("async_search_replay.json")).unwrap();
    verify(&recording);
}

#[test]
fn request_trace_connects_result_writes_and_update_counts_across_reload() {
    let recording = serde_json::from_str(include_str!("async_search_scene.json")).unwrap();
    let (interpreter, aot) = verify(&recording);
    for (frames, generation) in [(interpreter, 1), (aot, 0)] {
        let applied = frames[6].events.as_array().unwrap();
        let complete = applied
            .iter()
            .find(|event| event["event"] == "search_finished")
            .unwrap();
        assert_eq!(complete["request_id"], 2);
        assert_eq!(complete["dispatched_generation"], 0);
        assert_eq!(complete["processed_generation"], generation);
        assert_eq!(complete["outcome"], "applied");
        assert!(applied.iter().any(|event| {
            event["event"] == "property_write"
                && event["request_id"] == 2
                && event["property"] == "results"
                && event["file"]
                    .as_str()
                    .is_some_and(|file| file.ends_with("ui_replay_test.rs"))
                && event["line"].as_u64().is_some_and(|line| line > 0)
        }));
        assert!(applied.iter().any(|event| event["event"] == "ui_update"
            && event["request_id"] == 2
            && event["layout_runs"] == 1));

        let dropped = frames[8].events.as_array().unwrap();
        assert!(
            dropped
                .iter()
                .any(|event| event["event"] == "search_finished"
                    && event["request_id"] == 1
                    && event["outcome"] == "superseded")
        );
        assert!(
            dropped
                .iter()
                .all(|event| event["event"] != "property_write")
        );
        assert!(dropped.iter().any(|event| event["event"] == "ui_update"
            && event["request_id"] == 1
            && event["layout_runs"] == 0));
    }
}

#[test]
fn replay_recorded_actions() {
    let data = std::env::var_os("XRACK_REPLAY_FILE").map_or_else(
        || include_str!("async_search_scene.json").to_owned(),
        |file| std::fs::read_to_string(file).unwrap(),
    );
    let recording = serde_json::from_str(&data).unwrap();
    let (interpreter, aot) = verify(&recording);
    if let Some(path) = std::env::var_os("XRACK_REPLAY_REPORT") {
        let trace = json!({ "interpreter": interpreter, "aot": aot });
        let data = trace.to_string().replace('<', "\\u003c");
        let report = include_str!("ui_replay_report.html").replacen("<!--XRACK_TRACE-->", &data, 1);
        std::fs::write(path, report).unwrap();
    }
}

#[test]
fn compiler_report_links_paint_only_binding_to_its_named_source_element() {
    let source = r#"
        export component ProfileDemo inherits Window {
            in-out property <color> accent: #112233;
            swatch := Rectangle {
                width <- 40px;
                height <- 40px;
                background <- root.accent;
            }
        }
    "#;
    let report = xrack_compiler::prepare_preview(source, "profile.xr", Default::default())
        .unwrap()
        .module
        .update_plan();
    assert_eq!(report["pipelineApplied"], true);
    let entries = report["entries"].as_array().unwrap();
    let background = entries
        .iter()
        .find(|entry| entry["element"] == "swatch" && entry["property"] == "background")
        .expect("compiler report includes the named visual binding");
    assert_eq!(background["decisions"]["updateEffect"], "paint");
    assert_eq!(background["source"]["file"], "profile.xr");
    assert_eq!(background["source"]["line"], 7);
}

#[test]
fn profile_counts_only_materialized_results_from_large_models() {
    fn measure<C: SearchApp>(ui: C) {
        let mut driver = UiDriver::new(ui, Metrics, Size::new(540.0, 350.0));
        let rows = (0..10_000).map(|index| format!("row {index}")).collect();
        let sample = xrack_runtime::update_profile::measure(|| {
            driver.update(|ui| ui.complete("cloud".into(), rows));
        })
        .unwrap();
        assert_eq!(driver.node("results-view").list.unwrap().row_count, 10_000);
        let (_, rows) = sample
            .view_rows
            .iter()
            .find(|(id, _)| id == "results-view")
            .expect("the sample identifies its list view");
        assert!((1..100).contains(rows), "materialized {rows} rows");
        assert_eq!(*rows, sample.materialized_rows);
    }
    measure(interpreted().0);
    measure(compiled().0);
}
