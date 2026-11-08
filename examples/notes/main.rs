use notes_ui::NotesDemo;

fn main() -> std::process::ExitCode {
    match xrack_platform_winit::run(NotesDemo::new()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
