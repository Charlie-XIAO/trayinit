#[cfg(target_os = "macos")]
mod direct;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod threaded;

#[cfg(target_os = "macos")]
pub use direct::{BackendCommandSender, BackendRuntime};
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub use threaded::{BackendCommandSender, BackendRuntime};
