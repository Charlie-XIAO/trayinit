use std::fmt;

use crate::{IconError, MenuItemId};

pub type TrayResult<T> = Result<T, TrayError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayError {
    InvalidState(InvalidState),
    UnsupportedPlatform,
    NotMainThread,
    BackendUnavailable(String),
    CommandQueueClosed,
    ThreadInit(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvalidState {
    EmptyMenuItemId,
    DuplicateMenuItemId(MenuItemId),
    InvalidIcon(IconError),
}

impl fmt::Display for TrayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState(err) => write!(f, "invalid tray state: {err}"),
            Self::UnsupportedPlatform => {
                write!(f, "tray backend is not available on this platform")
            },
            Self::NotMainThread => write!(f, "tray backend operation must run on the main thread"),
            Self::BackendUnavailable(err) => write!(f, "tray backend is unavailable: {err}"),
            Self::CommandQueueClosed => write!(f, "tray backend command queue is closed"),
            Self::ThreadInit(err) => write!(f, "tray backend thread failed to initialize: {err}"),
        }
    }
}

impl fmt::Display for InvalidState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMenuItemId => write!(f, "menu item id must not be empty"),
            Self::DuplicateMenuItemId(id) => write!(f, "duplicate menu item id `{}`", id.as_str()),
            Self::InvalidIcon(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for TrayError {}

impl std::error::Error for InvalidState {}

impl From<InvalidState> for TrayError {
    fn from(value: InvalidState) -> Self {
        Self::InvalidState(value)
    }
}
