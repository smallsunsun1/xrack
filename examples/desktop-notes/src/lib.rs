use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    rc::Rc,
};

xrack::include_modules!();

pub enum FileJob {
    Open(PathBuf),
    Save { path: PathBuf, content: String },
}

pub struct FileResult {
    path: PathBuf,
    content: String,
    opened: bool,
}

impl FileJob {
    pub fn run(self) -> io::Result<FileResult> {
        let path = match &self {
            Self::Open(path) | Self::Save { path, .. } => path,
        };
        if path.to_str().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "需要 UTF-8 文件路径",
            ));
        }
        match self {
            Self::Open(path) => Ok(FileResult {
                content: fs::read_to_string(&path)?,
                path,
                opened: true,
            }),
            Self::Save { path, content } => {
                save_file(&path, &content)?;
                Ok(FileResult {
                    path,
                    content,
                    opened: false,
                })
            }
        }
    }
}

fn save_file(path: &Path, content: &str) -> io::Result<()> {
    let target = match fs::canonicalize(path) {
        Ok(target) => target,
        Err(error) if error.kind() == io::ErrorKind::NotFound => path.to_owned(),
        Err(error) => return Err(error),
    };
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut output = tempfile::NamedTempFile::new_in(parent)?;
    output.write_all(content.as_bytes())?;
    match fs::metadata(&target) {
        Ok(metadata) => output.as_file().set_permissions(metadata.permissions())?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    output.as_file().sync_all()?;
    output.persist(target)?;
    Ok(())
}

pub type CompleteFileJob = Box<dyn FnOnce(io::Result<FileResult>)>;

/// Connect the same application callbacks to a desktop executor or a test executor.
pub fn connect(
    ui: &mut FieldNotes,
    submit: impl Fn(FileJob, CompleteFileJob) -> io::Result<()> + 'static,
) {
    let submit = Rc::new(submit);
    let weak = ui.as_weak();
    let open_submit = submit.clone();
    ui.on_open_document(move |path| {
        if let Some(ui) = weak.upgrade() {
            send(ui, FileJob::Open(path.into()), &*open_submit);
        }
    });
    let weak = ui.as_weak();
    ui.on_save_document(move |path| {
        if let Some(ui) = weak.upgrade() {
            let content = ui.get_content();
            send(
                ui,
                FileJob::Save {
                    path: path.into(),
                    content,
                },
                &*submit,
            );
        }
    });
}

fn send(
    mut ui: FieldNotes,
    job: FileJob,
    submit: &impl Fn(FileJob, CompleteFileJob) -> io::Result<()>,
) {
    if ui.get_working() {
        return;
    }
    ui.set_working(true);
    ui.set_status(match &job {
        FileJob::Open(_) => "正在打开…".into(),
        FileJob::Save { .. } => "正在保存…".into(),
    });
    let weak = ui.as_weak();
    if let Err(error) = submit(
        job,
        Box::new(move |result| {
            if let Some(mut ui) = weak.upgrade() {
                apply_file_result(&mut ui, result);
            }
        }),
    ) {
        apply_file_result(&mut ui, Err(error));
    }
}

fn apply_file_result(ui: &mut FieldNotes, result: io::Result<FileResult>) {
    ui.set_working(false);
    match result {
        Ok(result) => {
            let name = result.path.file_name().unwrap_or(result.path.as_os_str());
            ui.set_document_name(name.to_string_lossy().into_owned());
            ui.set_path(result.path.to_string_lossy().into_owned());
            if result.opened {
                ui.set_content(result.content.clone());
            }
            ui.set_saved_content(result.content);
            ui.set_status(
                if result.opened {
                    "已打开"
                } else {
                    "已保存"
                }
                .into(),
            );
        }
        Err(error) => ui.set_status(format!("文件操作失败：{error}")),
    }
}
