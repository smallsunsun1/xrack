#![cfg(test)]

//! Replay named UI actions through the same interaction code used by desktop windows.
#![allow(dead_code)] // Only part of the shared driver is needed for this scenario.
mod ui_driver;

use async_ui::AsyncDemo;
use serde::Deserialize;
use serde_json::{Value as Json, json};
use std::{cell::RefCell, rc::Rc};
use ui_driver::UiDriver;
use xrack_interpreter::{Program, ReloadableInstance};
use xrack_layout::{Size, TextMeasurer};
use xrack_runtime::{Component, Value};

const UI: &str = include_str!("../examples/async_demo/async.xr");

#[derive(Deserialize)]
struct Recording {
    version: u8,
    #[serde(default)]
    truncated: bool,
    steps: Vec<Step>,
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
    },
    CompleteSearch {
        rows: Vec<String>,
    },
    ExpectResults {
        rows: Vec<String>,
    },
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

type Queries = Rc<RefCell<Vec<String>>>;

trait SearchApp: Component {
    fn complete(&mut self, query: String, results: Vec<String>);
    fn property(&self, name: &str) -> Json;
    fn results(&self) -> Vec<String>;
    fn update_layout(&mut self, _revised: &str) {}
    fn expected_row_height() -> f32;
}

impl SearchApp for AsyncDemo {
    fn complete(&mut self, query: String, results: Vec<String>) {
        self.set_loading(false);
        self.set_result_query(query);
        self.set_results(results);
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

    fn expected_row_height() -> f32 {
        30.0
    }
}

impl SearchApp for ReloadableInstance {
    fn complete(&mut self, query: String, results: Vec<String>) {
        let current = self.current();
        current.set("loading", false.into()).unwrap();
        current.set("result-query", query.into()).unwrap();
        current.set("results", Value::StringList(results)).unwrap();
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

    fn expected_row_height() -> f32 {
        34.0
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
            let Some(ui) = weak.upgrade() else {
                return Ok(None);
            };
            output.borrow_mut().push(query.clone());
            ui.current()
                .set("loading", true.into())
                .map_err(|error| error.to_string())?;
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
        output.borrow_mut().push(query);
        if let Some(mut ui) = weak.upgrade() {
            ui.set_loading(true);
        }
    });
    (ui, queries)
}

fn replay<C: SearchApp>(recording: &Recording, ui: C, queries: Queries) -> Vec<String> {
    assert_eq!(recording.version, 1, "unsupported XRack recording version");
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
    for step in &recording.steps {
        match step {
            Step::Click { id } => driver.click(id),
            Step::Fill { id, text } => {
                if driver.focused() != Some(id.as_str()) {
                    driver.click(id);
                }
                assert_eq!(driver.focused(), Some(id.as_str()));
                driver.fill_focused(text);
            }
            Step::ExpectSearches { queries: expected } => assert_eq!(&*queries.borrow(), expected),
            Step::ExpectProperty { name, value } => {
                assert_eq!(&driver.component().property(name), value)
            }
            Step::ReloadLayout { generation } => {
                source_generation += 1;
                if let Some(generation) = generation {
                    assert_eq!(*generation, source_generation);
                }
                let revised = UI.replace("row-height: 30px;", "row-height: 34px;");
                driver.update(|ui| ui.update_layout(&revised));
                assert_eq!(
                    driver.node("results-view").length("row-height"),
                    Some(C::expected_row_height())
                );
            }
            Step::CompleteSearch { rows } => {
                let query = queries
                    .borrow()
                    .last()
                    .expect("search must start before it can complete")
                    .clone();
                driver.update(|ui| ui.complete(query, rows.clone()));
            }
            Step::ExpectResults { rows } => {
                assert_eq!(&driver.component().results(), rows);
                let visible = driver.node("results-view");
                assert_eq!(visible.list.map(|list| list.row_count), Some(rows.len()));
                if let Some(first) = rows.first() {
                    fn contains(node: &xrack_runtime::Node, text: &str) -> bool {
                        node.text("text") == Some(text)
                            || node
                                .materialized_children()
                                .any(|(_, child)| contains(child, text))
                    }
                    assert!(contains(visible, first), "result row was not rendered");
                }
            }
        }
    }
    let actions = queries.borrow().clone();
    actions
}

#[test]
fn search_completion_replays_on_both_backends() {
    let recording = serde_json::from_str(include_str!("async_search_replay.json")).unwrap();
    let (ui, queries) = interpreted();
    let actual = replay(&recording, ui, queries);
    let (ui, queries) = compiled();
    assert_eq!(actual, replay(&recording, ui, queries));
}

#[test]
fn replay_recorded_actions() {
    let data = std::env::var_os("XRACK_REPLAY_FILE").map_or_else(
        || r#"{"version":1,"steps":[{"type":"click","id":"submit"}]}"#.to_owned(),
        |file| std::fs::read_to_string(file).unwrap(),
    );
    let recording = serde_json::from_str(&data).unwrap();
    let (ui, queries) = interpreted();
    let actual = replay(&recording, ui, queries);
    let (ui, queries) = compiled();
    assert_eq!(actual, replay(&recording, ui, queries));
}

#[test]
fn compiler_report_links_paint_only_binding_to_its_named_source_element() {
    let source = r#"
        export component ProfileDemo inherits Window {
            in-out property <color> accent: #112233;
            swatch := Rectangle {
                width: 40px;
                height: 40px;
                background: root.accent;
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
