#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(crate) mod plan;

mod command;
mod runtime;
mod validate;

pub(crate) use command::{BackendCommand, BackendCommandSender};
pub(crate) use runtime::BackendRuntime;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(crate) use validate::validate_menu;
pub(crate) use validate::validate_state;
