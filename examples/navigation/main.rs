use navigation_ui::{
    ApplicationWindowDemo, NavigationDemo, PageDemo, PathViewDemo, StackDemo, StackGuardDemo,
    TabBarDemo,
};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--stack") => xrack_platform_winit::run(StackDemo::new()),
        Some("--stack-guard") => xrack_platform_winit::run(StackGuardDemo::new()),
        Some("--tab-bar") => xrack_platform_winit::run(TabBarDemo::new()),
        Some("--rtl-tabs") => xrack_platform_winit::run(navigation_ui::DirectionalTabsDemo::new()),
        Some("--model-tabs") => xrack_platform_winit::run(navigation_ui::ModelTabsDemo::new()),
        Some("--model-swipe") => xrack_platform_winit::run(navigation_ui::ModelSwipeDemo::new()),
        Some("--value-tabs") => xrack_platform_winit::run(navigation_ui::ValueTabsDemo::new()),
        Some("--path-view") => xrack_platform_winit::run(PathViewDemo::new()),
        Some("--page") => xrack_platform_winit::run(PageDemo::new()),
        Some("--application-window") => xrack_platform_winit::run(ApplicationWindowDemo::new()),
        _ => xrack_platform_winit::run(NavigationDemo::new()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
