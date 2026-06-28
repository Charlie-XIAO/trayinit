#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::spawn;
#[cfg(target_os = "linux")]
pub use linux::{PlatformOptions, StartupPolicy};
#[cfg(target_os = "macos")]
pub use macos::PlatformOptions;
#[cfg(target_os = "macos")]
pub(crate) use macos::spawn;
#[cfg(target_os = "windows")]
pub use windows::PlatformOptions;
#[cfg(target_os = "windows")]
pub(crate) use windows::spawn;
