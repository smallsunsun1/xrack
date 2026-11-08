use counter_ui::Counter;

fn main() -> std::process::ExitCode {
    match xrack_platform_winit::run(Counter::new()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
