#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

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

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (initial_state, sink);
        Err(crate::TrayError::UnsupportedPlatform)
    }
}
