use table_ui::{
    InventoryRow, LogRow, ProjectedRowsDemo, RowVisibilityDemo, RowVisibilityFlags,
    RowVisibilityRecord, StandardTableDemo, TableDemo, TableSortViewsDemo, WideTableDemo,
};
use xrack_runtime::{Length, SelectionTarget, Time};

fn main() -> std::process::ExitCode {
    let mode = std::env::args().nth(1);
    let result = match mode.as_deref() {
        Some(
            "--standard" | "--frozen" | "--rows" | "--row-headers" | "--move-rows" | "--sort-state"
            | "--selection-api" | "--position-cells" | "--editable",
        ) => run_standard(mode.as_deref()),
        Some("--row-field") => run_row_field(),
        Some("--row-visibility") => run_row_visibility(),
        Some("--sorted-views") => run_sorted_views(),
        Some("--wide") => xrack_platform_winit::run(WideTableDemo::new()),
        mode => run_table(mode),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_row_visibility() -> Result<(), String> {
    let mut ui = RowVisibilityDemo::new();
    let records = ui.model_records();
    for index in 4..64 {
        records.push(RowVisibilityRecord {
            name: format!("设备 {index:02}"),
            rank: 64 - index,
            flags: RowVisibilityFlags {
                shown: index % 7 != 1,
            },
        });
    }
    ui.on_toggle_first(move || {
        if let Some(mut row) = records.row(0) {
            row.flags.shown = !row.flags.shown;
            records.set_row(0, row);
        }
    });
    xrack_platform_winit::run(ui)
}

fn run_row_field() -> Result<(), String> {
    const DEMO_ROW: usize = 12_000;
    let mut ui = ProjectedRowsDemo::new();
    ui.set_entries(
        (0..50_000)
            .map(|index| {
                let warning = index % 10 == 0;
                LogRow {
                    message: format!("日志 {index:05}"),
                    severity: if warning { "warn" } else { "info" }.into(),
                    height: Length(if warning { 52.0 } else { 0.0 }),
                }
            })
            .collect(),
    );
    let model = ui.model_entries();
    ui.on_grow_record(move || {
        if let Some(mut row) = model.row(DEMO_ROW) {
            row.height = Length(if row.height.0 > 60.0 { 52.0 } else { 92.0 });
            model.set_row(DEMO_ROW, row);
        }
    });
    xrack_platform_winit::run(ui)
}

fn run_table(mode: Option<&str>) -> Result<(), String> {
    let mut ui = TableDemo::new();
    match mode {
        Some("--cells") => ui.set_target(SelectionTarget::Cells),
        Some("--columns") => ui.set_target(SelectionTarget::Columns),
        Some("--row-numbers") => ui.set_row_header_size(Length(56.0)),
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

fn run_standard(mode: Option<&str>) -> Result<(), String> {
    let mut ui = StandardTableDemo::new();
    if mode == Some("--frozen") {
        ui.set_pinned(1);
        ui.set_window_width(Length(480.0));
    }
    if matches!(mode, Some("--row-headers" | "--move-rows")) {
        ui.set_row_header_size(Length(116.0));
        ui.set_target(SelectionTarget::Cells);
        ui.set_pinned(1);
        ui.set_window_width(Length(540.0));
        ui.set_resize_rows(true);
        ui.set_move_rows(mode == Some("--move-rows"));
    }
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
    if mode == Some("--editable") {
        ui.set_edit_columns(true);
        ui.set_filter_tools(true);
        ui.set_paste_cells(true);
        ui.set_auto_sort_fields(true);
        ui.set_target(SelectionTarget::Cells);
        return xrack_platform_winit::run(ui);
    }
    if mode == Some("--rows") {
        ui.set_resize_rows(true);
        ui.invoke_set_height(0, Length(64.0));
    }
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
    if mode == Some("--sort-state") {
        ui.set_sort_tools(true);
        ui.invoke_sort_rows(1, false);
    }
    if mode == Some("--selection-api") {
        ui.set_selection_tools(true);
        ui.set_target(SelectionTarget::Cells);
    }
    if mode == Some("--position-cells") {
        ui.set_position_tools(true);
        ui.set_pinned(1);
        ui.set_row_header_size(Length(56.0));
        ui.set_window_width(Length(500.0));
        ui.set_target(SelectionTarget::Cells);
    }
    xrack_platform_winit::run(ui)
}

fn run_sorted_views() -> Result<(), String> {
    let mut ui = TableSortViewsDemo::new();
    let inventory = ui.model_inventory();
    ui.on_reduce_zulu(move || {
        if let Some(mut row) = inventory.row(0) {
            row.units = (row.units - 25).max(0);
            inventory.set_row(0, row);
        }
    });
    ui.invoke_initialize();
    xrack_platform_winit::run(ui)
}
