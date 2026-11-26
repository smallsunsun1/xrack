mod debug_hir;
mod loader;
pub mod model;
pub mod modules;
#[doc(hidden)]
pub mod ownership;
pub mod semantic;
mod slots;
pub use model::*;

mod dynamic;
pub use dynamic::declare_handles;
