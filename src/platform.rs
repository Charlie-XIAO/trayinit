#[cfg(target_os = "windows")]
#[path = "platform/windows/mod.rs"]
mod platform;

#[cfg(target_os = "linux")]
#[path = "platform/linux/mod.rs"]
mod platform;

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
#[path = "platform/unimplemented.rs"]
mod platform;

pub use platform::*;
