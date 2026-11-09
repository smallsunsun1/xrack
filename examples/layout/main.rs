use bundled_ui::BundledFontsDemo;
use layout_ui::{FlexDemo, FlowDemo, FontsDemo, LabelsDemo, LayoutDemo};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--flow") => xrack_platform_winit::run(FlowDemo::new()),
        Some("--flex") => xrack_platform_winit::run(FlexDemo::new()),
        Some("--labels") => xrack_platform_winit::run(LabelsDemo::new()),
        Some("--fonts") => xrack_platform_winit::run(FontsDemo::new()),
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
