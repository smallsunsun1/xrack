use bundled_ui::BundledFontsDemo;
use layout_ui::{
    AnchorsDemo, AnimationsDemo, ClippingDemo, ComponentStatesDemo, CompositingDemo, FlexDemo,
    FlickableDemo, FlowDemo, FocusScopesDemo, FontsDemo, FramelessDemo, GeometryDemo, HoverDemo,
    LabelsDemo, LayoutDemo, LayoutDirectionDemo, PointerDemo, PointerPassThroughDemo,
    RectangleShadowDemo, ScaleRotateDemo, ScopedKeysDemo, StatesDemo, StyledTextDemo,
    SwipeGestureDemo, TransformDemo, WheelDemo,
};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--anchors") => xrack_platform_winit::run(AnchorsDemo::new()),
        Some("--geometry") => xrack_platform_winit::run(GeometryDemo::new()),
        Some("--transforms") => xrack_platform_winit::run(TransformDemo::new()),
        Some("--flickable") => xrack_platform_winit::run(FlickableDemo::new()),
        Some("--scale-rotate") => xrack_platform_winit::run(ScaleRotateDemo::new()),
        Some("--swipe-gesture") => xrack_platform_winit::run(SwipeGestureDemo::new()),
        Some("--wheel") => xrack_platform_winit::run(WheelDemo::new()),
        Some("--hover") => xrack_platform_winit::run(HoverDemo::new()),
        Some("--pointer") => xrack_platform_winit::run(PointerDemo::new()),
        Some("--pointer-pass-through") => xrack_platform_winit::run(PointerPassThroughDemo::new()),
        Some("--clipping") => xrack_platform_winit::run(ClippingDemo::new()),
        Some("--shadows") => xrack_platform_winit::run(RectangleShadowDemo::new()),
        Some("--compositing") => xrack_platform_winit::run(CompositingDemo::new()),
        Some("--flow") => xrack_platform_winit::run(FlowDemo::new()),
        Some("--rtl") => xrack_platform_winit::run(LayoutDirectionDemo::new()),
        Some("--flex") => xrack_platform_winit::run(FlexDemo::new()),
        Some("--styled-text") => xrack_platform_winit::run(StyledTextDemo::new()),
        Some("--selectable-styled-text") => {
            let mut ui = StyledTextDemo::new();
            ui.set_selectable(true);
            xrack_platform_winit::run(ui)
        }
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
