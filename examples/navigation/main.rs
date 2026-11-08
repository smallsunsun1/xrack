use navigation_ui::{NavigationDemo, StackDemo};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--stack") => xrack_platform_winit::run(StackDemo::new()),
        _ => xrack_platform_winit::run(NavigationDemo::new()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
