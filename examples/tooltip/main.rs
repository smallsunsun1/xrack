use tooltip_ui::TooltipDemo;

fn main() -> std::process::ExitCode {
    match xrack_platform_winit::run(TooltipDemo::new()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
