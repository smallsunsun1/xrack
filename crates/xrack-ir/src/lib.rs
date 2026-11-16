mod debug_hir;
mod loader;
pub mod model;
pub mod modules;
pub mod semantic;
mod slots;
pub use model::*;

mod dynamic;
pub use dynamic::declare_handles;
