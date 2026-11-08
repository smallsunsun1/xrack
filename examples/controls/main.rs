use controls_ui::ControlsDemo;

fn main() -> std::process::ExitCode {
    let mut ui = ControlsDemo::new();
    ui.on_interval_caption(|lower, upper| format!("区间 {lower:.1} – {upper:.1}"));
    match xrack_platform_winit::run(ui) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
