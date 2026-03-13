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
//!     ActionItem, CheckItem, Handle, MenuItem, Tray, TrayEvent, TrayMethods, TrayView,
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
//!     type MenuId = ItemId;
//!
//!     fn id(&self) -> &str {
//!         "com.example.app"
//!     }
//!
//!     fn view(&self) -> TrayView<Self::MenuId> {
//!         TrayView {
//!             menu: vec![
//!                 CheckItem::new(ItemId::Toggle, "Enabled", self.checked).into(),
//!                 MenuItem::Separator,
//!                 ActionItem::new(ItemId::Quit, "Quit").into(),
//!             ],
//!             ..Default::default()
//!         }
//!     }
//!
//!     fn event(&mut self, event: TrayEvent<Self::MenuId>) {
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
mod geometry;
mod icon;
mod menu;
mod platform;
mod tooltip;
mod tray;

pub use error::{ClosedError, Error, IconError, Result};
pub use geometry::{PhysicalPosition, PhysicalSize, Rect};
pub use icon::Icon;
pub use menu::{
    Accelerator, ActionItem, CheckItem, Key, MenuItem, Modifiers, RadioGroup, RadioItem, Submenu,
};
pub use tooltip::Tooltip;
pub use tray::{
    ActivateEvent, Builder, Handle, LinuxOptions, RuntimePreference, ScrollAxis, ScrollEvent, Tray,
    TrayEvent, TrayMethods, TrayStatus, TrayView,
};
