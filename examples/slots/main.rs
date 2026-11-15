fn main() -> Result<(), String> {
    if std::env::args().any(|arg| arg == "--tab-content") {
        xrack_platform_winit::run(slots_ui::TabContentDemo::new())
    } else if std::env::args().any(|arg| arg == "--range-handles") {
        xrack_platform_winit::run(slots_ui::RangeHandleDemo::new())
    } else if std::env::args().any(|arg| arg == "--spin-indicators") {
        xrack_platform_winit::run(slots_ui::SpinIndicatorDemo::new())
    } else if std::env::args().any(|arg| arg == "--toggle-indicators") {
        xrack_platform_winit::run(slots_ui::ToggleIndicatorDemo::new())
    } else {
        xrack_platform_winit::run(slots_ui::SlotDemo::new())
    }
}
