use bundled_ui::BundledFontsDemo;
use layout_ui::{
    AnimationsDemo, CompositingDemo, FlexDemo, FlowDemo, FocusScopesDemo, FontsDemo, LabelsDemo,
    LayoutDemo, ScopedKeysDemo, StatesDemo, StyledTextDemo,
};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--compositing") => xrack_platform_winit::run(CompositingDemo::new()),
        Some("--flow") => xrack_platform_winit::run(FlowDemo::new()),
        Some("--flex") => xrack_platform_winit::run(FlexDemo::new()),
        Some("--styled-text") => xrack_platform_winit::run(StyledTextDemo::new()),
        Some("--labels") => xrack_platform_winit::run(LabelsDemo::new()),
        Some("--fonts") => xrack_platform_winit::run(FontsDemo::new()),
        Some("--states") => xrack_platform_winit::run(StatesDemo::new()),
        Some("--key-routing") => xrack_platform_winit::run(ScopedKeysDemo::new()),
        Some("--focus-scopes") => xrack_platform_winit::run(FocusScopesDemo::new()),
        Some("--animations") => xrack_platform_winit::run(AnimationsDemo::new()),
        Some("--bundled-fonts") => xrack_platform_winit::run(BundledFontsDemo::new()),
        _ => xrack_platform_winit::run(LayoutDemo::new()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
