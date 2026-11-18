use mobile_navigation_ui::MobileNavigation;
use xrack_platform_winit::{AndroidApp, run_android};

#[unsafe(no_mangle)]
fn android_main(activity: AndroidApp) {
    if let Err(error) = run_android(activity, |desktop| {
        desktop.open(MobileNavigation::new())?;
        Ok(())
    }) {
        eprintln!("XRack Navigation failed: {error}");
    }
}
