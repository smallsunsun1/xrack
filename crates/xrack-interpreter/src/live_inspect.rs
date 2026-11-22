use crate::{
    Instance, Node, PropertySource, ReloadableInstance, Value, Visual, live::SavedSources,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
};
use xrack_mlir::OperationRef;
use xrack_runtime::WindowInspector;

const MAX_ROWS: usize = 12;
const MAX_STRING: usize = 256;
const MAX_NODES: usize = 128;
const MAX_DEPTH: usize = 24;
const MAX_ELEMENT_PROPERTIES: usize = 32;

pub(crate) type SourceVersions = BTreeMap<PathBuf, u32>;

pub(crate) fn source_versions(sources: &SavedSources) -> SourceVersions {
    sources
        .iter()
        .filter_map(|(path, contents)| {
            let text = contents.as_ref().ok()?;
            // FNV-1a over UTF-8, also computed on the document before a definition jump.
            let version = text.bytes().fold(0x811c9dc5u32, |hash, byte| {
                (hash ^ u32::from(byte)).wrapping_mul(0x01000193)
            });
            Some((path.clone(), version))
        })
        .collect()
}

#[derive(Clone, Serialize)]
pub(crate) struct Source {
    file: String,
    line: usize,
    column: usize,
}

impl Source {
    pub(crate) fn from_operation(op: OperationRef<'_>) -> Option<Self> {
        let file = op.source_file()?;
        if !Path::new(file).is_absolute()
            || !Path::new(file).extension().is_some_and(|ext| ext == "xr")
        {
            return None;
        }
        let span = op.span();
        Some(Self {
            file: file.into(),
            line: span.line,
            column: span.column,
        })
    }
}

#[derive(Serialize)]
struct Entry<'a> {
    key: String,
    #[serde(skip)]
    path: Vec<usize>,
    parent: Option<usize>,
    kind: String,
    name: Option<String>,
    source: Option<&'a Source>,
    #[serde(rename = "componentSource")]
    component_source: Option<Json>,
    #[serde(rename = "componentProperties")]
    component_properties: Vec<Json>,
    #[serde(skip)]
    component: Option<Instance>,
    properties: Vec<Json>,
}

#[derive(Default, Serialize)]
struct Tree<'a> {
    nodes: Vec<Entry<'a>>,
    truncated: bool,
    #[serde(skip)]
    details: bool,
    #[serde(skip)]
    retain_components: bool,
}

impl<'a> Tree<'a> {
    fn collect(
        &mut self,
        instance: &'a Instance,
        scene: &Node,
        node: &Node,
        parent: Option<usize>,
        scope: &str,
        path: &mut Vec<usize>,
    ) {
        if self.nodes.len() >= MAX_NODES || path.len() >= MAX_DEPTH {
            self.truncated = true;
            return;
        }
        if node.kind == "Empty" && !node.is_visible() {
            return;
        }
        let name = node.element_id();
        let target = instance.inspect_target(scene, path).filter(|(_, element)| {
            let same_id = element.id.as_ref() == name.unwrap_or_default();
            same_id
                && match &element.visual {
                    Visual::Builtin(kind) => {
                        xrack_resources::visual::scene_element(kind).0 == node.kind
                    }
                    Visual::Instance { .. } => true,
                }
        });
        let component = target.as_ref().and_then(|(owner, element)| {
            matches!(element.visual, Visual::Instance { .. })
                .then(|| owner.children().get(&element.key).cloned())
                .flatten()
        });
        let is_root = parent.is_none();
        let source = if is_root {
            instance.definition().source.as_ref()
        } else {
            target
                .as_ref()
                .and_then(|(_, element)| element.source.as_ref())
        };
        let parent_key = parent.map_or(scope, |index| &self.nodes[index].key);
        let key = match (parent, name, node.row_key) {
            (None, _, _) => "root".to_owned(),
            (_, Some(name), None) => format!("{scope}/#{name}"),
            (_, _, Some(row)) => format!("{parent_key}/{row:?}"),
            _ => format!(
                "{parent_key}/{}:{}",
                node.kind,
                path.last().copied().unwrap_or_default()
            ),
        };
        let kind = target
            .as_ref()
            .map_or(node.kind, |(_, element)| match &element.visual {
                Visual::Instance { component, .. } => component,
                Visual::Builtin(kind) => kind,
            });
        let component_source = component
            .as_ref()
            .and_then(|child| child.definition().source.as_ref())
            .map(|source| json!(source));
        let component_properties = component
            .as_ref()
            .filter(|_| self.details)
            .map(|child| public_properties(child, MAX_ELEMENT_PROPERTIES))
            .unwrap_or_default();
        let child_scope = if component.is_some() || node.row_key.is_some() {
            key.clone()
        } else {
            scope.to_owned()
        };
        let index = self.nodes.len();
        self.nodes.push(Entry {
            key,
            path: path.clone(),
            parent,
            kind: shortened(kind),
            name: name.map(shortened),
            source,
            component_source,
            component_properties,
            component: if self.retain_components {
                component
            } else {
                None
            },
            properties: if self.details {
                element_properties(node)
            } else {
                Vec::new()
            },
        });
        for (slot, child) in node.materialized_children() {
            if self.nodes.len() >= MAX_NODES {
                self.truncated = true;
                break;
            }
            path.push(slot);
            self.collect(instance, scene, child, Some(index), &child_scope, path);
            path.pop();
        }
    }
}

pub(crate) fn sync_targets(instance: &Instance, scene: &Rc<Node>, inspector: &WindowInspector) {
    let mut tree = Tree::default();
    tree.collect(instance, scene, scene, None, "root", &mut Vec::new());
    inspector.set_targets(
        scene,
        tree.nodes
            .into_iter()
            .map(|node| (node.key, node.path))
            .collect(),
    );
}

pub(crate) fn snapshot(ui: &ReloadableInstance, versions: SourceVersions) -> Json {
    let instance = ui.current();
    let mut payload = json!({"version": 1, "properties": public_properties(&instance, 128)});
    payload["sourceVersions"] = json!(versions);
    if let Some(trace) = instance.trace() {
        payload["trace"] = trace.snapshot();
    }
    let inspector = ui.inspector();
    if let Some(events) = inspector.trace_events() {
        payload["diagnostics"] = Json::Array(
            events
                .into_iter()
                .map(|event| {
                    Json::Object(
                        event
                            .into_iter()
                            .map(|(key, value)| {
                                let value = match value {
                                    xrack_runtime::ui_trace::Field::Number(value) => json!(value),
                                    xrack_runtime::ui_trace::Field::Text(value) => json!(value),
                                };
                                (key.into(), value)
                            })
                            .collect(),
                    )
                })
                .collect(),
        );
    }
    if let Some((actions, truncated, text_capture)) = inspector.recording() {
        let steps: Vec<_> = actions
            .into_iter()
            .map(|action| match action {
                xrack_runtime::RecordedAction::Click { id } => json!({"type": "click", "id": id}),
                xrack_runtime::RecordedAction::Fill { id, text } => {
                    json!({"type": "fill", "id": id, "text": text})
                }
                xrack_runtime::RecordedAction::Reload { generation } => {
                    json!({"type": "reload_layout", "generation": generation})
                }
            })
            .collect();
        payload["recording"] = json!({
            "version": 1, "steps": steps, "truncated": truncated, "textCapture": text_capture,
        });
    }
    if let Some(samples) = inspector.samples() {
        payload["updatePlan"] = instance.program().update_plan.as_ref().clone();
        payload["updates"] = json!(samples.into_iter().map(|(id, generation, sample)| json!({
            "id": id, "generation": generation, "propertyEvaluations": sample.property_evaluations,
            "propertyCacheHits": sample.property_cache_hits, "layoutRuns": sample.layout_runs,
            "layoutReuses": sample.layout_reuses, "materializedRows": sample.materialized_rows,
            "cpuMicros": sample.cpu_micros,
            "modelRowsScanned": sample.model_rows_scanned,
            "modelRowsCloned": sample.model_rows_cloned,
            "viewRows": sample.view_rows.into_iter().map(|(id, rows)| json!({"id": id, "rows": rows})).collect::<Vec<_>>(),
            "visualBindingReads": sample.visual_binding_reads.into_iter().map(|(id, property, reads)| json!({
                "id": id, "property": property, "reads": reads,
            })).collect::<Vec<_>>(),
            "visualBindingReadsTruncated": sample.visual_binding_reads_truncated,
        })).collect::<Vec<_>>());
        payload["renderSamples"] = json!(inspector.render_samples().into_iter().map(|render| {
            let sample = render.update;
            let changes = render.scene;
            json!({
            "frame": render.frame, "generation": render.generation, "cpuMicros": sample.cpu_micros,
            "preparedRevision": render.prepared_revision,
            "inputToPresentSubmitMicros": render.input_to_submit_micros,
            "primitiveDispatches": sample.draw_primitives, "quadBatches": sample.quad_batches,
            "glyphBatches": sample.glyph_batches, "queueSubmissions": sample.gpu_submissions,
            "batchedVertexBytes": sample.vertex_bytes, "imageUploadBytes": sample.texture_upload_bytes,
            "geometryUploadBytes": sample.geometry_upload_bytes, "geometryReuses": sample.geometry_reuses,
            "sceneChanges": {
                "inserted": changes.inserted, "removed": changes.removed, "moved": changes.moved,
                "pooledRows": changes.pooled_rows, "reusedRows": changes.reused_rows,
                "measure": changes.measure, "transform": changes.transform, "clip": changes.clip,
                "paint": changes.paint, "input": changes.input,
            },
        })}).collect::<Vec<_>>());
        payload["presentedFrames"] = json!(inspector.presentations().into_iter().map(|sample| json!({
            "frame": sample.frame, "generation": sample.generation, "hostMicros": sample.host_micros,
            "inputToPresentedMicros": sample.input_to_present_micros,
        })).collect::<Vec<_>>());
    }
    match instance.try_render() {
        Ok(scene) => {
            let mut tree = Tree {
                details: true,
                ..Tree::default()
            };
            tree.collect(&instance, &scene, &scene, None, "root", &mut Vec::new());
            let inspector = ui.inspector();
            if inspector.attached() {
                if !inspector.matches_scene(&scene) {
                    inspector.set_targets(
                        &scene,
                        tree.nodes
                            .iter()
                            .map(|node| (node.key.clone(), node.path.clone()))
                            .collect(),
                    );
                }
                let (key, pick_revision) = inspector.selection();
                payload["inspection"] = json!({
                    "key": key,
                    "pickRevision": pick_revision,
                    "picking": inspector.picking(),
                });
            }
            payload["tree"] = json!(tree);
        }
        Err(error) => payload["treeError"] = json!(shortened(&error.to_string())),
    }
    payload
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ModelQuery {
    node_key: String,
    property: String,
    offset: usize,
    limit: usize,
    generation: Option<u64>,
}

pub(crate) fn model_page(
    ui: &ReloadableInstance,
    query: &ModelQuery,
    versions: SourceVersions,
) -> Json {
    let root = ui.current();
    let generation = root.trace().map(|trace| trace.generation());
    if query.generation.is_some() && query.generation != generation {
        return json!({"error": "Source generation changed. Resolve the component node again."});
    }
    if !(1..=24).contains(&query.limit) {
        return json!({"error": "Model query limit must be between 1 and 24"});
    }
    let instance = if query.node_key == "root" {
        root
    } else {
        let scene = match root.try_render() {
            Ok(scene) => scene,
            Err(error) => return json!({"error": shortened(&error.to_string())}),
        };
        let mut tree = Tree {
            retain_components: true,
            ..Tree::default()
        };
        tree.collect(&root, &scene, &scene, None, "root", &mut Vec::new());
        let Some(instance) = tree
            .nodes
            .into_iter()
            .find(|node| node.key == query.node_key)
            .and_then(|node| node.component)
        else {
            return json!({"error": "Component instance is absent from the inspected tree"});
        };
        instance
    };
    let value = match instance.read_public_value(&query.property) {
        Ok(value) => value,
        Err(error) => return json!({"error": shortened(&error.to_string())}),
    };
    let (kind, count, items, revision) = match value {
        Value::IntList(rows) => {
            let items = rows
                .iter()
                .skip(query.offset)
                .take(query.limit)
                .map(|number| json!(number.to_string()))
                .collect();
            ("IntList", rows.len(), items, None)
        }
        Value::IntModel(model) => {
            let (count, items) = model.inspect_rows(query.offset, query.limit, |number| {
                json!(number.to_string())
            });
            ("IntList", count, items, Some(model.edit_stamp()))
        }
        Value::DateList(rows) => {
            let items = rows
                .iter()
                .skip(query.offset)
                .take(query.limit)
                .map(|date| json!(date.to_string()))
                .collect();
            ("DateList", rows.len(), items, None)
        }
        Value::DateModel(model) => {
            let (count, items) =
                model.inspect_rows(query.offset, query.limit, |date| json!(date.to_string()));
            ("DateList", count, items, Some(model.edit_stamp()))
        }
        Value::StringList(rows) => {
            let items = rows
                .iter()
                .skip(query.offset)
                .take(query.limit)
                .map(|text| json!(shortened(text)))
                .collect();
            ("StringList", rows.len(), items, None)
        }
        Value::StringModel(model) => {
            let (count, items) =
                model.inspect_rows(query.offset, query.limit, |text| json!(shortened(text)));
            ("StringList", count, items, Some(model.edit_stamp()))
        }
        Value::RecordList(list) => {
            let model = list.model();
            let (count, items) = model.inspect_rows(query.offset, query.limit, |record| {
                json!(record_fields(record, 1))
            });
            ("RecordList", count, items, Some(model.edit_stamp()))
        }
        _ => return json!({"error": "The property is not an int, string, or record model"}),
    };
    let end = query.offset + items.len();
    json!({
        "nodeKey": query.node_key, "property": query.property,
        "kind": kind, "count": count, "items": items, "offset": query.offset,
        "nextOffset": (end < count).then_some(end), "generation": generation,
        "modelRevision": revision.map(|(id, revision)| [id.to_string(), revision.to_string()]),
        "sourceVersions": versions,
    })
}

fn element_properties(node: &Node) -> Vec<Json> {
    let mut properties = vec![
        json!({"name": "visible", "kind": "Bool", "value": node.is_visible()}),
        json!({"name": "enabled", "kind": "Bool", "value": node.boolean("enabled", true)}),
    ];
    if let Some(list) = node.list {
        properties.push(json!({"name": "row-count", "kind": "Int", "value": list.row_count}));
    }
    properties.extend(
        node.properties
            .iter()
            .filter_map(|(&name, value)| {
                if matches!(name, "visible" | "enabled") || name.starts_with("__") {
                    return None;
                }
                if node.is_password() && matches!(name, "text" | "selected-text" | "display-text") {
                    return Some(json!({"name": name, "kind": "String", "value": "••••"}));
                }
                let mut property = json!({"name": name});
                describe_value(&mut property, value, 2);
                Some(property)
            })
            .take(MAX_ELEMENT_PROPERTIES),
    );
    properties
}

pub(crate) fn shortened(value: &str) -> String {
    let mut chars = value.chars();
    let mut result: String = chars.by_ref().take(MAX_STRING).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

fn public_properties(instance: &Instance, limit: usize) -> Vec<Json> {
    instance
        .definition()
        .properties
        .iter()
        .filter(|(member, definition)| member.owner == "root" && definition.public)
        .chain(
            instance
                .definition()
                .properties
                .iter()
                .filter(|(member, definition)| member.owner == "root" && !definition.public),
        )
        .take(limit)
        .map(|(member, definition)| {
            let name = &member.name;
            let mut property = json!({
                "name": name, "kind": format!("{:?}", definition.ty),
                "visibility": definition.visibility, "writable": definition.writable,
                "source": definition.location,
            });
            if let PropertySource::Alias(target) = &definition.source {
                property["alias"] = json!(format!("{}.{}", target.owner, target.name));
            }
            if definition.public {
                match instance.read_public_value(name) {
                    Ok(value) => describe_value(&mut property, &value, 3),
                    Err(error) => property["error"] = json!(shortened(&error.to_string())),
                }
            } else if instance
                .trace()
                .is_some_and(|trace| trace.captures_values())
            {
                match instance.cached_property(member) {
                    Some(value) => describe_value(&mut property, &value, 3),
                    None => property["status"] = json!("No cached value"),
                }
            } else {
                property["status"] = json!("Private value capture is disabled");
            }
            property
        })
        .collect()
}

pub(crate) fn describe_value(property: &mut Json, value: &Value, depth: usize) {
    describe(value, property, depth, &mut 24);
}

fn describe(value: &Value, property: &mut Json, depth: usize, remaining: &mut usize) {
    let (kind, value) = match value {
        Value::Bool(value) => ("Bool", json!(value)),
        Value::Int(value) => ("Int", json!(value.to_string())),
        Value::Float(value) => ("Float", json!(value.to_string())),
        Value::String(value) => ("String", json!(shortened(value))),
        Value::Length(value) => ("Length", json!(format!("{}px", value.0))),
        Value::Point2D(value) => ("Point2D", json!({"x": value.x.0, "y": value.y.0})),
        Value::Vector2D(value) => ("Vector2D", json!({"x": value.x.0, "y": value.y.0})),
        Value::Transform2D(value) => ("Transform2D", json!(value.coefficients())),
        Value::Percent(value) => ("Percent", json!(format!("{}%", value.0 * 100.0))),
        Value::Color(value) => ("Color", json!(format!("#{:08x}", value.0))),
        Value::IntList(items) if depth > 0 => {
            let sample: Vec<_> = items.iter().take(MAX_ROWS).map(i64::to_string).collect();
            set_items(property, "IntList", items.len(), sample);
            return;
        }
        Value::IntModel(model) if depth > 0 => {
            let (count, sample) = model.inspect_rows(0, MAX_ROWS, i64::to_string);
            set_items(property, "IntList", count, sample);
            return;
        }
        Value::DateList(items) if depth > 0 => {
            let sample: Vec<_> = items
                .iter()
                .take(MAX_ROWS)
                .map(ToString::to_string)
                .collect();
            set_items(property, "DateList", items.len(), sample);
            return;
        }
        Value::DateModel(model) if depth > 0 => {
            let (count, sample) = model.inspect_rows(0, MAX_ROWS, ToString::to_string);
            set_items(property, "DateList", count, sample);
            return;
        }
        Value::StringList(items) if depth > 0 => {
            let sample: Vec<_> = items
                .iter()
                .take(MAX_ROWS)
                .map(|item| shortened(item))
                .collect();
            set_items(property, "StringList", items.len(), sample);
            return;
        }
        Value::StringModel(model) if depth > 0 => {
            let (count, sample) = model.inspect_rows(0, MAX_ROWS, |item| shortened(item));
            set_items(property, "StringList", count, sample);
            return;
        }
        Value::RecordList(list) if depth > 0 => {
            let (count, sample) = list
                .model()
                .inspect_rows(0, 4, |item| sample_fields(item, depth - 1, remaining));
            set_items(property, "RecordList", count, sample);
            return;
        }
        Value::Record(record) if depth > 0 => {
            if property["kind"].is_null() {
                property["kind"] = json!(record.name());
            }
            let fields = sample_fields(record, depth - 1, remaining);
            if record.fields().len() > fields.len() {
                property["truncated"] = json!(true);
            }
            property["fields"] = json!(fields);
            return;
        }
        Value::StringList(items) => ("StringList", json!(format!("{} rows", items.len()))),
        Value::IntList(items) => ("IntList", json!(format!("{} rows", items.len()))),
        Value::DateList(items) => ("DateList", json!(format!("{} rows", items.len()))),
        Value::DateModel(model) => (
            "DateList",
            json!(format!("{} rows", model.inspect_rows(0, 0, |_| ()).0)),
        ),
        Value::IntModel(model) => (
            "IntList",
            json!(format!("{} rows", model.inspect_rows(0, 0, |_| ()).0)),
        ),
        Value::StringModel(model) => (
            "StringList",
            json!(format!("{} rows", model.inspect_rows(0, 0, |_| ()).0)),
        ),
        Value::Record(record) => ("Record", json!(record.name())),
        Value::RecordList(_) => ("RecordList", json!("Record model")),
        Value::Image(_) | Value::ImageAsset(_) => ("Image", json!("Image resource")),
        other => {
            // Stop formatting large texts or objects once the preview is full.
            let mut summary = BoundedString(String::new());
            let _ = std::fmt::write(&mut summary, format_args!("{other:?}"));
            let kind = summary.0.split(['(', '{', ' ']).next().unwrap_or("Value");
            if property["kind"].is_null() {
                property["kind"] = json!(kind);
            }
            property["value"] = json!(summary.0);
            return;
        }
    };
    if property["kind"].is_null() {
        property["kind"] = json!(kind);
    }
    property["value"] = value;
}

const MAX_FIELDS: usize = 6;

fn record_fields(record: &xrack_runtime::RecordValue, depth: usize) -> Vec<Json> {
    sample_fields(record, depth, &mut 24)
}

fn sample_fields(
    record: &xrack_runtime::RecordValue,
    depth: usize,
    remaining: &mut usize,
) -> Vec<Json> {
    let mut fields = Vec::new();
    for (name, value) in record.fields().take(MAX_FIELDS) {
        if *remaining == 0 {
            break;
        }
        *remaining -= 1;
        let mut field = json!({"name": name});
        describe(value, &mut field, depth, remaining);
        fields.push(field);
    }
    fields
}

fn set_items<T: Serialize>(property: &mut Json, kind: &str, count: usize, items: Vec<T>) {
    if property["kind"].is_null() {
        property["kind"] = json!(kind);
    }
    property["count"] = json!(count);
    property["items"] = json!(items);
}

struct BoundedString(String);

impl std::fmt::Write for BoundedString {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let remaining = MAX_STRING.saturating_sub(self.0.len());
        if text.len() <= remaining {
            self.0.push_str(text);
            Ok(())
        } else {
            self.0
                .push_str(&text[..text.floor_char_boundary(remaining)]);
            self.0.push('…');
            Err(std::fmt::Error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Program, live_trace::Trace};
    use std::cell::Cell;
    use xrack_runtime::Component;

    const UI: &str = r#"
        export component Search inherits Window {
            in-out property <[string]> results: [];
            property <int> untouched: root.read_private();
            pure callback read_private() -> int;
            callback search(query: string);
            go := Button {
                clicked => { root.search("XRack"); }
            }
        }
    "#;

    fn program(source: &str) -> Program {
        let document = xrack_parser::parse(source).unwrap();
        let module = xrack_mlir::lower_module(&document, "/tmp/xrack-debug/search.xr").unwrap();
        Program::from_module(&module).unwrap()
    }

    #[test]
    fn snapshots_retain_calls_locations_and_paginate_owned_model_across_reload() {
        let mut ui = ReloadableInstance::new(program(UI).create("Search").unwrap());
        let trace = Rc::new(Trace::new(true));
        ui.current().attach_trace(trace);
        let reads = Rc::new(Cell::new(0));
        ui.on("read_private", {
            let reads = reads.clone();
            Rc::new(move |_| {
                reads.set(reads.get() + 1);
                Ok(Some(Value::Int(42)))
            })
        })
        .unwrap();
        let calls = Rc::new(Cell::new(0));
        ui.on("search", {
            let calls = calls.clone();
            Rc::new(move |args| {
                assert_eq!(args, &[Value::String("XRack".into())]);
                calls.set(calls.get() + 1);
                Ok(None)
            })
        })
        .unwrap();
        ui.current()
            .set(
                "results",
                Value::StringList((0..20).map(|i| format!("result {i}")).collect()),
            )
            .unwrap();
        assert!(ui.dispatch("go", "clicked"));
        assert_eq!(calls.get(), 1);

        let state = snapshot(&ui, BTreeMap::new());
        let properties = state["properties"].as_array().unwrap();
        assert_eq!(properties[0]["name"], "results");
        assert_eq!(properties[0]["count"], 20);
        assert_eq!(properties[0]["visibility"], "in-out");
        assert_eq!(properties[1]["status"], "No cached value");
        assert_eq!(reads.get(), 0);
        let frames = state["trace"]["frames"].as_array().unwrap();
        let call = frames
            .iter()
            .find(|frame| frame["name"] == "search")
            .unwrap();
        let parent = frames
            .iter()
            .find(|frame| frame["id"] == call["parentId"])
            .unwrap();
        assert_eq!(parent["name"], "clicked");
        assert_eq!(call["parameters"][0]["name"], "query");
        assert_eq!(call["parameters"][0]["value"], "XRack");
        assert_eq!(call["callSite"]["file"], "/tmp/xrack-debug/search.xr");
        assert!(call["callSite"]["line"].as_u64() > call["source"]["line"].as_u64());

        let query = ModelQuery {
            node_key: "root".into(),
            property: "results".into(),
            offset: 13,
            limit: 2,
            generation: Some(0),
        };
        let page = model_page(&ui, &query, BTreeMap::new());
        assert_eq!(page["items"], json!(["result 13", "result 14"]));
        assert_eq!(page["nextOffset"], 15);
        ui.reload(&program(&UI.replace("go :=", "title: \"updated\"; go :=")))
            .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(snapshot(&ui, BTreeMap::new())["trace"]["generation"], 1);
        assert!(model_page(&ui, &query, BTreeMap::new())["error"].is_string());
        let next = ModelQuery {
            generation: Some(1),
            ..query
        };
        assert_eq!(
            model_page(&ui, &next, BTreeMap::new())["items"],
            page["items"]
        );
    }

    #[test]
    fn call_values_are_opt_in_and_record_fields_keep_types() {
        let ui = ReloadableInstance::new(program(UI).create("Search").unwrap());
        ui.current().attach_trace(Rc::new(Trace::new(false)));
        ui.current()
            .invoke("search", &[Value::String("private query".into())])
            .unwrap();
        let state = snapshot(&ui, BTreeMap::new());
        assert!(!state.to_string().contains("private query"));
        assert_eq!(
            state["trace"]["frames"][0]["parameters"][0]["kind"],
            "String"
        );

        let record = xrack_runtime::RecordValue::new(
            "Result",
            BTreeMap::from([
                ("exact".into(), Value::Int(i64::MAX)),
                ("ready".into(), Value::Bool(true)),
            ]),
        );
        let mut value = json!({});
        describe_value(&mut value, &Value::Record(record), 2);
        assert_eq!(
            value["fields"][0],
            json!({"name":"exact","kind":"Int","value":"9223372036854775807"})
        );
        assert_eq!(value["fields"][1]["value"], true);
    }
}
