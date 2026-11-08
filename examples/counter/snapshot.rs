use counter_ui::Counter;
use std::path::PathBuf;
use xrack_layout::{Interaction, LayoutNode, Point};
use xrack_renderer_femtovg::Renderer;

fn render() -> Result<(), String> {
    let path = PathBuf::from(std::env::args().nth(1).unwrap_or("counter.png".into()));
    let mut ui = Counter::new();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    println!("GPU: {:?}", renderer.adapter.get_info());
    let scene = ui.render();
    let mut input = Interaction::default();
    let before = renderer.snapshot(&scene, 320, 180, 1.0, &input)?;
    before.save_png(&path)?;
    let button: &LayoutNode = &before.layout.children[0].children[2];
    input.pointer_move(
        &scene,
        &before.layout,
        Point {
            x: button.bounds.x + button.bounds.width * 0.5,
            y: button.bounds.y + button.bounds.height * 0.5,
        },
    );
    input.pointer_press();
    let id = input
        .pointer_release()
        .ok_or("hit testing did not produce a click")?;
    if !ui.dispatch(id, "clicked") {
        return Err("generated component did not handle clicked".into());
    }
    let after = renderer.snapshot(&ui.render(), 320, 180, 1.0, &input)?;
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let clicked = path.with_file_name(format!("{stem}-clicked.png"));
    after.save_png(&clicked)?;
    println!(
        "count={}, doubled={}; saved {} and {}",
        ui.get_count(),
        ui.get_doubled(),
        path.display(),
        clicked.display()
    );
    Ok(())
}

fn main() -> std::process::ExitCode {
    match render() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
