use delegates_ui::{DelegatesDemo, SwipeDemo};

fn main() -> std::process::ExitCode {
    let result = if std::env::args().nth(1).as_deref() == Some("--swipe") {
        let mut ui = SwipeDemo::new();
        ui.set_messages(
            (0..50_000)
                .map(|index| format!("邮件 {index:05}"))
                .collect(),
        );
        xrack_platform_winit::run(ui)
    } else {
        run_delegates()
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_delegates() -> Result<(), String> {
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
    xrack_platform_winit::run(ui)
}
