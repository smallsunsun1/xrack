use tree_ui::{TreeBranchesDemo, TreeCommandsDemo, TreeDemo, VisibilityTreeDemo};

fn main() -> std::process::ExitCode {
    let result = if std::env::args().nth(1).as_deref() == Some("--commands") {
        xrack_platform_winit::run(TreeCommandsDemo::new())
    } else if std::env::args().nth(1).as_deref() == Some("--branches") {
        let mut ui = TreeBranchesDemo::new();
        ui.invoke_initialize();
        xrack_platform_winit::run(ui)
    } else if std::env::args().nth(1).as_deref() == Some("--visibility") {
        let mut ui = VisibilityTreeDemo::new();
        let records = ui.model_records();
        ui.on_toggle_branch(move || {
            if let Some(mut branch) = records.row(1) {
                branch.visibility.shown = !branch.visibility.shown;
                records.set_row(1, branch);
            }
        });
        ui.invoke_initialize();
        xrack_platform_winit::run(ui)
    } else {
        let mut ui = TreeDemo::new();
        let records = ui.model_records();
        ui.on_sort_requested(move || {
            records.sort_by(|a, b| a.label.cmp(&b.label));
        });
        xrack_platform_winit::run(ui)
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
