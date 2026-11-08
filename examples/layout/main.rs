use layout_ui::{FlowDemo, LayoutDemo};

fn main() -> std::process::ExitCode {
    let result = if std::env::args().nth(1).as_deref() == Some("--flow") {
        xrack_platform_winit::run(FlowDemo::new())
    } else {
        xrack_platform_winit::run(LayoutDemo::new())
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
