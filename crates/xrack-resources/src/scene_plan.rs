use xrack_mlir::{OperationRef, scene_children};
use xrack_runtime::{GridCellSlot, GridPlanner, GridTrackInput};

#[derive(Default)]
pub struct ScenePlan {
    pub grid: Option<Vec<GridCellSlot>>,
    pub paint_order: Option<Vec<usize>>,
}

impl ScenePlan {
    /// Read the layout and paint plans attached by the XR optimization pipeline.
    pub fn from_operation(element: OperationRef<'_>) -> Result<Self, String> {
        let grid = (element.attribute("xr.static_grid") == Some("true"))
            .then(|| grid(element))
            .transpose()?;
        let paint_order = element
            .i64_array_attribute("xr.static_z_order")
            .map(|indices| paint_order(element, indices))
            .transpose()?;
        Ok(Self { grid, paint_order })
    }
}

fn paint_order(element: OperationRef<'_>, indices: Vec<i64>) -> Result<Vec<usize>, String> {
    if indices.is_empty() {
        return Ok(Vec::new());
    }
    let count = scene_children(element).count();
    if indices.len() != count {
        return Err("static paint order must cover all scene children".into());
    }
    let mut seen = vec![false; count];
    indices
        .into_iter()
        .map(|index| {
            let index = usize::try_from(index).map_err(|_| "negative static paint index")?;
            let slot = seen
                .get_mut(index)
                .ok_or("static paint index out of range")?;
            if std::mem::replace(slot, true) {
                return Err("duplicate static paint index".into());
            }
            Ok(index)
        })
        .collect()
}

fn literal_binding<'a>(element: OperationRef<'a>, name: &str) -> Result<Option<&'a str>, String> {
    element
        .children()
        .filter(|op| op.name() == "xr.bind" && op.attribute("name") == Some(name))
        .last()
        .map(|binding| {
            binding
                .operand(0)
                .and_then(|value| value.defining_operation())
                .filter(|definition| definition.name() == "xr.literal")
                .ok_or_else(|| format!("static grid requires a literal {name} binding"))?
                .attr("value")
        })
        .transpose()
}

fn grid(element: OperationRef<'_>) -> Result<Vec<GridCellSlot>, String> {
    let mut planner = GridPlanner::default();
    let mut slots = Vec::new();
    let mut add = |planner: &mut GridPlanner, cell, index, child_index| -> Result<(), String> {
        if literal_binding(cell, "visible")? == Some("false") {
            return Ok(());
        }
        let track = |name| {
            literal_binding(cell, name)?
                .map(|text| {
                    text.parse::<i64>()
                        .map_err(|error| format!("invalid static grid {name}: {error}"))
                })
                .transpose()
        };
        let input = GridTrackInput {
            row: track("row")?,
            col: track("col")?,
            rowspan: track("rowspan")?,
            colspan: track("colspan")?,
        };
        if let Some(position) = planner.place(input) {
            slots.push(GridCellSlot {
                index,
                child_index,
                position,
            });
        }
        Ok(())
    };
    for (index, child) in scene_children(element).enumerate() {
        if child.attr("kind")? == "Row" {
            if literal_binding(child, "visible")? == Some("false") {
                continue;
            }
            planner.begin_row();
            for (child_index, cell) in scene_children(child).enumerate() {
                add(&mut planner, cell, index, Some(child_index))?;
            }
            planner.end_row();
        } else {
            add(&mut planner, child, index, None)?;
        }
    }
    Ok(slots)
}
