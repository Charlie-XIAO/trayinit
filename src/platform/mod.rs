#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::Arc;

use crate::backend::BackendProxy;
use crate::{EventSink, TrayResult, TrayState};

pub(crate) fn spawn(
    initial_state: TrayState,
    sink: Arc<dyn EventSink>,
) -> TrayResult<BackendProxy> {
    #[cfg(target_os = "windows")]
    {
        windows::spawn(initial_state, sink)
    }

    #[cfg(target_os = "linux")]
    {
        linux::spawn(initial_state, sink)
    }

    #[cfg(target_os = "macos")]
    {
        macos::spawn(initial_state, sink)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = (initial_state, sink);
        Err(crate::TrayError::UnsupportedPlatform)
    }
}
