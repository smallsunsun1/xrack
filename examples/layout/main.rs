use layout_ui::LayoutDemo;

fn main() -> std::process::ExitCode {
    match xrack_platform_winit::run(LayoutDemo::new()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
