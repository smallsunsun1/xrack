use dialog_ui::{DialogDemo, PopupDemo, StandardDialogDemo};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--standard") => xrack_platform_winit::run(StandardDialogDemo::new()),
        Some("--popup") => xrack_platform_winit::run(PopupDemo::new()),
        _ => xrack_platform_winit::run(DialogDemo::new()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
