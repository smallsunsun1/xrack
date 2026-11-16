use crate::{
    ReloadableInstance,
    live::EditorSources,
    live_inspect::{ModelQuery, model_page, snapshot},
};
use serde::Deserialize;
use serde_json::{Value as Json, json};
use std::{
    env, fs, io,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{fs::DirBuilderExt, net::UnixListener},
    path::{Path, PathBuf},
    process,
    sync::{Arc, Weak, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use xrack_runtime::{InspectionInput, RecordedAction, WeakComponent};

const POLL: Duration = Duration::from_millis(20);
const TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct Session {
    socket_dir: PathBuf,
    descriptor: PathBuf,
    _lifetime: Arc<()>,
}

impl Session {
    pub(crate) fn open(
        project: &Path,
        source: &Path,
        component: &str,
        ui: WeakComponent<ReloadableInstance>,
        editor: EditorSources,
    ) -> io::Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros();
        let pid = process::id();
        let id = format!("{pid}-{stamp:x}");
        let discovery = project.join(".xrack-live");
        fs::create_dir_all(&discovery)?;
        let socket_dir = env::temp_dir().join(format!("xra-{id}"));
        fs::DirBuilder::new().mode(0o700).create(&socket_dir)?;
        let session = Self {
            socket_dir,
            descriptor: discovery.join(format!("{id}.json")),
            _lifetime: Arc::new(()),
        };
        let socket = session.socket_dir.join("s");
        let listener = UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let editor_socket = session.socket_dir.join("e");
        let editor_listener = UnixListener::bind(&editor_socket)?;
        editor_listener.set_nonblocking(true)?;
        let query_socket = session.socket_dir.join("q");
        let query_listener = UnixListener::bind(&query_socket)?;
        query_listener.set_nonblocking(true)?;
        let metadata = json!({
            "version": 1,
            "pid": pid,
            "component": component,
            "source": source,
            "socket": socket,
            "editorSocket": editor_socket,
            "querySocket": query_socket,
        });
        let mut file = fs::File::create_new(&session.descriptor)?;
        serde_json::to_writer(&mut file, &metadata)?;
        file.write_all(b"\n")?;
        let alive = Arc::downgrade(&session._lifetime);
        let query_alive = alive.clone();
        let query_ui = ui.clone();
        let query_editor = editor.clone();
        thread::Builder::new()
            .name("xrack-inspection-queries".into())
            .spawn(move || serve_queries(query_listener, query_ui, query_alive, query_editor))?;
        let editor_alive = alive.clone();
        let sources = editor.clone();
        let editor_ui = ui.clone();
        thread::Builder::new()
            .name("xrack-editor-sync".into())
            .spawn(move || {
                crate::live_editor::serve(editor_listener, sources, editor_alive, editor_ui)
            })?;
        thread::Builder::new()
            .name("xrack-read-only-inspector".into())
            .spawn(move || serve(listener, ui, alive, editor))?;
        eprintln!(
            "XRack read-only inspection: {}",
            session.descriptor.display()
        );
        Ok(session)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.descriptor);
        let _ = fs::remove_file(self.socket_dir.join("s"));
        let _ = fs::remove_file(self.socket_dir.join("e"));
        let _ = fs::remove_file(self.socket_dir.join("q"));
        let _ = fs::remove_dir(&self.socket_dir);
    }
}

// A connection requests exactly one snapshot. The server never accepts an
// input command, a mutation, or an expression from the editor.
fn serve(
    listener: UnixListener,
    ui: WeakComponent<ReloadableInstance>,
    alive: Weak<()>,
    editor: EditorSources,
) {
    while alive.strong_count() != 0 {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let (result, received) = mpsc::channel();
                let active_sources = editor.clone();
                let update = ui.upgrade_in_event_loop(move |ui| {
                    let _ = result.send(snapshot(&ui, active_sources.source_versions()));
                });
                let mut payload = match update {
                    Ok(()) => received
                        .recv_timeout(TIMEOUT)
                        .unwrap_or_else(|error| json!({"error": error.to_string()})),
                    Err(error) => json!({"error": error.to_string()}),
                };
                if let Some(diagnostic) = editor.last_diagnostic() {
                    payload["reloadDiagnostic"] =
                        json!(diagnostic.chars().take(4096).collect::<String>());
                }
                if stream
                    .set_nonblocking(false)
                    .and_then(|()| stream.set_write_timeout(Some(TIMEOUT)))
                    .is_ok()
                    && let Ok(mut bytes) = serde_json::to_vec(&payload)
                {
                    while bytes.len() >= 4 * 1024 * 1024 {
                        let Some(nodes) = payload["tree"]["nodes"].as_array_mut() else {
                            break;
                        };
                        if nodes.len() <= 1 {
                            break;
                        }
                        nodes.truncate(nodes.len() / 2);
                        payload["tree"]["truncated"] = json!(true);
                        let Ok(encoded) = serde_json::to_vec(&payload) else {
                            break;
                        };
                        bytes = encoded;
                    }
                    if bytes.len() >= 4 * 1024 * 1024 {
                        bytes = br#"{"error":"XRack inspection snapshot exceeds 4 MiB"}"#.to_vec();
                    }
                    bytes.push(b'\n');
                    let _ = stream.write_all(&bytes);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL),
            Err(_) => return,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Query {
    ReadModel(ModelQuery),
    Input(InputQuery),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputQuery {
    generation: u64,
    input: InputAction,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum InputAction {
    Click { id: String },
    Fill { id: String, text: String },
}

fn run_query(query: Query, ui: &WeakComponent<ReloadableInstance>, editor: &EditorSources) -> Json {
    match query {
        Query::ReadModel(query) => {
            let (reply, received) = mpsc::channel();
            let sources = editor.clone();
            match ui.upgrade_in_event_loop(move |ui| {
                let _ = reply.send(model_page(&ui, &query, sources.source_versions()));
            }) {
                Ok(()) => received
                    .recv_timeout(TIMEOUT)
                    .unwrap_or_else(|error| json!({"error": error.to_string()})),
                Err(error) => json!({"error": error.to_string()}),
            }
        }
        Query::Input(query) => {
            let action = match query.input {
                InputAction::Click { id } => RecordedAction::Click { id },
                InputAction::Fill { id, text } if text.len() <= 2048 => {
                    RecordedAction::Fill { id, text }
                }
                InputAction::Fill { .. } => {
                    return json!({"error": "Text input exceeds 2048 bytes"});
                }
            };
            let (reply, received) = mpsc::channel();
            let generation = query.generation;
            let input = InspectionInput {
                action,
                generation,
                deadline: Instant::now() + TIMEOUT,
                reply,
            };
            match ui.upgrade_in_event_loop(move |ui| ui.inspector().queue_input(input)) {
                Ok(()) => match received.recv_timeout(TIMEOUT) {
                    Ok(Ok(())) => json!({"applied": true, "generation": generation}),
                    Ok(Err(error)) => json!({"error": error}),
                    Err(error) => json!({"error": error.to_string()}),
                },
                Err(error) => json!({"error": error.to_string()}),
            }
        }
    }
}

// One bounded query or opt-in action per connection.
fn serve_queries(
    listener: UnixListener,
    ui: WeakComponent<ReloadableInstance>,
    alive: Weak<()>,
    editor: EditorSources,
) {
    while alive.strong_count() != 0 {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if stream
                    .set_nonblocking(false)
                    .and_then(|()| stream.set_read_timeout(Some(TIMEOUT)))
                    .and_then(|()| stream.set_write_timeout(Some(TIMEOUT)))
                    .is_err()
                {
                    continue;
                }
                let mut line = Vec::new();
                let read = BufReader::new((&stream).take(4096)).read_until(b'\n', &mut line);
                let response = if read.is_ok() && line.ends_with(b"\n") {
                    match serde_json::from_slice::<Query>(&line) {
                        Ok(query) => run_query(query, &ui, &editor),
                        Err(error) => json!({"error": error.to_string()}),
                    }
                } else {
                    json!({"error": "Expected an inspection request of at most 4096 bytes"})
                };
                if serde_json::to_writer(&mut stream, &response).is_ok() {
                    let _ = stream.write_all(b"\n");
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL),
            Err(_) => return,
        }
    }
}
