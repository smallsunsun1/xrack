use table_ui::TableDemo;
use xrack_runtime::SelectionTarget;

fn main() -> std::process::ExitCode {
    let mut ui = TableDemo::new();
    match std::env::args().nth(1).as_deref() {
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
    match xrack_platform_winit::run(ui) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
