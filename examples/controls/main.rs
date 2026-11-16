use controls_ui::{
    CalendarAgendaDemo, CalendarDemo, CheckGroupDemo, CheckStateDemo, ColorDemo, ControlsDemo,
    DecimalSpinDemo, DelayDemo, FontPickerDemo, GroupBoxDemo, IndicatorsDemo, MonthGridDemo,
    PaneFrameDemo, RecordTumblerDemo, SnapSlidersDemo, ThemeDemo, TimeDemo, TimerDemo, TumblerDemo,
    WheelControlsDemo,
};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--slider-snap") => xrack_platform_winit::run(SnapSlidersDemo::new()),
        Some("--rtl-feedback") => {
            xrack_platform_winit::run(controls_ui::DirectionalFeedbackDemo::new())
        }
        Some("--rtl-sliders") => {
            xrack_platform_winit::run(controls_ui::DirectionalSlidersDemo::new())
        }
        Some("--wheel") => xrack_platform_winit::run(WheelControlsDemo::new()),
        Some("--panes") => xrack_platform_winit::run(PaneFrameDemo::new()),
        Some("--calendar") => xrack_platform_winit::run(CalendarDemo::new()),
        Some("--calendar-browser") => {
            xrack_platform_winit::run(controls_ui::CalendarBrowserDemo::new())
        }
        Some("--rtl-calendar") => {
            xrack_platform_winit::run(controls_ui::DirectionalCalendarDemo::new())
        }
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
        Some("--record-tumbler") => xrack_platform_winit::run(RecordTumblerDemo::new()),
        Some("--delay") => xrack_platform_winit::run(DelayDemo::new()),
        Some("--repeat-buttons") => {
            xrack_platform_winit::run(controls_ui::RepeatButtonsDemo::new())
        }
        Some(
            "--button-hold" | "--button-double-click" | "--button-pressed" | "--button-lifecycle",
        ) => {
            let mut ui = controls_ui::RepeatButtonsDemo::new();
            ui.set_repeating(false);
            xrack_platform_winit::run(ui)
        }
        Some("--check-state") => xrack_platform_winit::run(CheckStateDemo::new()),
        Some("--checkable-buttons") => {
            xrack_platform_winit::run(controls_ui::CheckableButtonsDemo::new())
        }
        Some("--grouped-buttons") => {
            xrack_platform_winit::run(controls_ui::GroupedButtonsDemo::new())
        }
        Some("--check-group") => xrack_platform_winit::run(CheckGroupDemo::new()),
        Some("--programmatic-click") => {
            xrack_platform_winit::run(controls_ui::ProgrammaticButtonsDemo::new())
        }
        Some("--programmatic-toggle") => {
            xrack_platform_winit::run(controls_ui::ProgrammaticToggleDemo::new())
        }
        Some("--groups") => xrack_platform_winit::run(GroupBoxDemo::new()),
        Some("--expander") => xrack_platform_winit::run(controls_ui::ExpanderDemo::new()),
        Some("--indicators") => xrack_platform_winit::run(IndicatorsDemo::new()),
        Some("--custom-progress") => {
            xrack_platform_winit::run(controls_ui::CustomProgressDemo::new())
        }
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
