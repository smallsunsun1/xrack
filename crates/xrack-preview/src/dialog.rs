use serde::{Deserialize, Serialize};
use xrack_runtime::{
    FileDialogMode, MessageButtons, MessageLevel, NativeDialogOptions, NativeDialogPresentation,
    NativeDialogResult, StandardButtonKind,
};

#[derive(Debug, Serialize)]
pub(crate) struct Panel {
    id: u64,
    #[serde(flatten)]
    options: Options,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Options {
    File {
        save: bool,
        directories: bool,
        multiple: bool,
        title: String,
        directory: String,
        file_name: String,
        filters: Vec<(String, Vec<String>)>,
        filter_error: Option<&'static str>,
    },
    Message {
        level: &'static str,
        buttons: &'static [&'static str],
        title: String,
        text: String,
        informative_text: String,
    },
}

impl From<NativeDialogPresentation> for Panel {
    fn from(request: NativeDialogPresentation) -> Self {
        let options = match request.options.as_ref() {
            NativeDialogOptions::File(file) => {
                use FileDialogMode::*;
                let (filters, filter_error) = match file.filters() {
                    Ok(filters) => (
                        filters
                            .into_iter()
                            .map(|(name, extensions)| {
                                (
                                    name.into(),
                                    extensions.into_iter().map(str::to_owned).collect(),
                                )
                            })
                            .collect(),
                        None,
                    ),
                    Err(error) => (Vec::new(), Some(error)),
                };
                Options::File {
                    save: file.mode == SaveFile,
                    directories: matches!(file.mode, SelectFolder | SelectFolders),
                    multiple: matches!(file.mode, OpenFiles | SelectFolders),
                    title: file.title.clone(),
                    directory: file.directory.clone(),
                    file_name: file.file_name.clone(),
                    filters,
                    filter_error,
                }
            }
            NativeDialogOptions::Message(message) => Options::Message {
                level: match message.level {
                    MessageLevel::Info => "info",
                    MessageLevel::Warning => "warning",
                    MessageLevel::Error => "error",
                },
                buttons: match message.buttons {
                    MessageButtons::Ok => &["ok"],
                    MessageButtons::OkCancel => &["ok", "cancel"],
                    MessageButtons::YesNo => &["yes", "no"],
                    MessageButtons::YesNoCancel => &["yes", "no", "cancel"],
                },
                title: message.title.clone(),
                text: message.text.clone(),
                informative_text: message.informative_text.clone(),
            },
        };
        Self {
            id: request.id,
            options,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub(crate) enum Response {
    Files { paths: Vec<String> },
    Button { button: Button },
    Canceled,
    Failed { message: String },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Button {
    Ok,
    Yes,
    No,
    Cancel,
}

impl From<Response> for NativeDialogResult {
    fn from(result: Response) -> Self {
        match result {
            Response::Files { paths } if paths.is_empty() => Self::Canceled,
            Response::Files { paths } => Self::Files(paths),
            Response::Button { button } => Self::Button(match button {
                Button::Ok => StandardButtonKind::Ok,
                Button::Yes => StandardButtonKind::Yes,
                Button::No => StandardButtonKind::No,
                Button::Cancel => StandardButtonKind::Cancel,
            }),
            Response::Canceled => Self::Canceled,
            Response::Failed { message } => Self::Failed(message),
        }
    }
}
