use list_ui::{DirectoryItem, GridDemo, ListDemo, StandardListDemo};

fn records() -> Vec<DirectoryItem> {
    (0..50_000)
        .map(|index| DirectoryItem {
            label: format!("条目 {index:05}"),
            rank: index,
        })
        .collect()
}

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--standard") => {
            let mut ui = StandardListDemo::new();
            ui.set_records(records());
            ui.set_jump_index(49_999);
            xrack_platform_winit::run(ui)
        }
        Some("--grid") => {
            let mut ui = GridDemo::new();
            ui.set_records(records());
            ui.set_jump_index(49_999);
            xrack_platform_winit::run(ui)
        }
        _ => xrack_platform_winit::run(ListDemo::new()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
