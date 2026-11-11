use menu_ui::{ActionGroupDemo, MenuDemo, RoundToolbarDemo, ShortcutDemo};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--shortcuts") => xrack_platform_winit::run(ShortcutDemo::new()),
        Some("--action-groups") => xrack_platform_winit::run(ActionGroupDemo::new()),
        Some("--round-tools") => xrack_platform_winit::run(RoundToolbarDemo::new()),
        _ => xrack_platform_winit::run(MenuDemo::new()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
