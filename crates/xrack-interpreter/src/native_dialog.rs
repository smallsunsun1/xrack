use crate::{Element, Error, Instance, Result, Visual};
use std::rc::Rc;
use xrack_runtime::{
    FileDialogOptions, FileDialogSelection, MessageDialogOptions, NativeDialogEvent,
    NativeDialogOptions, NativeDialogRegistration, NativeDialogResult, NativeDialogSlot, Value,
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
        owner.native_dialog(key)?.slot.try_open_if(
            || Ok(owner.binding_value(element, "enabled")? != Some(Value::Bool(false))),
            || owner.native_dialog_options(element),
        )
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

        if matches!(element.visual, Visual::Builtin("FileDialog")) {
            FileDialogOptions::try_read_bindings(
                || Ok(option!("mode", FileDialogMode)),
                || Ok(option!("title", String)),
                || Ok(option!("directory", String)),
                || Ok(option!("file-name", String)),
                || match self.binding_value(element, "name-filters")? {
                    Some(Value::StringModel(model)) => Ok(model.snapshot()),
                    Some(Value::StringList(filters)) => Ok(filters),
                    None => Ok(Vec::new()),
                    _ => Err(Error::Evaluation(
                        "invalid native dialog name filters".into(),
                    )),
                },
            )
            .map(NativeDialogOptions::File)
        } else {
            MessageDialogOptions::try_read_bindings(
                || Ok(option!("level", MessageLevel)),
                || Ok(option!("buttons", MessageButtons)),
                || Ok(option!("title", String)),
                || Ok(option!("text", String)),
                || Ok(option!("informative-text", String)),
            )
            .map(NativeDialogOptions::Message)
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
            let owner = Instance(state, None);
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
        let is_file = matches!(element.visual, Visual::Builtin("FileDialog"));
        state
            .slot
            .try_complete(id, result, is_file.then_some(&state.selection), |event| {
                match event {
                    NativeDialogEvent::FilesSelected(paths) => {
                        self.handler(element, "accepted", &[Value::StringModel(paths)])?
                    }
                    NativeDialogEvent::FileCanceled => self.handler(element, "canceled", &[])?,
                    NativeDialogEvent::MessageAccepted => self.handler(element, "accepted", &[])?,
                    NativeDialogEvent::MessageRejected => self.handler(element, "rejected", &[])?,
                    NativeDialogEvent::MessageFinished(button) => {
                        self.handler(element, "finished", &[Value::StandardButtonKind(button)])?
                    }
                    NativeDialogEvent::Failed(message) => {
                        self.handler(element, "failed", &[Value::String(message)])?
                    }
                };
                Ok(())
            })
    }
}
