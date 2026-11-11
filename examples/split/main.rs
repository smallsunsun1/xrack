use split_ui::{CustomSplitDemo, FillSplitDemo, SplitDemo};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--fill-pane") => xrack_platform_winit::run(FillSplitDemo::new()),
        Some("--custom-handle") => xrack_platform_winit::run(CustomSplitDemo::new()),
        _ => xrack_platform_winit::run(SplitDemo::new()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
