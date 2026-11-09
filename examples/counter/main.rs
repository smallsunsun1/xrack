use counter_ui::{Counter, DesktopCounter, SecondaryWindowDemo};
use std::{
    cell::{Cell, OnceCell},
    rc::Rc,
};
use xrack_platform_winit::{Desktop, DesktopWindow, UiSender, run_desktop, spawn_local};
use xrack_runtime::VecModel;

fn open_counter(
    desktop: &Desktop,
    sequence: Rc<Cell<u32>>,
    activity: VecModel<String>,
) -> Result<(), String> {
    let index = sequence.get() + 1;
    sequence.set(index);
    let mut counter = DesktopCounter::new();
    counter.set_caption(format!("XRack · 窗口 {index}"));
    counter.bind_model_activity(activity.clone());
    let shared = activity.clone();
    counter.on_publish(move || {
        shared.set_row(0, format!("窗口 {index} 更新了共享状态"));
    });
    let launcher = desktop.clone();
    counter.on_open_window(move || {
        if let Err(error) = open_counter(&launcher, sequence.clone(), activity.clone()) {
            eprintln!("{error}");
        }
    });

    let owner = Rc::new(OnceCell::<DesktopWindow>::new());
    let closer = owner.clone();
    counter.on_close_window(move || {
        if let Some(window) = closer.get() {
            let _ = window.close();
        }
    });
    let window = desktop.open_with_messages(counter, |ui, sender: UiSender<i64>| {
        ui.on_add_later(move || {
            let sender = sender.clone();
            let _ = spawn_local(async move {
                let _ = sender.try_send(10);
            });
        });
        |ui, delta| ui.set_count(ui.get_count() + delta)
    })?;
    assert!(owner.set(window).is_ok());
    Ok(())
}

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--windows") => run_desktop(|desktop| {
            let sequence = Rc::new(Cell::new(0));
            let activity = VecModel::from(vec!["等待共享状态更新".into()]);
            open_counter(desktop, sequence.clone(), activity.clone())?;
            open_counter(desktop, sequence, activity)
        }),
        Some("--secondary") => xrack_platform_winit::run(SecondaryWindowDemo::new()),
        _ => xrack_platform_winit::run(Counter::new()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
