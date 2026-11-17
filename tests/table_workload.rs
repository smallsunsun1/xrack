//! Shared data and input for the table interaction test and GPU benchmark.
use table_workload_ui::TableWorkload;
use xrack_interpreter::Instance;
use xrack_runtime::{Component, VecModel};
use xrack_testing::{Point, Size, UiDriver};

pub const VIEWPORT: Size = Size::new(640.0, 360.0);
pub const ROW_COUNT: usize = 10_000;
pub const GRID: &str = "grid";

fn rows() -> Vec<String> {
    (0..ROW_COUNT)
        .map(|index| format!("Device {index:05}"))
        .collect()
}

pub fn aot() -> (TableWorkload, VecModel<String>) {
    let mut component = TableWorkload::new();
    component.set_records(rows());
    let model = component.model_records();
    (component, model)
}

pub fn interpreted() -> Result<(Instance, VecModel<String>), Box<dyn std::error::Error>> {
    let mlir =
        xrack_compiler::compile(include_str!("table_workload.xr"), "table_workload.xr")?.mlir;
    let component = xrack_interpreter::load_mlir(&mlir)?.create("TableWorkload")?;
    let model = component.string_model("records")?;
    for row in rows() {
        model.push(row);
    }
    Ok((component, model))
}

pub fn scroll<C: Component>(ui: &mut UiDriver<C>) {
    assert!(ui.scroll(GRID, Point { x: 0.0, y: 56.0 }));
}

pub fn update_visible<C: Component>(
    ui: &mut UiDriver<C>,
    rows: &VecModel<String>,
    step: usize,
) -> usize {
    let Some(list) = ui.node(GRID).list else {
        panic!("table workload needs virtualized rows");
    };
    let index = list.first_row + 2;
    assert!(rows.set_row(index, format!("Updated {step:03}")));
    ui.update(|_| {});
    index
}
