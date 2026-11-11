use controls_ui::{
    CalendarAgendaDemo, CalendarDemo, CheckGroupDemo, CheckStateDemo, ColorDemo, ControlsDemo,
    DecimalSpinDemo, DelayDemo, FontPickerDemo, GroupBoxDemo, IndicatorsDemo, MonthGridDemo,
    PaneFrameDemo, SnapSlidersDemo, ThemeDemo, TimeDemo, TimerDemo, TumblerDemo, WheelControlsDemo,
};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--slider-snap") => xrack_platform_winit::run(SnapSlidersDemo::new()),
        Some("--wheel") => xrack_platform_winit::run(WheelControlsDemo::new()),
        Some("--panes") => xrack_platform_winit::run(PaneFrameDemo::new()),
        Some("--calendar") => xrack_platform_winit::run(CalendarDemo::new()),
        Some("--calendar-agenda") => {
            let mut ui = CalendarAgendaDemo::new();
            let weak = ui.as_weak();
            ui.on_browse(move |offset| {
                if let Some(mut ui) = weak.upgrade()
                    && let Some(month) = ui.get_month().add_months(offset)
                {
                    ui.set_month(month);
                }
            });
            xrack_platform_winit::run(ui)
        }
        Some("--month-grid") => {
            let mut ui = MonthGridDemo::new();
            let weak = ui.as_weak();
            ui.on_browse(move |offset| {
                if let Some(mut ui) = weak.upgrade()
                    && let Some(month) = ui.get_month().add_months(offset)
                {
                    ui.set_month(month);
                }
            });
            xrack_platform_winit::run(ui)
        }
        Some("--time") => xrack_platform_winit::run(TimeDemo::new()),
        Some("--color") => {
            let mut ui = ColorDemo::new();
            ui.set_plane_visible(true);
            ui.set_picker_open(true);
            xrack_platform_winit::run(ui)
        }
        Some("--font-picker") => {
            let mut ui = FontPickerDemo::new();
            ui.set_picker_open(true);
            xrack_platform_winit::run(ui)
        }
        Some("--theme") => xrack_platform_winit::run(ThemeDemo::new()),
        Some("--timer") => xrack_platform_winit::run(TimerDemo::new()),
        Some("--decimal-spin") => xrack_platform_winit::run(DecimalSpinDemo::new()),
        Some("--tumbler") => xrack_platform_winit::run(TumblerDemo::new()),
        Some("--delay") => xrack_platform_winit::run(DelayDemo::new()),
        Some("--check-state") => xrack_platform_winit::run(CheckStateDemo::new()),
        Some("--check-group") => xrack_platform_winit::run(CheckGroupDemo::new()),
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
