#[cfg(target_os = "windows")]
#[path = "platform/windows/mod.rs"]
mod platform;

#[cfg(not(target_os = "windows"))]
#[path = "platform/unimplemented.rs"]
mod platform;

pub use platform::*;
