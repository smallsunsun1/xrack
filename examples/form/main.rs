use form_ui::{
    BidiEditorDemo, CloseGuardDemo, DateFieldDemo, DragDropDemo, EditingLifecycleDemo,
    EditorCommandsDemo, EditorInsetsDemo, EditorMenuDemo, EditorSelectionDemo, FileDialogDemo,
    FileDropDemo, FormDemo, InputMaskDemo, LongTextSelectionDemo, MessageDialogDemo,
    NumericValidationDemo, PasswordDemo, PatternValidationDemo, PersistentSelectionDemo,
    SelectionColorsDemo, SubmissionValidationDemo, TextAlignmentDemo, TextOperationsDemo,
    TimeFieldDemo, ValidationDemo,
};
use std::{cell::OnceCell, rc::Rc};
use xrack_platform_winit::{DesktopWindow, run_desktop};

fn run_close_guard() -> Result<(), String> {
    run_desktop(|desktop| {
        let mut ui = CloseGuardDemo::new();
        let owner = Rc::new(OnceCell::<DesktopWindow>::new());
        let closer = owner.clone();
        ui.on_close_from_rust(move || {
            if let Some(window) = closer.get() {
                let _ = window.close();
            }
        });
        assert!(owner.set(desktop.open(ui)?).is_ok());
        Ok(())
    })
}

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--password") => xrack_platform_winit::run(PasswordDemo::new()),
        Some("--date-field") => xrack_platform_winit::run(DateFieldDemo::new()),
        Some("--time-field") => xrack_platform_winit::run(TimeFieldDemo::new()),
        Some("--editor-insets") => xrack_platform_winit::run(EditorInsetsDemo::new()),
        Some("--drag") => xrack_platform_winit::run(DragDropDemo::new()),
        Some("--drop-files") => xrack_platform_winit::run(FileDropDemo::new()),
        Some("--validation") => xrack_platform_winit::run(ValidationDemo::new()),
        Some("--rtl-text") => xrack_platform_winit::run(TextAlignmentDemo::new()),
        Some("--editor-selection") => xrack_platform_winit::run(EditorSelectionDemo::new()),
        Some("--persistent-selection") => xrack_platform_winit::run(PersistentSelectionDemo::new()),
        Some("--long-text-selection") => xrack_platform_winit::run(LongTextSelectionDemo::new()),
        Some("--selection-colors") => xrack_platform_winit::run(SelectionColorsDemo::new()),
        Some("--text-operations") => xrack_platform_winit::run(TextOperationsDemo::new()),
        Some("--input-pattern") => xrack_platform_winit::run(PatternValidationDemo::new()),
        Some("--editing-finish") => xrack_platform_winit::run(EditingLifecycleDemo::new()),
        Some("--input-mask") => xrack_platform_winit::run(InputMaskDemo::new()),
        Some("--numeric-validation") => xrack_platform_winit::run(NumericValidationDemo::new()),
        Some("--submission") => xrack_platform_winit::run(SubmissionValidationDemo::new()),
        Some("--editor-commands") => xrack_platform_winit::run(EditorCommandsDemo::new()),
        Some("--editor-menus") => xrack_platform_winit::run(EditorMenuDemo::new()),
        Some("--bidi") => xrack_platform_winit::run(BidiEditorDemo::new()),
        Some("--message") => xrack_platform_winit::run(MessageDialogDemo::new()),
        Some("--close-guard") => run_close_guard(),
        Some(mode @ ("--files" | "--folders")) => {
            let mut demo = FileDialogDemo::new();
            if let Some(directory) = std::env::args().nth(2) {
                demo.set_start_directory(directory);
            }
            if mode == "--folders" {
                demo.invoke_choose_folders();
            }
            xrack_platform_winit::run(demo)
        }
        _ => xrack_platform_winit::run(FormDemo::new()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
