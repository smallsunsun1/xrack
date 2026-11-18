use mobile_controls_ui::MobileControls;
use xrack_platform_winit::{AndroidApp, run_android};

#[unsafe(no_mangle)]
fn android_main(activity: AndroidApp) {
    if let Err(error) = run_android(activity, |desktop| {
        desktop.open(MobileControls::new())?;
        Ok(())
    }) {
        eprintln!("XRack Controls failed: {error}");
    }
}
