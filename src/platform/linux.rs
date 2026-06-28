#[cfg(any(
    all(feature = "linux-zbus-async-io", feature = "linux-zbus-tokio"),
    not(any(feature = "linux-zbus-async-io", feature = "linux-zbus-tokio"))
))]
compile_error!(
    "Linux requires exactly one of the following features: linux-zbus-async-io, linux-zbus-tokio"
);

mod menu;
mod runtime;
mod service;

pub(crate) use service::spawn;
pub use service::{PlatformOptions, StartupPolicy};
