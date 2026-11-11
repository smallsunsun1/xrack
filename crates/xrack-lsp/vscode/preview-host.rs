// This file is copied into the editor's temporary Cargo project.
mod ui {
    include!("ui.rs");
}

use std::{
    io::{self, BufRead, Write},
    rc::Rc,
};
use xrack_layout::{ControlInput, Interaction, LayoutNode, Point, Size};
use xrack_renderer_femtovg::Renderer;
use xrack_runtime::{Component, Node, ScrollRequests};

struct Preview {
    renderer: Renderer,
    component: Box<dyn Component>,
    input: Interaction,
    scroll: ScrollRequests,
    frame: Option<(Rc<Node>, LayoutNode)>,
    png: Vec<u8>,
}

impl Preview {
    fn select(&mut self, name: &str) -> Result<(), String> {
        self.component = component(name)?;
        self.input = Interaction::default();
        self.scroll = ScrollRequests::default();
        self.frame = None;
        Ok(())
    }

    fn click(&mut self, point: Point) {
        let Some((scene, layout)) = &self.frame else {
            return;
        };
        let _scroll = self.scroll.enter();
        self.input.pointer_move(scene, layout, point);
        let action = if self.input.menu_press(scene, layout) {
            self.input.menu_release(scene, layout)
        } else if self.input.choice_open() {
            self.input.choice_release(scene, layout)
        } else {
            self.input.pointer_press_in_scene(scene, layout);
            self.input.control_part_release(scene, layout).or_else(|| {
                let target = self.input.pointer_release_target()?;
                let action = self
                    .input
                    .dialog_button(scene, layout, &target)
                    .or_else(|| self.input.select_group_option(scene, layout, &target))
                    .or_else(|| self.input.text_link_activation(scene, layout, &target));
                if action.is_none()
                    && !self.input.open_menu_bar(scene, layout, target.clone())
                    && !self
                        .input
                        .open_choice_at_pointer(scene, layout, target.clone())
                {
                    self.component.dispatch_at(scene, &target.path, "clicked");
                }
                action
            })
        };
        if let Some(ControlInput { path, action }) = action {
            self.component.control_at(scene, &path, action);
        }
    }

    fn send_frame(&mut self, output: &mut impl Write) -> Result<(), String> {
        let scene = self.component.render_shared();
        let dimension = |name, default| {
            scene
                .length(name)
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(default)
                .ceil()
                .clamp(1.0, 2048.0) as u32
        };
        let (width, height) = (dimension("width", 640.0), dimension("height", 480.0));
        let size = Size::new(width as f32, height as f32);
        let (mut scene, mut layout) =
            self.renderer
                .prepare_scene(&self.component, scene, size, &self.input);
        if self
            .input
            .apply_scroll_requests(&scene, &layout, self.scroll.drain())
        {
            (scene, layout) = self.renderer.prepare_scene(
                &self.component,
                self.component.render_shared(),
                size,
                &self.input,
            );
        }
        self.input.reconcile(&scene, &layout);
        let snapshot =
            self.renderer
                .snapshot_prepared(&scene, layout, width, height, 1.0, &self.input)?;
        self.png.clear();
        snapshot.write_png(&mut self.png)?;
        self.frame = Some((scene, snapshot.layout));
        let length = u32::try_from(self.png.len()).map_err(|error| error.to_string())?;
        output
            .write_all(&length.to_be_bytes())
            .and_then(|()| output.write_all(&self.png))
            .and_then(|()| output.flush())
            .map_err(|error| error.to_string())
    }
}

fn component(name: &str) -> Result<Box<dyn Component>, String> {
    ui::__xrack_preview_component(name)
        .ok_or_else(|| format!("component {name:?} is no longer available"))
}

fn run() -> Result<(), String> {
    let name = std::env::args().nth(1).ok_or("missing component name")?;
    let mut preview = Preview {
        component: component(&name)?,
        renderer: pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))?,
        input: Interaction::default(),
        scroll: ScrollRequests::default(),
        frame: None,
        png: Vec::new(),
    };
    let mut output = io::stdout().lock();
    preview.send_frame(&mut output)?;
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|error| error.to_string())?;
        let (command, args) = line.split_once('\t').ok_or("invalid preview command")?;
        match command {
            "select" => preview.select(args)?,
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
        preview.send_frame(&mut output)?;
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
