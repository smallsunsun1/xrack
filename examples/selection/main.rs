use selection_ui::{EditableChoiceDemo, SelectionDemo};

fn main() -> std::process::ExitCode {
    let result = if std::env::args().nth(1).as_deref() == Some("--editable-combo") {
        xrack_platform_winit::run(EditableChoiceDemo::new())
    } else {
        xrack_platform_winit::run(SelectionDemo::new())
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
