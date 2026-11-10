use records_ui::{DelegateTableDemo, NestedRecordsDemo, QuantityPaste, RecordTableDemo, StockRow};

fn decode_quantity(text: String) -> QuantityPaste {
    let text = text.trim();
    let number = text.strip_suffix('件').unwrap_or(text).trim();
    let value = number
        .parse::<i64>()
        .ok()
        .filter(|value| (0..=999).contains(value));
    QuantityPaste {
        accepted: value.is_some(),
        value: value.unwrap_or_default(),
    }
}

fn run_delegates() -> Result<(), String> {
    let mut ui = DelegateTableDemo::new();
    ui.on_decode_quantity(|text, _, _| decode_quantity(text));
    ui.set_records(
        (0..2_000)
            .map(|index| StockRow {
                name: format!("设备 {index:04}"),
                units: index % 100,
                available: index % 3 != 0,
                ..StockRow::default()
            })
            .collect(),
    );
    let model = ui.model_records();
    ui.on_sort_requested(move |column, descending| {
        model.sort_by(|a, b| {
            let order = match column {
                1 => a.units.cmp(&b.units),
                2 => a.available.cmp(&b.available),
                _ => a.name.cmp(&b.name),
            };
            if descending { order.reverse() } else { order }
        });
    });
    xrack_platform_winit::run(ui)
}

fn main() -> std::process::ExitCode {
    let result = if std::env::args().any(|arg| arg == "--delegates") {
        run_delegates()
    } else if std::env::args().any(|arg| arg == "--nested") {
        let mut ui = NestedRecordsDemo::new();
        ui.on_decode_quantity(decode_quantity);
        xrack_platform_winit::run(ui)
    } else {
        run_records()
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_records() -> Result<(), String> {
    let mut ui = RecordTableDemo::new();
    if std::env::args().any(|arg| arg == "--cells") {
        ui.set_target(xrack_runtime::SelectionTarget::Cells);
    }
    let records = ui.model_records();
    ui.on_sort_requested(move |column, descending| {
        records.sort_by(|left, right| {
            let ordering = match column {
                1 => left.owner.cmp(&right.owner),
                2 => left.units.cmp(&right.units),
                3 => left.available.cmp(&right.available),
                4 => left.load.total_cmp(&right.load),
                _ => left.name.cmp(&right.name),
            };
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    });
    xrack_platform_winit::run(ui)
}
