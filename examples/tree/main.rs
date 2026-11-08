use tree_ui::TreeDemo;

fn main() -> std::process::ExitCode {
    let mut ui = TreeDemo::new();
    let records = ui.model_records();
    ui.on_sort_requested(move || {
        records.sort_by(|a, b| a.label.cmp(&b.label));
    });
    match xrack_platform_winit::run(ui) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
