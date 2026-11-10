use stepper_ui::{LinkedControlsDemo, StepperDemo};

fn run_linked() -> Result<(), String> {
    let mut ui = LinkedControlsDemo::new();
    let model = ui.model_tasks();
    ui.on_sort_tasks(move || {
        model.sort_by(|left, right| left.name.cmp(&right.name));
    });
    xrack_platform_winit::run(ui)
}

fn main() -> std::process::ExitCode {
    let result = if std::env::args().nth(1).as_deref() == Some("--linked") {
        run_linked()
    } else {
        xrack_platform_winit::run(StepperDemo::new())
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
