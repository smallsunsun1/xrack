use async_ui::AsyncDemo;
use std::{cell::Cell, process::ExitCode, rc::Rc, thread, time::Duration};
use tokio::{
    runtime::{Builder, Handle},
    time::sleep,
};
use xrack_platform_winit::{UiSender, run_with_messages, spawn_local};

enum Message {
    Search(String),
    Loaded {
        request: u64,
        query: String,
        rows: Vec<String>,
    },
    UpdateRow {
        request: u64,
        index: usize,
        text: String,
    },
}

fn setup(
    ui: &mut AsyncDemo,
    sender: UiSender<Message>,
    runtime: Handle,
) -> impl FnMut(&mut AsyncDemo, Message) + use<> {
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

    let intent = sender.clone();
    ui.on_search(move |query| {
        if let Err(error) = intent.try_send(Message::Search(query)) {
            eprintln!("could not start search: {error}");
        }
    });

    ui.invoke_start_local();
    ui.invoke_search(ui.get_query());
    let mut current_request = 0;
    move |ui, message| match message {
        Message::Search(query) => {
            current_request += 1;
            let request = current_request;
            let sender = sender.clone();
            ui.set_loading(true);
            runtime.spawn(async move {
                sleep(Duration::from_millis(350)).await;
                let rows = (1..=12)
                    .map(|index| format!("{query} · 结果 {index}"))
                    .collect();
                if let Err(error) = sender.try_send(Message::Loaded {
                    request,
                    query: query.clone(),
                    rows,
                }) {
                    eprintln!("could not deliver search results: {error}");
                    return;
                }
                sleep(Duration::from_millis(700)).await;
                if let Err(error) = sender.try_send(Message::UpdateRow {
                    request,
                    index: 1,
                    text: format!("{query} · 结果 2 已更新"),
                }) {
                    eprintln!("could not deliver row update: {error}");
                }
            });
        }
        Message::Loaded {
            request,
            query,
            rows,
        } if request == current_request && query == ui.get_query() => {
            println!(
                "search {request} committed {} results on the UI thread",
                rows.len()
            );
            ui.set_suggestions(rows.iter().take(8).cloned().collect());
            ui.set_results(rows);
            ui.set_result_query(query);
            ui.set_loading(false);
        }
        Message::UpdateRow {
            request,
            index,
            text,
        } if request == current_request => {
            if ui.model_results().set_row(index, text.clone()) {
                ui.model_suggestions().set_row(index, text);
                println!("search {request} updated row {index} on the UI thread");
            }
        }
        Message::Loaded { .. } | Message::UpdateRow { .. } => {}
    }
}

fn main() -> ExitCode {
    let runtime = match Builder::new_multi_thread().enable_time().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("could not start Tokio: {error}");
            return ExitCode::FAILURE;
        }
    };
    let handle = runtime.handle().clone();
    match run_with_messages(AsyncDemo::new(), move |ui, sender| {
        setup(ui, sender, handle)
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
