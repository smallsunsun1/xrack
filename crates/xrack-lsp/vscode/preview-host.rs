// This file is copied into the editor's temporary Cargo project.
mod ui {
    include!("ui.rs");
}

use std::io::{self, BufRead, Write};
use xrack_layout::Point;
use xrack_renderer_femtovg::{Renderer, preview::PreviewScene};
use xrack_runtime::Component;

fn send_frame(png: &[u8], output: &mut impl Write) -> Result<(), String> {
    let length = u32::try_from(png.len()).map_err(|error| error.to_string())?;
    output
        .write_all(&length.to_be_bytes())
        .and_then(|()| output.write_all(png))
        .and_then(|()| output.flush())
        .map_err(|error| error.to_string())
}

fn component(name: &str) -> Result<Box<dyn Component>, String> {
    ui::__xrack_preview_component(name)
        .ok_or_else(|| format!("component {name:?} is no longer available"))
}

fn run() -> Result<(), String> {
    let name = std::env::args().nth(1).ok_or("missing component name")?;
    let mut preview = PreviewScene::new(component(&name)?);
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut output = io::stdout().lock();
    send_frame(preview.render(&mut renderer)?, &mut output)?;
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|error| error.to_string())?;
        let (command, args) = line.split_once('\t').ok_or("invalid preview command")?;
        match command {
            "select" => preview = PreviewScene::new(component(args)?),
            "click" => {
                let (x, y) = args.split_once('\t').ok_or("invalid click position")?;
                let coordinate = |value: &str| {
                    value
                        .parse::<f32>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .ok_or("invalid click position")
                };
                preview.click(Point {
                    x: coordinate(x)?,
                    y: coordinate(y)?,
                });
            }
            _ => return Err("unknown preview command".into()),
        }
        send_frame(preview.render(&mut renderer)?, &mut output)?;
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("XRack preview: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
