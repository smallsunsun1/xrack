use crate::{ReloadableInstance, live::EditorSources};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use xrack_runtime::WeakComponent;

const MAX_MESSAGE: usize = 4 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(untagged)]
enum Update {
    Sources {
        version: u32,
        buffers: BTreeMap<PathBuf, String>,
    },
    Inspection {
        version: u32,
        inspection: Inspection,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Inspection {
    Attach,
    Select { key: String },
    Pick { active: bool },
}

struct Connection {
    editor: EditorSources,
    active: Arc<AtomicBool>,
    ui: WeakComponent<ReloadableInstance>,
    inspecting: bool,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.editor.replace(BTreeMap::new());
        if self.inspecting {
            let _ = self
                .ui
                .upgrade_in_event_loop(|ui| ui.inspector().attach(false));
        }
        self.active.store(false, Ordering::Release);
    }
}

// Source buffers stay in memory, and inspection messages affect the debug overlay.
pub(crate) fn serve(
    listener: UnixListener,
    editor: EditorSources,
    alive: Weak<()>,
    ui: WeakComponent<ReloadableInstance>,
) {
    let active = Arc::new(AtomicBool::new(false));
    while alive.strong_count() != 0 {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if active.swap(true, Ordering::AcqRel) {
                    let _ =
                        write_error(&mut stream, "Another editor is already syncing to this app");
                    continue;
                }
                let mut connection = Connection {
                    editor: editor.clone(),
                    active: active.clone(),
                    ui: ui.clone(),
                    inspecting: false,
                };
                let alive = alive.clone();
                let _ = thread::Builder::new()
                    .name("xrack-editor-buffers".into())
                    .spawn(move || {
                        if let Err(error) = receive(&mut stream, &mut connection, alive) {
                            let _ = write_error(&mut stream, &error.to_string());
                        }
                        drop(connection);
                    });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return,
        }
    }
}

fn receive(
    stream: &mut UnixStream,
    connection: &mut Connection,
    alive: Weak<()>,
) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    stream.set_write_timeout(Some(Duration::from_millis(200)))?;
    stream.write_all(b"{\"version\":1,\"inspection\":true}\n")?;
    let mut input = BufReader::new(stream);
    let mut data = Vec::new();
    while alive.strong_count() != 0 {
        let read = input
            .by_ref()
            .take((MAX_MESSAGE - data.len()) as u64)
            .read_until(b'\n', &mut data);
        match read {
            Ok(0) => return Ok(()),
            Ok(_) => {
                if data.last() != Some(&b'\n') {
                    return Err(io::Error::other(
                        "Incomplete or oversized XRack editor update",
                    ));
                }
                match serde_json::from_slice(&data)? {
                    Update::Sources {
                        version: 1,
                        buffers,
                    } if buffers.keys().all(|path| {
                        path.is_absolute() && path.extension().is_some_and(|ext| ext == "xr")
                    }) =>
                    {
                        connection.editor.replace(buffers)
                    }
                    Update::Inspection {
                        version: 1,
                        inspection,
                    } => {
                        let attaching = matches!(inspection, Inspection::Attach);
                        if connection.inspecting || attaching {
                            connection
                                .ui
                                .upgrade_in_event_loop(move |ui| {
                                    let inspector = ui.inspector();
                                    match inspection {
                                        Inspection::Attach if !inspector.attached() => {
                                            inspector.attach(true)
                                        }
                                        Inspection::Select { key } => inspector.select(&key),
                                        Inspection::Pick { active } => {
                                            inspector.set_picking(active)
                                        }
                                        _ => {}
                                    }
                                })
                                .map_err(|error| io::Error::other(error.to_string()))?;
                            connection.inspecting = true;
                        }
                    }
                    _ => return Err(io::Error::other("Invalid XRack editor update")),
                }
                data.clear();
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_error(stream: &mut UnixStream, error: &str) -> io::Result<()> {
    serde_json::to_writer(&mut *stream, &serde_json::json!({"error": error}))?;
    stream.write_all(b"\n")
}
