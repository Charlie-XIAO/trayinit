use core::fmt;

pub type Result<T, E = Error> = core::result::Result<T, E>;

/// Top-level tray API error.
#[derive(Debug)]
pub enum Error {
    /// The requested backend path exists in the API, but is not implemented
    /// yet.
    NotImplemented,
    /// The tray service rejected the request because it is already closed.
    Closed,
    /// The request could not be performed on this platform or thread.
    Unsupported(&'static str),
    /// The platform backend failed to initialize or update native state.
    Os(std::io::Error),
    /// A backend thread exited before it could finish initialization.
    Initialization(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotImplemented => f.write_str("tray backend is not implemented"),
            Error::Closed => f.write_str("tray handle is closed"),
            Error::Unsupported(message) => f.write_str(message),
            Error::Os(error) => write!(f, "operating system error: {error}"),
            Error::Initialization(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Os(error) => Some(error),
            _ => None,
        }
    }
}

/// Returned when a [`crate::Handle`] can no longer talk to its tray service.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClosedError;

impl fmt::Display for ClosedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("tray handle is closed")
    }
}

impl std::error::Error for ClosedError {}

/// Errors while constructing an [`crate::Icon`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconError {
    ZeroDimensions,
    PixelCountMismatch {
        width: u32,
        height: u32,
        rgba_len: usize,
    },
}

impl fmt::Display for IconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IconError::ZeroDimensions => f.write_str("icon dimensions must be non-zero"),
            IconError::PixelCountMismatch {
                width,
                height,
                rgba_len,
            } => write!(
                f,
                "icon RGBA length {rgba_len} does not match dimensions {width}x{height}"
            ),
        }
    }
}

impl std::error::Error for IconError {}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Os(value)
    }
}
