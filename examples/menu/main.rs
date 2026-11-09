use menu_ui::{MenuDemo, ShortcutDemo};

fn main() -> std::process::ExitCode {
    let result = if std::env::args().nth(1).as_deref() == Some("--shortcuts") {
        xrack_platform_winit::run(ShortcutDemo::new())
    } else {
        xrack_platform_winit::run(MenuDemo::new())
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
