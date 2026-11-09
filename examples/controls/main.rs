use controls_ui::{
    CalendarDemo, CheckStateDemo, ColorDemo, ControlsDemo, DecimalSpinDemo, DelayDemo,
    GroupBoxDemo, IndicatorsDemo, ThemeDemo, TimeDemo, TimerDemo, TumblerDemo,
};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--calendar") => xrack_platform_winit::run(CalendarDemo::new()),
        Some("--time") => xrack_platform_winit::run(TimeDemo::new()),
        Some("--color") => xrack_platform_winit::run(ColorDemo::new()),
        Some("--theme") => xrack_platform_winit::run(ThemeDemo::new()),
        Some("--timer") => xrack_platform_winit::run(TimerDemo::new()),
        Some("--decimal-spin") => xrack_platform_winit::run(DecimalSpinDemo::new()),
        Some("--tumbler") => xrack_platform_winit::run(TumblerDemo::new()),
        Some("--delay") => xrack_platform_winit::run(DelayDemo::new()),
        Some("--check-state") => xrack_platform_winit::run(CheckStateDemo::new()),
        Some("--groups") => xrack_platform_winit::run(GroupBoxDemo::new()),
        Some("--indicators") => xrack_platform_winit::run(IndicatorsDemo::new()),
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
