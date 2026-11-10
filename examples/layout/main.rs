use bundled_ui::BundledFontsDemo;
use layout_ui::{
    AnimationsDemo, ClippingDemo, ComponentStatesDemo, CompositingDemo, FlexDemo, FlickableDemo,
    FlowDemo, FocusScopesDemo, FontsDemo, FramelessDemo, GeometryDemo, LabelsDemo, LayoutDemo,
    RectangleShadowDemo, ScaleRotateDemo, ScopedKeysDemo, StatesDemo, StyledTextDemo,
    SwipeGestureDemo, TransformDemo,
};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--geometry") => xrack_platform_winit::run(GeometryDemo::new()),
        Some("--transforms") => xrack_platform_winit::run(TransformDemo::new()),
        Some("--flickable") => xrack_platform_winit::run(FlickableDemo::new()),
        Some("--scale-rotate") => xrack_platform_winit::run(ScaleRotateDemo::new()),
        Some("--swipe-gesture") => xrack_platform_winit::run(SwipeGestureDemo::new()),
        Some("--clipping") => xrack_platform_winit::run(ClippingDemo::new()),
        Some("--shadows") => xrack_platform_winit::run(RectangleShadowDemo::new()),
        Some("--compositing") => xrack_platform_winit::run(CompositingDemo::new()),
        Some("--flow") => xrack_platform_winit::run(FlowDemo::new()),
        Some("--flex") => xrack_platform_winit::run(FlexDemo::new()),
        Some("--styled-text") => xrack_platform_winit::run(StyledTextDemo::new()),
        Some("--labels") => xrack_platform_winit::run(LabelsDemo::new()),
        Some("--fonts") => xrack_platform_winit::run(FontsDemo::new()),
        Some("--states") => xrack_platform_winit::run(StatesDemo::new()),
        Some("--component-states") => xrack_platform_winit::run(ComponentStatesDemo::new()),
        Some("--frameless") => xrack_platform_winit::run(FramelessDemo::new()),
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
