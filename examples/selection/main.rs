use selection_ui::{EditableChoiceDemo, GroupedRadiosDemo, RecordComboDemo, SelectionDemo};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--radio-groups") => xrack_platform_winit::run(GroupedRadiosDemo::new()),
        Some("--editable-combo") => xrack_platform_winit::run(EditableChoiceDemo::new()),
        Some("--record-combo") => {
            let mut ui = RecordComboDemo::new();
            let options = ui.model_options();
            let archive = options.clone();
            ui.on_reorder(move || {
                options.sort_by(|a, b| a.code.cmp(&b.code));
            });
            ui.on_toggle_archive(move || {
                for index in 0..archive.len() {
                    let Some(mut row) = archive.row(index).filter(|row| row.code == 400) else {
                        continue;
                    };
                    row.access.available = !row.access.available;
                    archive.set_row(index, row);
                    break;
                }
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
