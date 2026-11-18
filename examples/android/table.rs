use mobile_table_ui::MobileTable;
use xrack_platform_winit::{AndroidApp, run_android};

#[unsafe(no_mangle)]
fn android_main(activity: AndroidApp) {
    if let Err(error) = run_android(activity, |desktop| {
        desktop.open(MobileTable::new())?;
        Ok(())
    }) {
        eprintln!("XRack Inventory failed: {error}");
    }
}
