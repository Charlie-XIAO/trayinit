#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod plan;

mod command;
mod runtime;
mod validate;

pub use command::BackendCommand;
pub use runtime::{BackendCommandSender, BackendRuntime};
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub use validate::validate_menu;
pub use validate::validate_state;
