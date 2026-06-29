use std::fmt;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use crate::{MenuItemId, PhysicalPosition, PhysicalRect};

/// Application-facing identity for one tray instance.
///
/// A [`TrayId`] is assigned at tray construction. It is included in every
/// [`TrayEvent`] so applications can route events from multiple trays through
/// one sink without requiring globally unique menu item ids.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TrayId(Arc<str>);

/// Receives tray events from backend/platform threads.
///
/// Delivery is intentionally best-effort. Implementations should forward events
/// into an application queue, channel, or event-loop proxy and should avoid
/// mutating UI state directly. The backend never relies on a sink acknowledging
/// delivery; for example, [`ChannelEventSink`] drops events silently after the
/// receiver has been closed.
pub trait EventSink: Send + Sync + 'static {
    fn send(&self, event: TrayEvent);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    MenuItemActivated {
        tray_id: TrayId,
        item_id: MenuItemId,
    },
    /// The backend observed an activation gesture on the tray icon.
    ///
    /// Native tray hosts do not expose identical activation hooks on every
    /// platform. On Windows and macOS, a menu-opening click emits this event
    /// before any resulting [`TrayEvent::MenuItemActivated`]. Other backends
    /// may only emit icon activation for gestures that are exposed by the
    /// desktop host.
    IconActivated {
        tray_id: TrayId,
        kind: TrayIconEventKind,
        position: Option<PhysicalPosition>,
        rect: Option<PhysicalRect>,
    },
    StatusChanged {
        tray_id: TrayId,
        status: TrayStatus,
    },
}

impl TrayId {
    pub fn new(id: impl Into<String>) -> Self {
        let id: String = id.into();
        Self(Arc::from(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn is_valid(&self) -> bool {
        !self.0.trim().is_empty()
    }
}

impl From<&str> for TrayId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for TrayId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for TrayId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayIconEventKind {
    PrimaryClick,
    SecondaryClick,
    DoubleClick,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayStatus {
    /// The platform tray service is registered and a host/watcher is present.
    Available,
    /// Linux StatusNotifierWatcher is not currently available.
    WatcherUnavailable(String),
    /// Linux watcher exists, but no StatusNotifierHost is currently registered.
    NoHost(String),
    TemporarilyUnavailable(String),
    BackendError(String),
}

#[derive(Clone)]
pub struct ChannelEventSink {
    sender: Sender<TrayEvent>,
}

impl<F> EventSink for F
where
    F: Fn(TrayEvent) + Send + Sync + 'static,
{
    fn send(&self, event: TrayEvent) {
        self(event);
    }
}

impl EventSink for ChannelEventSink {
    fn send(&self, event: TrayEvent) {
        let _ = self.sender.send(event);
    }
}

impl EventSink for Arc<dyn EventSink> {
    fn send(&self, event: TrayEvent) {
        (**self).send(event);
    }
}

pub fn channel() -> (ChannelEventSink, Receiver<TrayEvent>) {
    let (sender, receiver) = mpsc::channel();
    (ChannelEventSink { sender }, receiver)
}
