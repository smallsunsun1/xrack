use form_ui::{FormDemo, PasswordDemo, ValidationDemo};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--password") => xrack_platform_winit::run(PasswordDemo::new()),
        Some("--validation") => xrack_platform_winit::run(ValidationDemo::new()),
        _ => xrack_platform_winit::run(FormDemo::new()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
