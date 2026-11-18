use split_ui::FillSplitDemo;
use xrack_runtime::Length;

#[unsafe(no_mangle)]
pub extern "C" fn xrack_ios_main() -> i32 {
    let mut app = FillSplitDemo::new();
    app.set_handle_target(Length(44.0));
    match xrack_platform_winit::run(app) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("XRack Split failed: {error}");
            1
        }
    }
}
