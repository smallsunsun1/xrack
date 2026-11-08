use records_ui::RecordTableDemo;

fn main() -> std::process::ExitCode {
    let mut ui = RecordTableDemo::new();
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
    match xrack_platform_winit::run(ui) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
