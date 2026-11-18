use list_ui::{ScrollBarsDemo, StandaloneScrollDemo};
use mobile_controls_ui::MobileControls;
use xrack_platform_winit::run;

#[unsafe(no_mangle)]
pub extern "C" fn xrack_ios_main() -> i32 {
    let demo = std::env::var("XRACK_CONTROLS_DEMO")
        .ok()
        .or_else(|| std::env::args().nth(1));
    let result = match demo.as_deref() {
        Some("--native-actions") => run(native_actions_ui::NativeActionsDemo::new()),
        Some("--scroll-bars") => run(ScrollBarsDemo::new()),
        Some("--standalone-scroll") => run(StandaloneScrollDemo::new()),
        _ => {
            let mut app = MobileControls::new();
            app.set_lower(50.0);
            app.set_upper(50.0);
            run(app)
        }
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("XRack Controls failed: {error}");
            1
        }
    }
}
