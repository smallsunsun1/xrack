use std::{io, process::ExitCode};
use xrack_inspection_workbench::{InspectionWorkbench, Project, connect};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread().build()?;
    let worker = runtime.handle().clone();
    let mut ui = InspectionWorkbench::new();
    connect(&mut ui, Project::sample(10_000), move |job, complete| {
        let worker = worker.clone();
        xrack::spawn_local(async move {
            let result = worker
                .spawn_blocking(move || job.run())
                .await
                .map_err(io::Error::other)
                .and_then(|result| result);
            complete(result);
        })
        .map_err(io::Error::other)?;
        Ok(())
    });
    let initial_file = std::env::args_os().nth(1);
    xrack::run_desktop(move |desktop| {
        if let Some(path) = initial_file {
            match path.into_string() {
                Ok(path) => ui.invoke_read_file(path),
                Err(_) => ui.set_message("项目路径需要使用 UTF-8".into()),
            }
        }
        desktop.open(ui)?;
        Ok(())
    })?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
