use navigation_ui::{NavigationDemo, StackDemo, TabBarDemo};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--stack") => xrack_platform_winit::run(StackDemo::new()),
        Some("--tab-bar") => xrack_platform_winit::run(TabBarDemo::new()),
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
