use crate::{
    Component, LoaderFrame, LoaderItem, LoaderMount, Node, Property, WeakComponent,
    loader_candidate_scene,
};
use std::{
    any::Any,
    cell::RefCell,
    io::Read,
    path::PathBuf,
    rc::{Rc, Weak},
    sync::{OnceLock, mpsc},
};

/// Source and optional exported component delivered to the interpreter on the UI thread.
#[doc(hidden)]
pub struct ExternalSource {
    pub location: String,
    pub path: Option<PathBuf>,
    pub component: Option<String>,
    pub text: String,
}

/// The interpreter-owned component and its non-owning host handle.
#[doc(hidden)]
pub struct LoadedExternal {
    pub(crate) ui: Box<dyn Component>,
    pub(crate) item: Box<dyn Any>,
}

impl LoadedExternal {
    pub fn new<T: Component + 'static>(ui: T, item: WeakComponent<T>) -> Self {
        Self {
            ui: Box::new(ui),
            item: Box::new(item),
        }
    }
}

type Factory = fn(ExternalSource) -> Result<LoadedExternal, String>;
static FACTORY: OnceLock<Factory> = OnceLock::new();

#[doc(hidden)]
pub fn register_external_loader(factory: Factory) {
    let _ = FACTORY.set(factory);
}

struct Request {
    read: mpsc::Receiver<Result<ExternalSource, String>>,
    gate: Rc<Property<bool>>,
}

enum Content {
    Loading(Request),
    Ready(Rc<RefCell<LoadedExternal>>, LoaderMount),
    Failed(String),
}

struct Selection {
    source: String,
    content: Content,
}

/// One externally loaded component, with a request scoped to the selected URI.
#[doc(hidden)]
#[derive(Default)]
pub struct ExternalLoader {
    current: RefCell<Option<Selection>>,
    status: Property<&'static str>,
}

impl ExternalLoader {
    pub fn item<T: 'static>(&self) -> Option<LoaderItem<T>> {
        let current = self.current.borrow();
        let Content::Ready(component, _) = &current.as_ref()?.content else {
            return None;
        };
        let item = component
            .try_borrow()
            .ok()?
            .item
            .downcast_ref::<WeakComponent<T>>()?
            .clone();
        let mount: Rc<dyn Any> = component.clone();
        Some(LoaderItem::with_mount(Rc::downgrade(&mount), item))
    }

    pub fn status(&self, source: &str) -> String {
        if source.is_empty() {
            return "null".into();
        }
        let status = self.status.get(|| "loading");
        if self
            .current
            .borrow()
            .as_ref()
            .is_some_and(|selection| selection.source == source)
        {
            status.into()
        } else {
            "loading".into()
        }
    }

    pub fn render(&self, source: &str) -> Node {
        if source.is_empty() {
            self.current.take();
            self.status.set_if_changed("null");
            return Node::hidden();
        }
        let mut current = self.current.borrow_mut();
        if !current
            .as_ref()
            .is_some_and(|selection| selection.source == source)
        {
            self.status.set_if_changed("loading");
            *current = Some(Selection {
                source: source.into(),
                content: match FACTORY.get() {
                    Some(_) => match start_read(source) {
                        Ok(request) => Content::Loading(request),
                        Err(error) => Content::Failed(error),
                    },
                    None => {
                        Content::Failed("external Loader requires the XRack interpreter".into())
                    }
                },
            });
        }
        let Some(selection) = current.as_mut() else {
            return Node::hidden();
        };
        if let Content::Loading(request) = &mut selection.content
            && request.gate.get(|| false)
        {
            match request.read.try_recv() {
                Ok(result) => {
                    let loaded = result.and_then(|source| {
                        let factory = FACTORY.get().ok_or("missing XRack interpreter")?;
                        factory(source)
                    });
                    selection.content = match loaded {
                        Ok(component) => {
                            Content::Ready(Rc::new(RefCell::new(component)), LoaderMount::default())
                        }
                        Err(error) => Content::Failed(error),
                    };
                }
                Err(mpsc::TryRecvError::Empty) => request.gate = Rc::default(),
                Err(mpsc::TryRecvError::Disconnected) => {
                    selection.content =
                        Content::Failed("external Loader read did not complete".into());
                }
            }
        }
        match &selection.content {
            Content::Loading(request) => LoaderFrame::waiting(&request.gate),
            Content::Ready(component, mount) => {
                let mut node = component.borrow().ui.render();
                node.set_loader_mount(mount.clone());
                node.set_external_component(ExternalComponent(Rc::downgrade(component)));
                self.status.set_if_changed("ready");
                loader_candidate_scene(vec![node])
            }
            Content::Failed(error) => {
                let mut node = Node::hidden();
                node.properties.insert("load-error", error.clone().into());
                self.status.set_if_changed("error");
                node
            }
        }
    }
}

/// Route input only while the external slot still owns the component.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct ExternalComponent(pub(crate) Weak<RefCell<LoadedExternal>>);

impl PartialEq for ExternalComponent {
    fn eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
}

impl ExternalComponent {
    pub(crate) fn with_mut<T>(&self, visit: impl FnOnce(&mut dyn Component) -> T) -> Option<T> {
        let component = self.0.upgrade()?;
        let mut component = component.try_borrow_mut().ok()?;
        Some(visit(component.ui.as_mut()))
    }
}

fn start_read(source: &str) -> Result<Request, String> {
    let (send, read) = mpsc::channel();
    let source = source.to_owned();
    std::thread::Builder::new()
        .name("xrack-external-loader".into())
        .spawn(move || {
            let _ = send.send(read_source(&source));
        })
        .map_err(|error| error.to_string())?;
    Ok(Request {
        read,
        gate: Rc::default(),
    })
}

fn read_source(source: &str) -> Result<ExternalSource, String> {
    const LIMIT: usize = 4 * 1024 * 1024;
    let (location, component) = source
        .rsplit_once('#')
        .map_or((source, None), |(path, name)| (path, Some(name.to_owned())));
    let (path, text) = if location.starts_with("https://") || location.starts_with("http://") {
        let output = std::process::Command::new("curl")
            .args([
                "--disable",
                "--globoff",
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--max-redirs",
                "4",
                "--connect-timeout",
                "5",
                "--max-time",
                "15",
                "--max-filesize",
                "4194304",
                "--proto",
                "=http,https",
                "--proto-redir",
                "=http,https",
                "--url",
                location,
            ])
            .output()
            .map_err(|error| format!("could not run curl: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "external Loader download failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        (None, output.stdout)
    } else {
        let path = if let Some(uri) = location.strip_prefix("file://") {
            let uri = uri.strip_prefix("localhost").unwrap_or(uri);
            if !uri.starts_with('/') {
                return Err("file URL must use a local absolute path".into());
            }
            let decoded = decode_path(uri)?;
            #[cfg(windows)]
            let decoded = decoded.strip_prefix('/').unwrap_or(&decoded).to_owned();
            PathBuf::from(decoded)
        } else if location.contains("://") {
            return Err("external Loader supports file, HTTP and HTTPS URLs".into());
        } else {
            PathBuf::from(location)
        };
        let file =
            std::fs::File::open(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut text = Vec::new();
        file.take((LIMIT + 1) as u64)
            .read_to_end(&mut text)
            .map_err(|error| error.to_string())?;
        (Some(path), text)
    };
    if text.len() > LIMIT {
        return Err("external Loader source exceeds 4 MiB".into());
    }
    let text = String::from_utf8(text).map_err(|error| error.to_string())?;
    Ok(ExternalSource {
        location: location.into(),
        path,
        component,
        text,
    })
}

fn decode_path(path: &str) -> Result<String, String> {
    let mut bytes = path.bytes();
    let mut decoded = Vec::new();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let hex = |byte: Option<u8>| {
                byte.and_then(|byte| (byte as char).to_digit(16))
                    .ok_or_else(|| "invalid percent-encoding in file URL".to_owned())
            };
            decoded.push((hex(bytes.next())? * 16 + hex(bytes.next())?) as u8);
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded).map_err(|error| error.to_string())
}
