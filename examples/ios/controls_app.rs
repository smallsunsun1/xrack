use list_ui::{RefreshDemo, ScrollBarsDemo, StandaloneScrollDemo};
use mobile_controls_ui::MobileControls;
use xrack_platform_winit::run;

#[unsafe(no_mangle)]
pub extern "C" fn xrack_ios_main() -> i32 {
    let demo = std::env::var("XRACK_CONTROLS_DEMO")
        .ok()
        .or_else(|| std::env::args().nth(1));
    let result = match demo.as_deref() {
        Some("--native-actions") => run(native_actions_ui::NativeActionsDemo::new()),
        Some("--compact-rows") => run(native_actions_ui::CompactRowsDemo::new()),
        Some("--compact-toggles") => run(native_actions_ui::CompactTogglesDemo::new()),
        Some("--modal-titles") => run(native_actions_ui::ModalTitlesDemo::new()),
        Some("--group-titles") => run(native_actions_ui::GroupTitlesDemo::new()),
        Some("--swipe-actions") => run(native_actions_ui::SwipeActionsDemo::new()),
        Some("--menu-paging") => run(native_actions_ui::MenuPagingDemo::new()),
        Some("--button-shapes") => run(native_actions_ui::ButtonShapesDemo::new()),
        Some("--native-selection") => run(native_actions_ui::NativeSelectionDemo::new()),
        Some("--system-direction") => run(native_actions_ui::SystemDirectionDemo::new()),
        Some("--emoji") => run(native_actions_ui::EmojiDemo::new()),
        Some("--clipboard-authorization") => {
            run(native_actions_ui::ClipboardAuthorizationDemo::new())
        }
        Some("--text-drop") => run(native_actions_ui::TextDropDemo::new()),
        Some("--text-drag") => run(native_actions_ui::TextDragDemo::new()),
        Some("--function-keys") => run(native_actions_ui::HardwareCommandsDemo::new()),
        Some("--touch-tooltip") => run(native_actions_ui::TouchTooltipDemo::new()),
        Some("--pointer-styles") => run(native_actions_ui::PointerStyleDemo::new()),
        Some("--appearance") => run(native_actions_ui::MobileAppearance::new()),
        Some("--file-drop") => run(form_ui::FileDropDemo::new()),
        Some("--scroll-bars") => run(ScrollBarsDemo::new()),
        Some("--pull-to-refresh") => run(RefreshDemo::new()),
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
