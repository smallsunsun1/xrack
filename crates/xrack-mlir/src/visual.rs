//! Decode visual operations shared by the code generator and interpreter.
use crate::{OperationRef, ValueRef};

/// Direct children that occupy a scene slot, in declaration order.
pub fn scene_children(op: OperationRef<'_>) -> impl Iterator<Item = OperationRef<'_>> {
    op.children().filter(|child| {
        matches!(child.name(), "xr.element" | "xr.instance")
            && !matches!(
                child.attribute("kind"),
                Some(
                    "Action"
                        | "ActionGroup"
                        | "ButtonGroup"
                        | "Timer"
                        | "SecondaryWindow"
                        | "DayCell"
                        | "SplitHandle"
                        | "FileDialog"
                        | "MessageDialog"
                )
            )
    })
}

/// Repeats with an explicit position use an overlay instead of a vertical layout.
pub fn repeat_is_positioned(element: OperationRef<'_>) -> bool {
    element.children().any(|op| {
        matches!(op.name(), "xr.bind" | "xr.layout_bind")
            && matches!(op.attribute("name"), Some("x" | "y"))
    })
}

/// An expression cache planned by the native XR passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VisualCacheGroup {
    Binding(u32),
    RepeatInvariant(u32),
    RepeatSubexpression(u32),
}

impl VisualCacheGroup {
    pub fn for_binding(op: OperationRef<'_>) -> Option<Self> {
        op.attribute("xr.repeat_invariant_group")
            .and_then(|group| group.parse().ok())
            .map(Self::RepeatInvariant)
            .or_else(|| {
                op.attribute("xr.visual_cache_group")
                    .and_then(|group| group.parse().ok())
                    .map(Self::Binding)
            })
    }

    pub fn for_subexpression(op: OperationRef<'_>) -> Option<Self> {
        op.attribute("xr.repeat_subexpression_group")
            .and_then(|group| group.parse().ok())
            .map(Self::RepeatSubexpression)
    }
}

/// A visual cache keyed by a pass group or the original SSA operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VisualCacheKey<Operand> {
    Operand(Operand),
    Planned(VisualCacheGroup),
}

impl<Operand> VisualCacheKey<Operand> {
    pub fn uses_component_scope(&self) -> bool {
        matches!(
            self,
            Self::Planned(
                VisualCacheGroup::RepeatInvariant(_) | VisualCacheGroup::RepeatSubexpression(_)
            )
        )
    }
}

impl<'m> VisualCacheKey<ValueRef<'m>> {
    pub fn for_binding(binding: OperationRef<'m>, use_visual_plan: bool) -> Option<Self> {
        if use_visual_plan && let Some(group) = VisualCacheGroup::for_binding(binding) {
            return Some(Self::Planned(group));
        }
        binding.operand(0).map(Self::Operand)
    }
}

/// Literals, plain reads and row parameters already have direct access paths.
pub fn visual_cache_operand(
    binding: OperationRef<'_>,
    use_visual_plan: bool,
) -> Option<OperationRef<'_>> {
    let definition = binding.operand(0)?.defining_operation()?;
    let static_gradient = use_visual_plan
        && matches!(
            definition.name(),
            "xr.linear_gradient" | "xr.radial_gradient"
        )
        && definition.attribute("xr.static_gradient") == Some("true");
    (!static_gradient
        && !matches!(
            definition.name(),
            "xr.literal" | "xr.read" | "xr.image_url" | "xr.param"
        ))
    .then_some(definition)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowSceneCache {
    Keyed,
    Indexed,
}

impl RowSceneCache {
    pub fn from_operation(op: OperationRef<'_>) -> Option<Self> {
        match op.attribute("xr.row_scene_cache")? {
            "keyed" => Some(Self::Keyed),
            "indexed" => Some(Self::Indexed),
            _ => None,
        }
    }
}

pub struct AnimationRef<'m> {
    pub duration: ValueRef<'m>,
    pub enabled: Option<ValueRef<'m>>,
    pub delay_ms: u64,
    pub curve: Option<[f64; 4]>,
}

impl<'m> AnimationRef<'m> {
    pub fn from_operation(op: OperationRef<'m>) -> Result<Self, String> {
        let offset = match op.name() {
            "xr.animate" => 0,
            "xr.state_timing" => 1,
            name => return Err(format!("expected animation timing, found {name}")),
        };
        Ok(Self {
            duration: op.operand(offset).ok_or("animation missing duration")?,
            enabled: op.operand(offset + 1),
            delay_ms: u64::try_from(op.integer_attribute("delay").unwrap_or_default())
                .map_err(|_| "negative animation delay")?,
            curve: op.f64_array_attribute("curve"),
        })
    }
}

pub struct StateCaseRef<'m> {
    pub selector: ValueRef<'m>,
    pub value: ValueRef<'m>,
    pub index: i64,
}

impl<'m> StateCaseRef<'m> {
    pub fn from_operation(op: OperationRef<'m>) -> Result<Self, String> {
        if !matches!(
            op.name(),
            "xr.state_set" | "xr.instance_state" | "xr.state_timing"
        ) {
            return Err(format!("expected state case, found {}", op.name()));
        }
        Ok(Self {
            selector: op.operand(0).ok_or("state case missing selector")?,
            value: op.operand(1).ok_or("state case missing value")?,
            index: op
                .integer_attribute("case_index")
                .ok_or("state case missing an index")?,
        })
    }
}

pub struct TranslationRef<'m> {
    pub id: &'m str,
    pub arguments: Vec<(&'m str, ValueRef<'m>)>,
}

impl<'m> TranslationRef<'m> {
    pub fn from_operation(op: OperationRef<'m>) -> Result<Self, String> {
        if op.name() != "xr.translate" {
            return Err(format!("expected translation, found {}", op.name()));
        }
        let names = op
            .string_array_attribute("arg_names")
            .ok_or("invalid translation argument names")?;
        if names.len() != op.operand_count() {
            return Err("translation argument count does not match its names".into());
        }
        let arguments = names
            .into_iter()
            .enumerate()
            .map(|(index, name)| (name, op.operand(index).expect("checked argument count")))
            .collect();
        Ok(Self {
            id: op.attr("value")?,
            arguments,
        })
    }
}

pub struct RepeatRef<'m> {
    pub model: ValueRef<'m>,
    pub item: &'m str,
    pub index: Option<&'m str>,
}

impl<'m> RepeatRef<'m> {
    pub fn from_operation(op: OperationRef<'m>) -> Result<Self, String> {
        if op.name() != "xr.repeat" {
            return Err(format!("expected repeat, found {}", op.name()));
        }
        let index = op.attr("index")?;
        Ok(Self {
            model: op.operand(0).ok_or("repeat missing model")?,
            item: op.attr("item")?,
            index: (!index.is_empty()).then_some(index),
        })
    }
}

pub fn nine_slice_edges(op: OperationRef<'_>) -> Result<[u32; 4], String> {
    let edge = |index| {
        op.operand(index)
            .and_then(|value| value.defining_operation())
            .and_then(|literal| literal.attribute("value"))
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| "expected a nonnegative nine-slice edge".to_owned())
    };
    Ok([edge(1)?, edge(2)?, edge(3)?, edge(4)?])
}
