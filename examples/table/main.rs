use table_ui::{InventoryRow, StandardTableDemo, TableDemo};
use xrack_runtime::{SelectionTarget, Time};

fn main() -> std::process::ExitCode {
    let mode = std::env::args().nth(1);
    let result = if mode.as_deref() == Some("--standard") {
        run_standard()
    } else {
        run_table(mode.as_deref())
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_table(mode: Option<&str>) -> Result<(), String> {
    let mut ui = TableDemo::new();
    match mode {
        Some("--cells") => ui.set_target(SelectionTarget::Cells),
        Some("--columns") => ui.set_target(SelectionTarget::Columns),
        _ => {}
    }
    ui.set_records(
        (1..=10_000)
            .rev()
            .map(|row| format!("记录 {row:05}"))
            .collect(),
    );
    let records = ui.model_records();
    ui.on_sort_requested(move |_column, descending| {
        records.sort_by(|a, b| if descending { b.cmp(a) } else { a.cmp(b) });
    });
    xrack_platform_winit::run(ui)
}

fn run_standard() -> Result<(), String> {
    let mut ui = StandardTableDemo::new();
    ui.set_inventory(
        (0..50_000)
            .map(|index| InventoryRow {
                name: format!("设备 {index:05}"),
                units: 50_000 - index,
                available: index % 4 != 0,
                refreshed: Time {
                    hour: index % 24,
                    minute: index % 60,
                    second: 0,
                },
            })
            .collect(),
    );
    ui.set_target_row(49_999);
    let rows = ui.model_inventory();
    ui.on_sort_requested(move |column, descending| {
        rows.sort_by(|a, b| {
            let order = match column {
                0 => a.name.cmp(&b.name),
                1 => a.units.cmp(&b.units),
                2 => a.available.cmp(&b.available),
                _ => a.refreshed.cmp(&b.refreshed),
            };
            if descending { order.reverse() } else { order }
        });
    });
    xrack_platform_winit::run(ui)
}
