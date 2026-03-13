//! Cross-platform tray API sketch.
//!
//! The platform backends are intentionally left unimplemented for now. The
//! goal of this crate revision is to lock down the public API shape around a
//! reactive tray model:
//!
//! - [`Tray`] owns the application state.
//! - [`Tray::view`] renders a full tray snapshot from that state.
//! - [`Tray::event`] applies user input back into that state.
//! - [`Handle`] lets outside code mutate state and request refreshes.
//!
//! ```no_run
//! use trayinit::{
//!     CheckItem, Handle, MenuItem, StandardItem, Tray, TrayEvent, TrayMethods,
//! };
//!
//! #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
//! enum ItemId {
//!     Toggle,
//!     Quit,
//! }
//!
//! struct AppTray {
//!     checked: bool,
//! }
//!
//! impl Tray for AppTray {
//!     type Message = ItemId;
//!
//!     fn id(&self) -> &str {
//!         "com.example.app"
//!     }
//!
//!     fn menu(&self) -> Vec<MenuItem<Self::Message>> {
//!         vec![
//!             CheckItem::new("Enabled", self.checked, ItemId::Toggle).into(),
//!             MenuItem::Separator,
//!             StandardItem::new("Quit", ItemId::Quit).into(),
//!         ]
//!     }
//!
//!     fn event(&mut self, event: TrayEvent<Self::Message>) {
//!         match event {
//!             TrayEvent::Menu(ItemId::Toggle) => self.checked = !self.checked,
//!             TrayEvent::Menu(ItemId::Quit) => {}
//!             TrayEvent::Activate(_) => {}
//!             TrayEvent::SecondaryActivate(_) => {}
//!             TrayEvent::Scroll(_) => {}
//!         }
//!     }
//! }
//!
//! let handle: Handle<AppTray> = AppTray { checked: false }.spawn().unwrap();
//! handle.update(|tray| tray.checked = true).unwrap();
//! ```

mod error;
mod icon;
mod menu;
pub(crate) mod model;
mod platform;
mod tray;

pub use dpi;

pub use error::{ClosedError, Error, IconError, Result};
pub use icon::Icon;
pub use menu::{
    Accelerator, CheckItem, Key, MenuItem, Modifiers, RadioGroup, RadioItem, StandardItem, Submenu,
};
pub use tray::{
    ActivateEvent, Builder, Handle, LinuxOptions, RuntimePreference, ScrollAxis, ScrollEvent, Tray,
    TrayEvent, TrayMethods, TrayStatus,
};
