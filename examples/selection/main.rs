use selection_ui::{EditableChoiceDemo, RecordComboDemo, SelectionDemo};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--editable-combo") => xrack_platform_winit::run(EditableChoiceDemo::new()),
        Some("--record-combo") => {
            let mut ui = RecordComboDemo::new();
            let options = ui.model_options();
            ui.on_reorder(move || {
                options.sort_by(|a, b| a.code.cmp(&b.code));
            });
            xrack_platform_winit::run(ui)
        }
        _ => xrack_platform_winit::run(SelectionDemo::new()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
