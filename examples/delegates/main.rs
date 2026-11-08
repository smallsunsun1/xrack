use delegates_ui::DelegatesDemo;

fn main() -> std::process::ExitCode {
    let mut ui = DelegatesDemo::new();
    ui.set_records((1..=1000).map(|index| format!("记录 {index}")).collect());
    let records = ui.model_records();
    let insert = records.clone();
    ui.on_insert_first(move || {
        insert.insert(0, "新记录".into());
    });
    ui.on_remove_first(move || {
        records.remove(0);
    });
    match xrack_platform_winit::run(ui) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
