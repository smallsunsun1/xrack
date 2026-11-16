use crate::{Module, OperationRef};
use serde_json::{Value, json};

const MAX_ENTRIES: usize = 512;

impl Module {
    /// Report the UI decisions made by the verified XR optimization pipeline.
    pub fn update_plan(&self) -> Value {
        let mut entries = Vec::new();
        let mut truncated = false;
        for component in self.operations().filter(|op| op.name() == "xr.component") {
            if let Some(name) = component.attribute("sym_name") {
                collect(component, name, None, &mut entries, &mut truncated);
            }
        }
        json!({
            "pipelineApplied": self.has_visual_cache_plan(),
            "entries": entries,
            "truncated": truncated,
        })
    }
}

fn collect(
    op: OperationRef<'_>,
    component: &str,
    owner: Option<OperationRef<'_>>,
    entries: &mut Vec<Value>,
    truncated: &mut bool,
) {
    let owner = if op.name() == "xr.element" {
        Some(op)
    } else {
        owner
    };
    let decisions: Vec<_> = [
        ("xr.update_effect", "updateEffect"),
        ("xr.row_scene_cache", "rowSceneCache"),
        ("xr.static_subtree", "staticSubtree"),
        ("xr.static_subtree_group", "staticSubtreeGroup"),
        ("xr.visual_cache_group", "visualBindingGroup"),
    ]
    .into_iter()
    .filter_map(|(attribute, name)| op.attribute(attribute).map(|value| (name, value)))
    .collect();
    if !decisions.is_empty() {
        if entries.len() == MAX_ENTRIES {
            *truncated = true;
            return;
        }
        let span = op.span();
        entries.push(json!({
            "component": component,
            "operation": op.name(),
            "element": owner.and_then(|op| op.attribute("id")).filter(|name| !name.is_empty()),
            "kind": owner.and_then(|op| op.attribute("kind")),
            "property": op.attribute("name"),
            "source": op.source_file().map(|file| json!({
                "file": file, "line": span.line, "column": span.column,
            })),
            "decisions": decisions.into_iter().map(|(name, value)| (name.to_owned(), json!(value))).collect::<serde_json::Map<String, Value>>(),
        }));
    }
    for child in op.children() {
        collect(child, component, owner, entries, truncated);
        if *truncated {
            return;
        }
    }
}
