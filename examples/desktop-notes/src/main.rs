use std::{io, process::ExitCode};
use xrack_desktop_notes::{FieldNotes, connect};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread().build()?;
    let handle = runtime.handle().clone();
    let mut notes = FieldNotes::new();
    connect(&mut notes, move |job, complete| {
        let handle = handle.clone();
        xrack::spawn_local(async move {
            let result = handle
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
                Ok(path) => notes.invoke_open_document(path),
                Err(_) => notes.set_status("文件操作失败：需要 UTF-8 文件路径".into()),
            }
        }
        desktop.open(notes)?;
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
