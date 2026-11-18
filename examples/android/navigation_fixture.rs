use xrack_interpreter::{Instance, Program};

pub fn interpreted_navigation() -> Result<Instance, Box<dyn std::error::Error>> {
    let filename = "examples/android/navigation.xr";
    let mut preview = xrack_compiler::prepare_preview(
        include_str!("navigation.xr"),
        filename,
        Default::default(),
    )?;
    let translations = preview.take_translation_catalog();
    let (program, _) =
        Program::from_module_with_file_resources(&preview.module, filename, translations)?;
    Ok(program.create("MobileNavigation")?)
}
