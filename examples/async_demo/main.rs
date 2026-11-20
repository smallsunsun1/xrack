use async_ui::AsyncDemo;
use search_completion::{
    Completion, completion, finished, property_write, property_written, request_span, started,
};
use std::{cell::Cell, process::ExitCode, rc::Rc, thread, time::Duration};
use tokio::{
    runtime::{Builder, Handle},
    time::sleep,
};
use trace_capture::Capture;
use tracing::Instrument;
use tracing_subscriber::{Layer, filter::filter_fn, layer::SubscriberExt, util::SubscriberInitExt};
use xrack_platform_winit::{run, spawn_local};
use xrack_runtime::{
    Component, WindowInspector,
    ui_trace::{Buffer, TARGET},
};

fn setup(ui: &mut AsyncDemo, runtime: Handle, inspector: Option<WindowInspector>) {
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
        if let Err(error) = spawn_local(async move {
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
                        println!("Tokio timer completed, local future resumed on the UI thread");
                    }
                    Err(error) => ui.set_local_status(format!("Tokio 计时任务结束：{error}")),
                }
            }
        }) {
            eprintln!("could not start XRack local task: {error}");
        }
    });

    let weak = ui.as_weak();
    let current_request = Rc::new(Cell::new(0_usize));
    ui.on_search(move |query| {
        let Some(mut ui) = weak.upgrade() else {
            return;
        };
        let request = current_request.get() + 1;
        current_request.set(request);
        let span = request_span(request, ui.source_generation());
        span.in_scope(|| {
            started();
            property_write("loading", || ui.set_loading(true));
        });

        let weak = weak.clone();
        let current_request = current_request.clone();
        let background = runtime.clone();
        let search_query = query.clone();
        let job = runtime.spawn(
            async move {
                sleep(Duration::from_millis(350)).await;
                tracing::debug!(target: TARGET, event = "search_background_ready");
                (1..=12)
                    .map(|index| format!("{search_query} · 结果 {index}"))
                    .collect::<Vec<_>>()
            }
            .instrument(span.clone()),
        );

        let update_inspector = inspector.clone();
        let detail_span = span.clone();
        if let Err(error) = spawn_local(
            async move {
                let result = job.await;
                {
                    let Some(mut ui) = weak.upgrade() else {
                        return;
                    };
                    let generation = ui.source_generation();
                    let decision =
                        completion(request, current_request.get(), &query, &ui.get_query());
                    if matches!(decision, Completion::Superseded) {
                        finished("superseded", generation);
                        return;
                    }
                    property_write("loading", || ui.set_loading(false));
                    if let Some(inspector) = &update_inspector {
                        inspector.trace_update_from(request as u64);
                    }
                    if matches!(decision, Completion::QueryChanged) {
                        finished("query_changed", generation);
                        return;
                    }
                    match result {
                        Ok(rows) => {
                            property_write("suggestions", || {
                                ui.set_suggestions(rows.iter().take(8).cloned().collect())
                            });
                            property_write("results", || ui.set_results(rows));
                            property_write("result-query", || ui.set_result_query(query.clone()));
                            finished("applied", generation);
                        }
                        Err(error) => {
                            finished("error", generation);
                            eprintln!("could not finish search: {error}");
                            return;
                        }
                    }
                }

                let detail = background.spawn(
                    async move {
                        sleep(Duration::from_millis(700)).await;
                        format!("{query} · 结果 2 已更新")
                    }
                    .instrument(detail_span),
                );
                let Ok(text) = detail.await else {
                    return;
                };
                if request == current_request.get()
                    && let Some(ui) = weak.upgrade()
                    && ui.get_result_query() == ui.get_query()
                    && ui.model_results().set_row(1, text.clone())
                {
                    property_written("results");
                    if ui.model_suggestions().set_row(1, text) {
                        property_written("suggestions");
                    }
                    tracing::debug!(
                        target: TARGET,
                        event = "search_detail_applied",
                        processed_generation = ui.source_generation(),
                    );
                    if let Some(inspector) = &update_inspector {
                        inspector.trace_update_from(request as u64);
                    }
                }
            }
            .instrument(span.clone()),
        ) {
            span.in_scope(|| {
                property_write("loading", || ui.set_loading(false));
                finished("error", ui.source_generation());
                if let Some(inspector) = &inspector {
                    inspector.trace_update_from(request as u64);
                }
            });
            eprintln!("{error}");
        }
    });
}

fn main() -> ExitCode {
    let trace = std::env::var_os("XRACK_TRACE_UI")
        .is_some_and(|value| value == "1")
        .then(Buffer::default);
    if let Some(buffer) = &trace
        && let Err(error) = tracing_subscriber::registry()
            .with(
                Capture(buffer.clone())
                    .with_filter(filter_fn(|metadata| metadata.target() == TARGET)),
            )
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_filter(filter_fn(|metadata| metadata.target() == TARGET)),
            )
            .try_init()
    {
        eprintln!("could not start XRack tracing: {error}");
        return ExitCode::FAILURE;
    }
    let runtime = match Builder::new_multi_thread().enable_time().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("could not start Tokio: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut ui = AsyncDemo::new();
    let inspector = trace.and_then(|buffer| {
        let inspector = ui.window_inspector(&ui.render_shared())?;
        inspector.set_trace_buffer(buffer);
        Some(inspector)
    });
    setup(&mut ui, runtime.handle().clone(), inspector);
    match run(ui) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
