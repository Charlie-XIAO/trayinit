use crate::menu::AcceleratorError;

pub type Result<T, E = Error> = core::result::Result<T, E>;

/// Top-level tray API error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The requested backend path exists in the API, but is not implemented
    /// yet.
    #[error("tray backend is not implemented")]
    NotImplemented,
    /// The tray service rejected the request because it is already closed.
    #[error("tray handle is closed")]
    Closed,
    /// The request could not be performed on this platform or thread.
    #[error("{0}")]
    Unsupported(&'static str),
    /// A menu accelerator could not be represented on the native backend.
    #[error("accelerator error: {0}")]
    Accelerator(#[source] AcceleratorError),
    /// The platform backend failed to initialize or update native state.
    #[error("operating system error: {0}")]
    Os(#[from] std::io::Error),
    /// A platform backend reported an implementation-specific failure.
    #[error("{0}")]
    Backend(String),
    /// A backend thread exited before it could finish initialization.
    #[error("{0}")]
    Initialization(&'static str),
}

/// Returned when a [`crate::Handle`] can no longer talk to its tray service.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, thiserror::Error)]
#[error("tray handle is closed")]
pub struct ClosedError;

/// Errors while constructing an [`crate::Icon`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IconError {
    #[error("icon dimensions must be non-zero")]
    ZeroDimensions,
    #[error("icon RGBA length {rgba_len} does not match dimensions {width}x{height}")]
    PixelCountMismatch {
        width: u32,
        height: u32,
        rgba_len: usize,
    },
}
