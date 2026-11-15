//! Workspace development components use the compiler already linked by Bazel.

use xrack_interpreter::live::{PreparedUi, imported_sources, source_text};

#[doc(hidden)]
pub use xrack_interpreter::live::{
    __xr_forward_component_methods, LiveComponent, LiveUi, ReloadableInstance,
};

#[doc(hidden)]
pub fn open_live<T: LiveUi>(filename: &str, name: &str) -> Result<LiveComponent<T>, String> {
    LiveComponent::open_with(filename, name, None, |source, saved| {
        let filename = source.to_str().ok_or("the .xr path must be valid UTF-8")?;
        let prepared = xrack_compiler::prepare_preview(
            source_text(source, saved)?,
            filename,
            imported_sources(source, saved),
        )
        .map_err(|error| error.to_string())?;
        Ok(PreparedUi {
            mlir: prepared.module.to_mlir(),
            inputs: prepared.inputs,
        })
    })
}
