use crate::{Element, Error, Instance, Result, Visual};
use std::rc::Rc;
use xrack_runtime::{
    FileDialogOptions, FileDialogSelection, MessageDialogOptions, NativeDialogOptions,
    NativeDialogRegistration, NativeDialogResult, NativeDialogSlot, Value, VecModel,
};

#[derive(Default)]
pub(crate) struct State {
    slot: NativeDialogSlot,
    selection: FileDialogSelection,
}

#[derive(Clone, Copy)]
pub(crate) enum Read {
    Pending,
    SelectedFile,
    SelectedFiles,
}

impl Read {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "pending" => Self::Pending,
            "selected-file" => Self::SelectedFile,
            "selected-files" => Self::SelectedFiles,
            _ => return None,
        })
    }
}

impl Instance {
    fn native_dialog(&self, key: &str) -> Result<&State> {
        self.0
            .native_dialogs
            .get(key)
            .ok_or_else(|| Error::Evaluation(format!("unmounted native dialog {key}")))
    }

    pub(crate) fn native_dialog_read(&self, key: &str, field: Read) -> Result<Value> {
        let (owner, _) = self.element_owner(key)?;
        let state = owner.native_dialog(key)?;
        Ok(match field {
            Read::Pending => state.slot.pending().into(),
            Read::SelectedFile => state.selection.selected_file().into(),
            Read::SelectedFiles => Value::StringModel(state.selection.selected_files()),
        })
    }

    pub(crate) fn open_native_dialog(&self, key: &str) -> Result<()> {
        let (owner, element) = self.element_owner(key)?;
        if owner.binding_value(element, "enabled")? == Some(Value::Bool(false)) {
            return Ok(());
        }
        owner
            .native_dialog(key)?
            .slot
            .try_open(|| owner.native_dialog_options(element))
    }

    fn native_dialog_options(&self, element: &Element) -> Result<NativeDialogOptions> {
        macro_rules! option {
            ($name:literal, $kind:ident) => {
                match self.binding_value(element, $name)? {
                    Some(Value::$kind(value)) => value,
                    None => Default::default(),
                    _ => {
                        return Err(Error::Evaluation(format!(
                            "invalid native dialog option {}",
                            $name
                        )))
                    }
                }
            };
        }

        let title = option!("title", String);
        if matches!(element.visual, Visual::Builtin("FileDialog")) {
            let name_filters = match self.binding_value(element, "name-filters")? {
                Some(Value::StringModel(model)) => model.snapshot(),
                Some(Value::StringList(filters)) => filters,
                None => Vec::new(),
                _ => {
                    return Err(Error::Evaluation(
                        "invalid native dialog name filters".into(),
                    ));
                }
            };
            Ok(NativeDialogOptions::File(FileDialogOptions {
                mode: option!("mode", FileDialogMode),
                title,
                directory: option!("directory", String),
                file_name: option!("file-name", String),
                name_filters,
            }))
        } else {
            Ok(NativeDialogOptions::Message(MessageDialogOptions {
                level: option!("level", MessageLevel),
                buttons: option!("buttons", MessageButtons),
                title,
                text: option!("text", String),
                informative_text: option!("informative-text", String),
            }))
        }
    }

    pub(crate) fn native_dialog_registration(
        &self,
        element: &Element,
    ) -> Result<Option<NativeDialogRegistration>> {
        let key = element.key.clone();
        let weak = Rc::downgrade(&self.0);
        Ok(self.native_dialog(&key)?.slot.register(move |id, result| {
            let Some(state) = weak.upgrade() else {
                return false;
            };
            let owner = Instance(state);
            let Some(element) = owner.definition().root.find_key(&key) else {
                return false;
            };
            owner.handle_result(owner.finish_native_dialog(element, id, result))
        }))
    }

    fn finish_native_dialog(
        &self,
        element: &Element,
        id: u64,
        result: NativeDialogResult,
    ) -> Result<bool> {
        let state = self.native_dialog(&element.key)?;
        if !state.slot.finish(id) {
            return Ok(false);
        }
        let is_file = matches!(element.visual, Visual::Builtin("FileDialog"));
        match result {
            NativeDialogResult::Files(paths) if is_file => {
                state.selection.commit(&paths);
                self.handler(
                    element,
                    "accepted",
                    &[Value::StringModel(VecModel::from(paths))],
                )?;
            }
            NativeDialogResult::Canceled if is_file => {
                self.handler(element, "canceled", &[])?;
            }
            NativeDialogResult::Button(button) if !is_file => {
                let event = if button.accepts_message() {
                    "accepted"
                } else {
                    "rejected"
                };
                self.handler(element, event, &[])?;
                self.handler(element, "finished", &[Value::StandardButtonKind(button)])?;
            }
            NativeDialogResult::Failed(message) => {
                self.handler(element, "failed", &[Value::String(message)])?;
            }
            NativeDialogResult::Abandoned => {}
            _ => return Ok(false),
        }
        Ok(true)
    }
}
