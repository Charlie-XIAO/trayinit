use crate::platform::PlatformOptions;

#[derive(Debug, Default)]
pub struct TrayOptions {
    platform: PlatformOptions,
}

impl TrayOptions {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TrayOptions {
    pub fn with_platform(mut self, platform: PlatformOptions) -> Self {
        self.platform = platform;
        self
    }

    pub(crate) fn into_platform(self) -> PlatformOptions {
        self.platform
    }
}
