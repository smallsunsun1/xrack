// This file is copied into the editor's temporary Cargo project.
mod ui {
    include!("ui.rs");
}

use std::io::{self, BufRead, Write};
use xrack_layout::Point;
use xrack_renderer_femtovg::{Renderer, preview::PreviewScene};
use xrack_runtime::Component;

fn send_frame(
    preview: &mut PreviewScene,
    renderer: &mut Renderer,
    scale: f32,
    output: &mut impl Write,
) -> Result<(), String> {
    let png = preview.render_scaled(renderer, scale)?;
    let length = u32::try_from(png.len()).map_err(|error| error.to_string())?;
    output
        .write_all(&length.to_be_bytes())
        .and_then(|()| output.write_all(png))
        .map_err(|error| error.to_string())?;
    let [width, height] = preview.logical_size();
    output
        .write_all(&width.to_be_bytes())
        .and_then(|()| output.write_all(&height.to_be_bytes()))
        .and_then(|()| output.flush())
        .map_err(|error| error.to_string())
}

fn component(name: &str) -> Result<Box<dyn Component>, String> {
    ui::__xrack_preview_component(name)
        .ok_or_else(|| format!("component {name:?} is no longer available"))
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let name = args.next().ok_or("missing component name")?;
    let mut scale = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1.0);
    let mut preview = PreviewScene::new(component(&name)?);
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut output = io::stdout().lock();
    send_frame(&mut preview, &mut renderer, scale, &mut output)?;
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|error| error.to_string())?;
        let (command, args) = line.split_once('\t').ok_or("invalid preview command")?;
        match command {
            "select" => preview = PreviewScene::new(component(args)?),
            "scale" => scale = args.parse().map_err(|_| "invalid preview scale")?,
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
        send_frame(&mut preview, &mut renderer, scale, &mut output)?;
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(io::stderr)
        .finish();
    match tracing::subscriber::with_default(subscriber, run) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("XRack preview: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
