use crate::TrayId;
use crate::platform::PlatformOptions;

#[derive(Debug, Default)]
pub struct TrayOptions {
    pub(crate) id: Option<TrayId>,
    pub(crate) platform: PlatformOptions,
}

impl TrayOptions {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TrayOptions {
    /// Sets the application-facing identity for this tray.
    ///
    /// Events emitted by this tray include the same id. If no id is provided,
    /// the crate generates a process-local id such as `tray-1`; set an
    /// explicit id when events from multiple trays share one sink or when
    /// Linux StatusNotifierItem identity should be stable.
    pub fn with_id(mut self, id: impl Into<TrayId>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_platform(mut self, platform: PlatformOptions) -> Self {
        self.platform = platform;
        self
    }
}
