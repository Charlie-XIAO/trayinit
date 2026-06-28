use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use crate::{MenuItemId, PhysicalPosition, PhysicalRect};

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
        kind: TrayIconEventKind,
        position: Option<PhysicalPosition>,
        rect: Option<PhysicalRect>,
    },
    StatusChanged {
        status: TrayStatus,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayIconEventKind {
    PrimaryClick,
    SecondaryClick,
    DoubleClick,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayStatus {
    Available,
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
