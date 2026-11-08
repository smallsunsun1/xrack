use stepper_ui::StepperDemo;

fn main() -> std::process::ExitCode {
    match xrack_platform_winit::run(StepperDemo::new()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
