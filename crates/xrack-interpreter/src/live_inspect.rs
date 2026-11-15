use crate::{Instance, Node, ReloadableInstance, Value, ValueType, Visual, live::SavedSources};
use serde::Serialize;
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

#[derive(Serialize)]
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
    properties: Vec<Json>,
}

#[derive(Default, Serialize)]
struct Tree<'a> {
    nodes: Vec<Entry<'a>>,
    truncated: bool,
    #[serde(skip)]
    details: bool,
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

fn element_properties(node: &Node) -> Vec<Json> {
    let mut properties = vec![
        json!({"name": "visible", "kind": "Bool", "value": node.is_visible()}),
        json!({"name": "enabled", "kind": "Bool", "value": node.boolean("enabled", true)}),
    ];
    if let Some(list) = node.list {
        properties.push(json!({"name": "row-count", "kind": "Int", "value": list.row_count}));
    }
    properties.extend(node.properties.iter().filter_map(|(&name, value)| {
        if matches!(name, "visible" | "enabled") || name.starts_with("__") {
            return None;
        }
        if node.is_password() && matches!(name, "text" | "selected-text" | "display-text") {
            return Some(json!({"name": name, "kind": "String", "value": "••••"}));
        }
        let (kind, value) = match value {
            Value::Bool(value) => ("Bool", json!(value)),
            Value::Int(value) => ("Int", json!(value.to_string())),
            Value::Float(value) => ("Float", json!(value.to_string())),
            Value::String(value) => ("String", json!(shortened(value))),
            Value::Length(value) => ("Length", json!(format!("{}px", value.0))),
            Value::Percent(value) => ("Percent", json!(format!("{}%", value.0 * 100.0))),
            Value::Color(value) => ("Color", json!(format!("#{:08x}", value.0))),
            Value::InputType(value) => ("InputType", json!(format!("{value:?}"))),
            Value::StringList(items) => return Some(json!({
                "name": name, "kind": "StringList", "count": items.len(),
                "items": items.iter().take(5).map(|text| shortened(text)).collect::<Vec<_>>(),
            })),
            Value::StringModel(items) => return Some(json!({
                "name": name, "kind": "StringList", "count": items.len(),
                "items": (0..items.len().min(5)).filter_map(|i| items.row_at(i as i64))
                    .map(|text| shortened(&text)).collect::<Vec<_>>(),
            })),
            _ => return None,
        };
        Some(json!({"name": name, "kind": kind, "value": value}))
    }).take(MAX_ELEMENT_PROPERTIES));
    properties
}

fn shortened(value: &str) -> String {
    let mut chars = value.chars();
    let mut result: String = chars.by_ref().take(MAX_STRING).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

fn public_properties(instance: &Instance, limit: usize) -> Vec<Json> {
    instance
        .properties()
        .take(limit)
        .map(|(name, kind, _)| {
            let mut property = json!({"name": name, "kind": format!("{kind:?}")});
            if kind == ValueType::StringList {
                match instance.string_model(name) {
                    Ok(model) => {
                        let count = model.len();
                        let items: Vec<_> = (0..count.min(MAX_ROWS))
                            .filter_map(|index| {
                                model.row_at(index as i64).map(|text| shortened(&text))
                            })
                            .collect();
                        property["count"] = json!(count);
                        property["items"] = json!(items);
                    }
                    Err(error) => property["error"] = json!(shortened(&error.to_string())),
                }
            } else if matches!(
                kind,
                ValueType::Bool
                    | ValueType::Float
                    | ValueType::Int
                    | ValueType::String
                    | ValueType::Length
                    | ValueType::Color
            ) {
                match instance.get(name) {
                    Ok(Value::Bool(value)) => property["value"] = json!(value),
                    Ok(Value::Float(value)) => property["value"] = json!(value.to_string()),
                    Ok(Value::Int(value)) => property["value"] = json!(value.to_string()),
                    Ok(Value::String(value)) => property["value"] = json!(shortened(&value)),
                    Ok(Value::Length(value)) => property["value"] = json!(format!("{}px", value.0)),
                    Ok(Value::Color(value)) => {
                        property["value"] = json!(format!("#{:08x}", value.0))
                    }
                    Ok(_) => {}
                    Err(error) => property["error"] = json!(shortened(&error.to_string())),
                }
            }
            property
        })
        .collect()
}
