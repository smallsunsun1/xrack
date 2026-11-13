use async_ui::AsyncDemo;
use std::{cell::Cell, process::ExitCode, rc::Rc, thread, time::Duration};
use tokio::{
    runtime::{Builder, Handle},
    time::sleep,
};
use xrack_platform_winit::{run, spawn_local};

fn setup(ui: &mut AsyncDemo, runtime: Handle) {
    let weak = ui.as_weak();
    let generation = Rc::new(Cell::new(0_u64));
    let timer_runtime = runtime.clone();
    ui.on_start_local(move || {
        let weak = weak.clone();
        let generation = generation.clone();
        let task = generation.get() + 1;
        generation.set(task);
        let ui_thread = thread::current().id();
        let timer = timer_runtime.spawn(async { sleep(Duration::from_millis(500)).await });
        drop(
            spawn_local(async move {
                if let Some(mut ui) = weak.upgrade() {
                    ui.set_local_status("UI Future 已挂起，界面仍可操作".into());
                }
                let result = timer.await;
                assert_eq!(thread::current().id(), ui_thread);
                if generation.get() == task
                    && let Some(mut ui) = weak.upgrade()
                {
                    match result {
                        Ok(()) => {
                            ui.set_local_status("Tokio 计时结束，UI Future 已在主线程恢复".into());
                            println!(
                                "Tokio timer completed, local future resumed on the UI thread"
                            );
                        }
                        Err(error) => ui.set_local_status(format!("Tokio 计时任务结束：{error}")),
                    }
                }
            })
            .expect("XRack event loop is active"),
        );
    });

    let weak = ui.as_weak();
    let current_request = Rc::new(Cell::new(0_u64));
    ui.on_search(move |query| {
        let Some(mut ui) = weak.upgrade() else {
            return;
        };
        let request = current_request.get() + 1;
        current_request.set(request);
        ui.set_loading(true);

        let weak = weak.clone();
        let current_request = current_request.clone();
        let background = runtime.clone();
        let search_query = query.clone();
        let job = runtime.spawn(async move {
            sleep(Duration::from_millis(350)).await;
            (1..=12)
                .map(|index| format!("{search_query} · 结果 {index}"))
                .collect::<Vec<_>>()
        });

        if let Err(error) = spawn_local(async move {
            let result = job.await;
            {
                let Some(mut ui) = weak.upgrade() else {
                    return;
                };
                if request != current_request.get() {
                    return;
                }
                ui.set_loading(false);
                if query != ui.get_query() {
                    return;
                }
                match result {
                    Ok(rows) => {
                        ui.set_suggestions(rows.iter().take(8).cloned().collect());
                        ui.set_results(rows);
                        ui.set_result_query(query.clone());
                    }
                    Err(error) => {
                        eprintln!("could not finish search: {error}");
                        return;
                    }
                }
            }

            let detail = background.spawn(async move {
                sleep(Duration::from_millis(700)).await;
                format!("{query} · 结果 2 已更新")
            });
            let Ok(text) = detail.await else {
                return;
            };
            if request == current_request.get()
                && let Some(ui) = weak.upgrade()
                && ui.get_result_query() == ui.get_query()
                && ui.model_results().set_row(1, text.clone())
            {
                ui.model_suggestions().set_row(1, text);
            }
        }) {
            ui.set_loading(false);
            eprintln!("{error}");
        }
    });
}

fn main() -> ExitCode {
    let runtime = match Builder::new_multi_thread().enable_time().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("could not start Tokio: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut ui = AsyncDemo::new();
    setup(&mut ui, runtime.handle().clone());
    match run(ui) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
