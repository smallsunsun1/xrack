use controls_ui::{CalendarDemo, ControlsDemo, DecimalSpinDemo};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--calendar") => xrack_platform_winit::run(CalendarDemo::new()),
        Some("--decimal-spin") => xrack_platform_winit::run(DecimalSpinDemo::new()),
        _ => {
            let mut ui = ControlsDemo::new();
            ui.on_interval_caption(|lower, upper| format!("区间 {lower:.1} – {upper:.1}"));
            xrack_platform_winit::run(ui)
        }
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
