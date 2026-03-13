use core::hash::Hash;

use crate::{ClosedError, Error, Icon, MenuItem, Result, platform};

/// User-defined tray state.
pub trait Tray: Sized + Send + 'static {
    /// Application-defined identifier type for menu items.
    type MenuId: Clone + Eq + Hash + Send + Sync + 'static;

    /// Stable identifier for the tray instance.
    fn id(&self) -> &str;

    /// Renders the current tray state.
    fn view(&self) -> TrayView<Self::MenuId>;

    /// Applies a tray-originated event back into the state.
    fn event(&mut self, event: TrayEvent<Self::MenuId>);
}

/// Blanket convenience methods for [`Tray`] implementations.
pub trait TrayMethods: Tray + private::Sealed {
    /// Creates a configurable tray builder.
    fn builder(self) -> Builder<Self> {
        Builder::new(self)
    }

    /// Starts the tray service in host-integrated mode.
    ///
    /// This is the preferred startup mode for windowed apps such as `winit`
    /// applications. The backend does not own the app's top-level control flow.
    fn attach(self) -> Result<Handle<Self>> {
        self.builder().attach()
    }

    /// Starts the tray service in self-hosted non-blocking mode.
    ///
    /// This is mainly a convenience for backends that can own themselves on a
    /// helper thread without taking over the caller's main thread.
    fn spawn(self) -> Result<Handle<Self>> {
        self.builder().spawn()
    }

    /// Runs the tray as a standalone application.
    ///
    /// This mode is intended for tray-only apps where the tray runtime should
    /// own the application's top-level control flow.
    fn run(self) -> Result<()> {
        self.builder().run()
    }
}

impl<T: Tray> TrayMethods for T {}

mod private {
    pub trait Sealed {}

    impl<T: crate::Tray> Sealed for T {}
}

/// Fully rendered tray snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayView<Id> {
    pub icon: Option<Icon>,
    pub title: Option<String>,
    pub tooltip: Option<String>,
    pub visible: bool,
    pub status: TrayStatus,
    pub menu_on_primary_click: bool,
    pub menu: Vec<MenuItem<Id>>,
}

impl<Id> Default for TrayView<Id> {
    fn default() -> Self {
        Self {
            icon: None,
            title: None,
            tooltip: None,
            visible: true,
            status: TrayStatus::Active,
            menu_on_primary_click: false,
            menu: Vec::new(),
        }
    }
}

/// High-level tray visibility/importance hint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TrayStatus {
    Passive,
    #[default]
    Active,
    Attention,
}

/// Event emitted from a tray backend into [`Tray::event`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayEvent<Id> {
    Activate(ActivateEvent),
    SecondaryActivate(ActivateEvent),
    Scroll(ScrollEvent),
    Menu(Id),
}

/// Activation metadata for tray clicks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ActivateEvent {
    pub position: Option<crate::PhysicalPosition>,
    pub area: Option<crate::Rect>,
}

/// A wheel/gesture scroll event over the tray icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScrollEvent {
    pub delta: i32,
    pub axis: ScrollAxis,
}

/// Scroll axis reported by the backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollAxis {
    Horizontal,
    Vertical,
}

/// Startup-time tray configuration.
#[derive(Debug)]
pub struct Builder<T: Tray> {
    pub(crate) tray: T,
    pub(crate) runtime_preference: RuntimePreference,
    pub(crate) linux: LinuxOptions,
}

impl<T: Tray> Builder<T> {
    pub fn new(tray: T) -> Self {
        Self {
            tray,
            runtime_preference: RuntimePreference::Auto,
            linux: LinuxOptions::default(),
        }
    }

    pub fn runtime_preference(mut self, runtime_preference: RuntimePreference) -> Self {
        self.runtime_preference = runtime_preference;
        self
    }

    pub fn linux_options(mut self, linux: LinuxOptions) -> Self {
        self.linux = linux;
        self
    }

    pub fn linux_own_dbus_name(mut self, own_dbus_name: bool) -> Self {
        self.linux.own_dbus_name = own_dbus_name;
        self
    }

    pub fn linux_assume_watcher_available(mut self, assume_watcher_available: bool) -> Self {
        self.linux.assume_watcher_available = assume_watcher_available;
        self
    }

    pub fn runtime_preference_ref(&self) -> &RuntimePreference {
        &self.runtime_preference
    }

    pub fn linux_options_ref(&self) -> &LinuxOptions {
        &self.linux
    }

    pub fn tray_ref(&self) -> &T {
        &self.tray
    }

    pub fn into_inner(self) -> T {
        self.tray
    }

    pub fn attach(self) -> Result<Handle<T>> {
        platform::attach(self)
    }

    pub fn spawn(self) -> Result<Handle<T>> {
        platform::spawn(self)
    }

    pub fn run(self) -> Result<()> {
        platform::run(self)
    }
}

/// Advisory runtime strategy for future backend implementations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RuntimePreference {
    /// Let the backend pick the best strategy for the platform.
    #[default]
    Auto,
    /// Prefer a dedicated background thread owned by the tray.
    DedicatedThread,
    /// Prefer binding the backend to the caller's current thread.
    CurrentThread,
}

/// Linux-specific spawn options that are intentionally kept off the core trait.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LinuxOptions {
    pub own_dbus_name: bool,
    pub assume_watcher_available: bool,
}

impl Default for LinuxOptions {
    fn default() -> Self {
        Self {
            own_dbus_name: true,
            assume_watcher_available: false,
        }
    }
}

/// Control channel to a running tray.
#[derive(Debug, Clone)]
pub struct Handle<T: Tray> {
    tray_id: String,
    inner: crate::platform::PlatformHandle<T>,
}

impl<T: Tray> Handle<T> {
    pub(crate) fn new(
        tray_id: impl Into<String>,
        inner: crate::platform::PlatformHandle<T>,
    ) -> Self {
        Self {
            tray_id: tray_id.into(),
            inner,
        }
    }

    pub fn tray_id(&self) -> &str {
        &self.tray_id
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> core::result::Result<R, ClosedError> {
        self.inner.update(f)
    }

    pub fn refresh(&self) -> core::result::Result<(), ClosedError> {
        self.inner.refresh()
    }

    pub fn shutdown(&self) -> Result<()> {
        self.inner.shutdown()
    }

    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}

impl From<ClosedError> for Error {
    fn from(_: ClosedError) -> Self {
        Error::Closed
    }
}
