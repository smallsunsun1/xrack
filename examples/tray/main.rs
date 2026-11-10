use tray_ui::{TrayDemo, TrayWindow};
use xrack_platform_winit::run_desktop;

fn main() -> std::process::ExitCode {
    let result = run_desktop(|desktop| {
        let mut ui = TrayDemo::new();
        let open = desktop.clone();
        ui.on_open_window(move || {
            if let Err(error) = open.open(TrayWindow::new()) {
                eprintln!("{error}");
            }
        });
        let weak = ui.as_weak();
        let tray = desktop.register_tray(ui)?;
        let mut ui = weak
            .upgrade()
            .ok_or("the registered tray component was dropped")?;
        ui.on_remove_tray(move || {
            if let Err(error) = tray.close() {
                eprintln!("{error}");
            }
        });
        Ok(())
    });
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
