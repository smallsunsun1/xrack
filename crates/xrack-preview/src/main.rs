//! Long-lived offscreen preview host used by the VS Code extension.
mod dialog;
mod windows;

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
    path::PathBuf,
};
use windows::Windows;
use xrack_compiler::PreviewModule;
use xrack_interpreter::{Error, Instance, Program};
use xrack_layout::{ActivationKey, EditAction, NavigationKey, Point};
use xrack_renderer_femtovg::{
    Renderer,
    preview::{PointerContact, PreviewScene},
};
use xrack_runtime::{CursorShape, KeyEvent, KeyModifiers, PointerButton, PointerEvent};

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum SplitPhase {
    Start,
    Move,
    Finish,
    Cancel,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum PointerPhase {
    Press,
    Move,
    Release,
    Cancel,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ContactButton {
    #[default]
    Primary,
    Secondary,
    Middle,
}

impl From<ContactButton> for PointerButton {
    fn from(button: ContactButton) -> Self {
        match button {
            ContactButton::Primary => Self::Primary,
            ContactButton::Secondary => Self::Secondary,
            ContactButton::Middle => Self::Middle,
        }
    }
}

fn primary_contact() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum DelayKey {
    Enter,
    Space,
}

impl From<DelayKey> for ActivationKey {
    fn from(key: DelayKey) -> Self {
        match key {
            DelayKey::Enter => Self::Enter,
            DelayKey::Space => Self::Space,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
enum CompositionPhase {
    Start,
    Update { text: String },
    Commit { text: String },
    Cancel,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request {
    Load {
        filename: String,
        text: String,
        #[serde(default)]
        buffers: BTreeMap<PathBuf, String>,
        component: Option<String>,
        scale: Option<f32>,
    },
    LoadMlir {
        filename: PathBuf,
        component: Option<String>,
    },
    Activate,
    Frame,
    RestoreWindow,
    Scale {
        scale: f32,
    },
    SelectWindow {
        window: Option<u64>,
    },
    NativeDialogResult {
        id: u64,
        #[serde(flatten)]
        result: dialog::Response,
    },
    ClipboardResult {
        id: u64,
        success: bool,
        #[serde(default)]
        text: String,
    },
    Escape,
    Key {
        key: String,
        text: String,
        pressed: bool,
        repeat: bool,
        control: bool,
        shift: bool,
        alt: bool,
        meta: bool,
        primary: bool,
    },
    Click {
        x: f32,
        y: f32,
    },
    ContextMenu {
        x: f32,
        y: f32,
    },
    Hover {
        x: f32,
        y: f32,
    },
    Leave,
    Pointer {
        phase: PointerPhase,
        x: f32,
        y: f32,
        #[serde(default)]
        click: bool,
        #[serde(default)]
        pan: bool,
        #[serde(default)]
        contact: u64,
        #[serde(default = "primary_contact")]
        primary: bool,
        #[serde(default)]
        touch: bool,
        #[serde(default)]
        button: ContactButton,
        #[serde(default)]
        control: bool,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        alt: bool,
        #[serde(default)]
        meta: bool,
    },
    FileDrag {
        sequence: u64,
        x: f32,
        y: f32,
        paths: Vec<String>,
        #[serde(default)]
        drop: bool,
    },
    FileDragLeave {
        sequence: u64,
    },
    DelayKey {
        key: Option<DelayKey>,
        pressed: bool,
    },
    SplitDrag {
        phase: SplitPhase,
        x: f32,
        y: f32,
        start: Option<(f32, f32)>,
    },
    Wheel {
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        control: bool,
        #[serde(default)]
        alt: bool,
        #[serde(default)]
        meta: bool,
        lines: Option<(f32, f32)>,
    },
    Edit {
        action: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        select: bool,
    },
    Composition {
        id: u64,
        #[serde(flatten)]
        phase: CompositionPhase,
    },
}

#[derive(Debug, Default, Serialize)]
struct Reply {
    components: Vec<String>,
    inputs: Vec<PathBuf>,
    component: Option<String>,
    error: Option<String>,
    unsupported: bool,
    image_length: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<[u32; 2]>,
    next_frame_ms: Option<u64>,
    table_tab_enabled: bool,
    tree_navigation_enabled: bool,
    control_navigation_enabled: bool,
    menu_open: bool,
    cursor: Option<&'static str>,
    ime_area: Option<[f32; 3]>,
    split_navigation_enabled: bool,
    delay_keyboard_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    clipboard: Option<Clipboard>,
    #[serde(skip_serializing_if = "Option::is_none")]
    native_dialog: Option<dialog::Panel>,
    #[serde(skip_serializing_if = "is_false")]
    window_closed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_minimized: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    windows: Vec<windows::Description>,
    window: Option<u64>,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Serialize)]
struct Clipboard {
    id: u64,
    text: Option<String>,
}

#[derive(Debug)]
struct PreviewError {
    message: String,
    unsupported: bool,
}

impl From<Error> for PreviewError {
    fn from(error: Error) -> Self {
        Self {
            unsupported: matches!(error, Error::Unsupported(_)),
            message: error.to_string(),
        }
    }
}

#[derive(Default)]
struct Host {
    active: Option<(Instance, Windows)>,
    pending: Option<(Instance, Windows)>,
    renderer: Option<Renderer>,
    scale: Option<f32>,
    check: bool,
}

impl Host {
    fn load(
        &mut self,
        filename: &str,
        text: &str,
        buffers: BTreeMap<PathBuf, String>,
        component: Option<String>,
    ) -> Result<(Reply, Vec<u8>), PreviewError> {
        self.pending = None;
        let preview = xrack_compiler::prepare_preview(text, filename, buffers)
            .map_err(|error| failure(error.to_string()))?;
        self.stage_module(preview, filename, component)
    }

    fn stage_module(
        &mut self,
        mut preview: PreviewModule,
        filename: &str,
        component: Option<String>,
    ) -> Result<(Reply, Vec<u8>), PreviewError> {
        let translations = preview.take_translation_catalog();
        let (program, assets) =
            Program::from_module_with_file_resources(&preview.module, filename, translations)?;
        preview.inputs.extend(assets);
        self.stage(
            program,
            preview.components,
            preview.inputs.into_iter().collect(),
            component,
        )
    }

    fn stage(
        &mut self,
        program: Program,
        components: Vec<String>,
        inputs: Vec<PathBuf>,
        component: Option<String>,
    ) -> Result<(Reply, Vec<u8>), PreviewError> {
        let selected = component
            .filter(|name| components.contains(name))
            .or_else(|| components.first().cloned())
            .ok_or_else(|| failure("this file has no component to preview"))?;
        let instance = program.create(&selected).map_err(PreviewError::from)?;
        let mut windows = Windows::new(Box::new(instance.clone()));
        instance.try_render().map_err(PreviewError::from)?;
        let png = if self.check {
            Vec::new()
        } else {
            if self.renderer.is_none() {
                self.renderer = Some(
                    pollster::block_on(Renderer::new(
                        wgpu::Instance::new(
                            wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
                        ),
                        None,
                    ))
                    .map_err(failure)?,
                );
            }
            windows
                .render(self.renderer.as_mut().unwrap(), self.scale.unwrap_or(1.0))
                .map_err(failure)?
        };
        if let Some(error) = instance.take_error() {
            return Err(error.into());
        }
        let reply = Reply {
            components,
            inputs,
            component: Some(selected),
            next_frame_ms: next_frame_ms(&windows),
            windows: windows.descriptions(),
            window: windows.selection(),
            window_minimized: windows.selected().window_status().minimized,
            size: (!png.is_empty()).then(|| windows.selected().logical_size()),
            ime_area: ime_area(windows.selected()),
            ..Reply::default()
        };
        self.pending = Some((instance, windows));
        Ok((reply, png))
    }

    fn interact(
        &mut self,
        action: impl FnOnce(&mut PreviewScene),
    ) -> Result<(Reply, Vec<u8>), PreviewError> {
        self.interact_if(|scene| {
            action(scene);
            true
        })
    }

    fn interact_if(
        &mut self,
        action: impl FnOnce(&mut PreviewScene) -> bool,
    ) -> Result<(Reply, Vec<u8>), PreviewError> {
        self.interact_with(|scene, _| action(scene))
    }

    fn interact_with(
        &mut self,
        action: impl FnOnce(&mut PreviewScene, &mut Renderer) -> bool,
    ) -> Result<(Reply, Vec<u8>), PreviewError> {
        let (windows, renderer) = match (&mut self.active, &mut self.renderer) {
            (Some((_, windows)), Some(renderer)) => (windows, renderer),
            (None, _) => return Err(failure("no preview component is loaded")),
            _ => return Err(failure("no preview renderer is active")),
        };
        let redraw = !windows.closed() && action(windows.selected_mut(), renderer);
        self.render_active(redraw)
    }

    fn active_windows(&mut self) -> Result<&mut Windows, PreviewError> {
        self.active
            .as_mut()
            .map(|(_, windows)| windows)
            .ok_or_else(|| failure("no preview component is loaded"))
    }

    fn render_active(&mut self, redraw: bool) -> Result<(Reply, Vec<u8>), PreviewError> {
        let (instance, windows) = self
            .active
            .as_mut()
            .ok_or_else(|| failure("no preview component is loaded"))?;
        if let Some(error) = instance.take_error() {
            return Err(error.into());
        }
        if windows.closed() {
            return Ok((
                Reply {
                    window_closed: true,
                    ..Reply::default()
                },
                Vec::new(),
            ));
        }
        if !redraw {
            return Ok((Reply::default(), Vec::new()));
        }
        let renderer = self
            .renderer
            .as_mut()
            .ok_or_else(|| failure("no preview renderer is active"))?;
        instance.try_render().map_err(PreviewError::from)?;
        let png = windows
            .render(renderer, self.scale.unwrap_or(1.0))
            .map_err(failure)?;
        if let Some(error) = instance.take_error() {
            return Err(error.into());
        }
        let clipboard = windows.take_clipboard_request().map(|request| Clipboard {
            id: request.id,
            text: request.text,
        });
        let native_dialog = windows.begin_native_dialog().map(dialog::Panel::from);
        let scene = windows.selected();
        Ok((
            Reply {
                clipboard,
                native_dialog,
                window_closed: windows.closed(),
                window_minimized: scene.window_status().minimized,
                next_frame_ms: next_frame_ms(windows),
                windows: windows.descriptions(),
                window: windows.selection(),
                size: (!png.is_empty()).then(|| scene.logical_size()),
                table_tab_enabled: scene.table_tab_enabled(),
                tree_navigation_enabled: scene.tree_navigation_enabled(),
                control_navigation_enabled: scene.control_navigation_enabled(),
                menu_open: scene.menu_open(),
                cursor: Some(cursor_name(scene.cursor_shape())),
                ime_area: ime_area(scene),
                split_navigation_enabled: scene.split_navigation_enabled(),
                delay_keyboard_enabled: scene.delay_keyboard_enabled(),
                ..Reply::default()
            },
            png,
        ))
    }

    fn request(&mut self, request: Request) -> Result<(Reply, Vec<u8>), PreviewError> {
        match request {
            Request::Load {
                filename,
                text,
                buffers,
                component,
                scale,
            } => {
                if let Some(scale) = scale {
                    self.scale = Some(scale);
                }
                self.load(&filename, &text, buffers, component)
            }
            Request::LoadMlir {
                filename,
                component,
            } => {
                self.pending = None;
                let source = std::fs::read_to_string(&filename)
                    .map_err(|error| failure(format!("{}: {error}", filename.display())))?;
                let filename = filename.to_string_lossy();
                let preview = xrack_compiler::prepare_mlir_preview(&source, &filename)
                    .map_err(|error| failure(error.to_string()))?;
                self.stage_module(preview, &filename, component)
            }
            Request::Activate => {
                let (instance, mut windows) = self
                    .pending
                    .take()
                    .ok_or_else(|| failure("no preview to activate"))?;
                let reply = Reply {
                    native_dialog: windows.begin_native_dialog().map(dialog::Panel::from),
                    window_closed: windows.closed(),
                    ..Reply::default()
                };
                self.active = Some((instance, windows));
                Ok((reply, Vec::new()))
            }
            Request::Frame => self.render_active(true),
            Request::RestoreWindow => self.interact(PreviewScene::request_window_restore),
            Request::Scale { scale } => {
                self.scale = Some(scale);
                self.render_active(true)
            }
            Request::SelectWindow { window } => {
                let changed = self.active_windows()?.select(window);
                self.render_active(changed)
            }
            Request::NativeDialogResult { id, result } => {
                let changed = self
                    .active_windows()?
                    .finish_native_dialog(id, result.into());
                self.render_active(changed)
            }
            Request::ClipboardResult { id, success, text } => {
                let changed = self.active_windows()?.finish_clipboard(id, success, &text);
                self.render_active(changed)
            }
            Request::Escape => self.interact(PreviewScene::escape),
            Request::Composition { id, phase } => match phase {
                CompositionPhase::Start => {
                    self.interact_with(|scene, renderer| scene.begin_text_composition(renderer, id))
                }
                CompositionPhase::Update { text } => {
                    self.interact_if(|scene| scene.update_text_composition(id, text))
                }
                CompositionPhase::Commit { text } => {
                    self.interact_if(|scene| scene.finish_text_composition(id, &text))
                }
                CompositionPhase::Cancel => {
                    self.interact_if(|scene| scene.cancel_text_composition(id))
                }
            },
            Request::Key {
                key,
                text,
                pressed,
                repeat,
                control,
                shift,
                alt,
                meta,
                primary,
            } => {
                let event = KeyEvent {
                    key,
                    text,
                    repeat,
                    modifiers: KeyModifiers {
                        control,
                        shift,
                        alt,
                        meta,
                        primary,
                    },
                };
                self.interact_with(|scene, renderer| scene.keyboard(renderer, &event, pressed))
            }
            Request::Click { x, y } if x.is_finite() && y.is_finite() => {
                self.interact(|scene| scene.click(Point { x, y }))
            }
            Request::Click { .. } => Err(failure("invalid click position")),
            Request::ContextMenu { x, y } if x.is_finite() && y.is_finite() => {
                self.interact_if(|scene| scene.open_context_menu(Some(Point { x, y })))
            }
            Request::Hover { x, y } if x.is_finite() && y.is_finite() => {
                self.interact_if(|scene| scene.hover(Point { x, y }))
            }
            Request::Leave => self.interact_if(PreviewScene::leave),
            Request::ContextMenu { .. } | Request::Hover { .. } => {
                Err(failure("invalid menu pointer position"))
            }
            Request::Pointer {
                phase,
                x,
                y,
                click,
                pan,
                contact,
                primary,
                touch,
                button,
                control,
                shift,
                alt,
                meta,
            } if x.is_finite() && y.is_finite() => {
                let point = Point { x, y };
                let contact = PointerContact {
                    id: contact,
                    primary,
                    event: PointerEvent {
                        button: if pan {
                            PointerButton::Middle
                        } else {
                            button.into()
                        },
                        is_touch: touch,
                        modifiers: KeyModifiers {
                            control,
                            shift,
                            alt,
                            meta,
                            primary: if cfg!(target_os = "macos") {
                                meta
                            } else {
                                control
                            },
                        },
                        ..Default::default()
                    },
                };
                self.interact_if(|scene| match phase {
                    PointerPhase::Press => scene.press_contact(contact, point, pan),
                    PointerPhase::Move => scene.move_contact(contact, point),
                    PointerPhase::Release => scene.finish_contact(contact, point, click, false),
                    PointerPhase::Cancel => scene.finish_contact(contact, point, false, true),
                })
            }
            Request::Pointer { .. } => Err(failure("invalid pointer position")),
            Request::FileDrag {
                sequence,
                x,
                y,
                paths,
                drop,
            } if x.is_finite() && y.is_finite() => {
                self.interact_if(|scene| scene.file_drag(sequence, Point { x, y }, paths, drop))
            }
            Request::FileDrag { .. } => Err(failure("invalid file drop position")),
            Request::FileDragLeave { sequence } => {
                self.interact_if(|scene| scene.leave_file_drag(sequence))
            }
            Request::DelayKey { key, pressed } => {
                self.interact_if(|scene| match (key.map(ActivationKey::from), pressed) {
                    (Some(key), true) => scene.press_delay_key(key),
                    (Some(key), false) => scene.release_delay_key(key),
                    (None, _) => scene.cancel_delay_key(),
                })
            }
            Request::SplitDrag { phase, x, y, start }
                if x.is_finite()
                    && y.is_finite()
                    && start.is_none_or(|(x, y)| x.is_finite() && y.is_finite()) =>
            {
                let point = Point { x, y };
                self.interact_if(|scene| match phase {
                    SplitPhase::Start => start.is_some_and(|(x, y)| {
                        scene.begin_split_drag(Point { x, y }) && scene.move_split_drag(point)
                    }),
                    SplitPhase::Move => scene.move_split_drag(point),
                    SplitPhase::Finish => {
                        scene.move_split_drag(point);
                        scene.finish_split_drag()
                    }
                    SplitPhase::Cancel => scene.finish_split_drag(),
                })
            }
            Request::SplitDrag { .. } => Err(failure("invalid split position")),
            Request::Wheel {
                x,
                y,
                dx,
                dy,
                shift,
                control,
                alt,
                meta,
                lines,
            } if [x, y, dx, dy].into_iter().all(f32::is_finite)
                && lines.is_none_or(|(x, y)| x.is_finite() && y.is_finite()) =>
            {
                let event = xrack_runtime::WheelEvent {
                    delta_x: xrack_runtime::Length(f64::from(dx)),
                    delta_y: xrack_runtime::Length(f64::from(dy)),
                    lines_x: lines.map_or(0.0, |(x, _)| f64::from(x)),
                    lines_y: lines.map_or(0.0, |(_, y)| f64::from(y)),
                    is_line: lines.is_some(),
                    modifiers: KeyModifiers {
                        shift,
                        control,
                        alt,
                        meta,
                        primary: if cfg!(target_os = "macos") {
                            meta
                        } else {
                            control
                        },
                    },
                    ..Default::default()
                };
                self.interact_with(|scene, renderer| {
                    scene.wheel_event_with_renderer(renderer, Point { x, y }, event);
                    true
                })
            }
            Request::Wheel { .. } => Err(failure("invalid wheel input")),
            Request::Edit {
                action,
                text,
                select,
            } => {
                let action = match action.as_str() {
                    "paste" => return self.interact(|scene| scene.paste(&text)),
                    "undo" => return self.interact(|scene| scene.replay_edit(false)),
                    "redo" => return self.interact(|scene| scene.replay_edit(true)),
                    "tab" => return self.interact(|scene| scene.table_tab(select)),
                    "up" => {
                        return self.interact(|scene| {
                            scene.navigate_tree_or_split(NavigationKey::Up, select)
                        });
                    }
                    "down" => {
                        return self.interact(|scene| {
                            scene.navigate_tree_or_split(NavigationKey::Down, select)
                        });
                    }
                    "page_up" => {
                        return self.interact(|scene| {
                            scene.navigate_tree_or_split(NavigationKey::PageUp, select)
                        });
                    }
                    "page_down" => {
                        return self.interact(|scene| {
                            scene.navigate_tree_or_split(NavigationKey::PageDown, select)
                        });
                    }
                    "insert" => EditAction::Insert(&text),
                    "backspace" => EditAction::Backspace,
                    "delete" => EditAction::Delete,
                    "left" => EditAction::Left(select),
                    "right" => EditAction::Right(select),
                    "home" => EditAction::Home(select),
                    "end" => EditAction::End(select),
                    "select_all" => EditAction::SelectAll,
                    _ => return Err(failure("unknown text edit action")),
                };
                self.interact(|scene| scene.edit(action))
            }
        }
    }
}

fn ime_area(scene: &PreviewScene) -> Option<[f32; 3]> {
    scene.ime_area().map(|area| [area.x, area.y, area.height])
}

fn cursor_name(shape: CursorShape) -> &'static str {
    match shape {
        CursorShape::Auto => "auto",
        CursorShape::Default => "default",
        CursorShape::Pointer => "pointer",
        CursorShape::Text => "text",
        CursorShape::Crosshair => "crosshair",
        CursorShape::Move => "move",
        CursorShape::Grab => "grab",
        CursorShape::Grabbing => "grabbing",
        CursorShape::NotAllowed => "not-allowed",
        CursorShape::Wait => "wait",
        CursorShape::Help => "help",
        CursorShape::ResizeHorizontal => "ew-resize",
        CursorShape::ResizeVertical => "ns-resize",
        CursorShape::ZoomIn => "zoom-in",
        CursorShape::ZoomOut => "zoom-out",
    }
}

fn next_frame_ms(windows: &Windows) -> Option<u64> {
    windows
        .next_frame_delay()
        .map(|delay| u64::try_from(delay.as_millis()).unwrap_or(u64::MAX))
}

fn failure(error: impl Into<String>) -> PreviewError {
    PreviewError {
        message: error.into(),
        unsupported: false,
    }
}

fn run() -> io::Result<()> {
    let mut host = Host {
        check: std::env::args().any(|arg| arg == "--check"),
        ..Host::default()
    };
    let mut output = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let result = serde_json::from_str(&line?)
            .map_err(|error| failure(error.to_string()))
            .and_then(|request| host.request(request));
        let (mut reply, png) = result.unwrap_or_else(|error| {
            (
                Reply {
                    error: Some(error.message),
                    unsupported: error.unsupported,
                    ..Reply::default()
                },
                Vec::new(),
            )
        });
        reply.image_length = png.len();
        let header = serde_json::to_vec(&reply)?;
        let length = u32::try_from(header.len()).map_err(io::Error::other)?;
        output.write_all(&length.to_be_bytes())?;
        output.write_all(&header)?;
        output.write_all(&png)?;
        output.flush()?;
    }
    Ok(())
}

fn main() -> io::Result<()> {
    run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_uses_compiler_catalog_for_reactive_locale_and_fallback() {
        use xrack_runtime::{Component, LocaleContext};

        let directory = std::env::var_os("TEST_TMPDIR")
            .map_or_else(std::env::temp_dir, PathBuf::from)
            .join(format!(
                "xrack-interpreter-localization-{}",
                std::process::id()
            ));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("app.xr");
        std::fs::write(
            directory.join("catalog.toml"),
            "default = 'en-US'\n[locales]\nen-US = 'en.ftl'\nzh-CN = 'zh.ftl'",
        )
        .unwrap();
        std::fs::write(
            directory.join("en.ftl"),
            "count = Count { $n }\nsubtitle = Shared fallback",
        )
        .unwrap();
        std::fs::write(directory.join("zh.ftl"), "count = 计数 { $n }").unwrap();
        let source = r#"
            translations "catalog.toml";
            component Caption inherits Label { text: @tr("subtitle"); }
            export component App inherits Window {
                in-out property <int> count: 4;
                title: @tr("count", n: root.count);
                Caption {}
            }
        "#;
        let mut host = Host {
            check: true,
            ..Host::default()
        };
        let (reply, _) = host
            .load(file.to_str().unwrap(), source, BTreeMap::new(), None)
            .unwrap();
        assert!(reply.inputs.iter().any(|input| input.ends_with("zh.ftl")));
        host.request(Request::Activate).unwrap();
        let ui = &host.active.as_ref().unwrap().0;
        let locale = LocaleContext::default();
        ui.set_locale_context(locale.clone());
        let scene = ui.try_render().unwrap();
        assert!(scene.text("title").unwrap().starts_with("Count"));
        locale.set_locale("zh-CN").unwrap();
        assert!(ui.scene_dirty());
        let scene = ui.try_render().unwrap();
        assert!(scene.text("title").unwrap().starts_with("计数"));
        assert_eq!(scene.children[0].text("text"), Some("Shared fallback"));
        ui.set("count", xrack_interpreter::HostValue::Int(9))
            .unwrap();
        assert!(
            ui.try_render()
                .unwrap()
                .text("title")
                .unwrap()
                .contains('9')
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_reload_keeps_previous_instance_and_pending_is_explicitly_activated() {
        let mut host = Host {
            check: true,
            ..Host::default()
        };
        let source = |count| {
            format!(
                "export component App inherits Window {{ in-out property <int> count: {count}; }}"
            )
        };
        host.load("preview.xr", &source(1), BTreeMap::new(), None)
            .unwrap();
        assert!(host.active.is_none());
        host.request(Request::Activate).unwrap();
        let value = |host: &Host| host.active.as_ref().unwrap().0.get("count").unwrap();
        assert_eq!(value(&host), xrack_interpreter::HostValue::Int(1));

        host.load("preview.xr", &source(2), BTreeMap::new(), None)
            .unwrap();
        assert_eq!(value(&host), xrack_interpreter::HostValue::Int(1));
        assert!(
            host.load(
                "preview.xr",
                "export component App {",
                BTreeMap::new(),
                None
            )
            .is_err()
        );
        assert!(host.request(Request::Activate).is_err());
        assert_eq!(value(&host), xrack_interpreter::HostValue::Int(1));
        host.load("preview.xr", &source(3), BTreeMap::new(), None)
            .unwrap();
        host.request(Request::Activate).unwrap();
        assert_eq!(value(&host), xrack_interpreter::HostValue::Int(3));
    }
}
