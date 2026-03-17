use dpi::{PhysicalPosition, PhysicalSize};

use crate::menu::MenuItem;
use crate::platform::{self, PlatformHandle};
use crate::{ClosedError, Error, Icon, Result};

/// User-defined tray state.
pub trait Tray: Sized + Send + 'static {
    /// Application-defined message type emitted by menu items.
    type Message;

    /// Stable identifier for the tray instance.
    fn id(&self) -> &str;

    /// Tray icon image.
    fn icon(&self) -> Option<Icon> {
        None
    }

    /// Themed tray icon name, if supported by the platform.
    ///
    /// Platform notes:
    /// - Linux SNI/DBus: exported as `IconName`; host/theme-dependent.
    /// - Windows: ignored.
    fn icon_name(&self) -> Option<String> {
        None
    }

    /// Themed overlay icon name, if supported by the platform.
    ///
    /// Platform notes:
    /// - Linux SNI/DBus: exported as `OverlayIconName`; host/theme-dependent.
    /// - Windows: ignored.
    fn overlay_icon_name(&self) -> Option<String> {
        None
    }

    /// Raster overlay icon image, if supported by the platform.
    ///
    /// Platform notes:
    /// - Linux SNI/DBus: exported as `OverlayIconPixmap`.
    /// - Windows: ignored.
    fn overlay_icon(&self) -> Option<Icon> {
        None
    }

    /// Themed attention icon name, if supported by the platform.
    ///
    /// Platform notes:
    /// - Linux SNI/DBus: exported as `AttentionIconName`; host/theme-dependent.
    /// - Windows: ignored.
    fn attention_icon_name(&self) -> Option<String> {
        None
    }

    /// Raster attention icon image, if supported by the platform.
    ///
    /// Platform notes:
    /// - Linux SNI/DBus: exported as `AttentionIconPixmap`.
    /// - Windows: ignored.
    fn attention_icon(&self) -> Option<Icon> {
        None
    }

    /// Optional attention animation resource name.
    ///
    /// Platform notes:
    /// - Linux SNI/DBus: exported as `AttentionMovieName`; hosts may ignore it.
    /// - Windows: ignored.
    fn attention_movie_name(&self) -> Option<String> {
        None
    }

    /// Tray title or label, if supported by the platform.
    ///
    /// Platform note:
    /// - Windows: unsupported.
    fn title(&self) -> Option<String> {
        None
    }

    /// Tray tooltip text.
    ///
    /// Platform notes:
    /// - Linux SNI/DBus: host-dependent. Some desktops may ignore or delay
    ///   tooltip presentation entirely.
    fn tooltip(&self) -> Option<String> {
        None
    }

    /// Whether the tray should be visible.
    fn visible(&self) -> bool {
        true
    }

    /// High-level tray state hint.
    fn status(&self) -> TrayStatus {
        TrayStatus::Active
    }

    /// Whether primary activation should open the menu.
    ///
    /// Platform notes:
    /// - Linux SNI/DBus: best-effort only. Hosts may choose their own primary
    ///   click behavior when a menu is exported, and some desktops do not
    ///   surface this as a reliable live toggle.
    fn menu_on_primary_click(&self) -> bool {
        false
    }

    /// Declarative tray menu tree.
    fn menu(&self) -> Vec<MenuItem<Self::Message>> {
        Vec::new()
    }

    /// Applies a tray-originated event back into the state.
    fn event(&mut self, event: TrayEvent<Self::Message>);

    /// Whether the tray runtime should shut down.
    ///
    /// This is primarily used by standalone [`TrayMethods::run`] mode, where
    /// there is no external [`Handle`] to request shutdown. Backends may also
    /// honor it in other startup modes by destroying only the tray runtime,
    /// without taking down any host application loop.
    fn should_exit(&self) -> bool {
        false
    }
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
    fn attach(self) -> Result<Handle<Self>>
    where
        Self::Message: Clone,
    {
        self.builder().attach()
    }

    /// Starts the tray service in self-hosted non-blocking mode.
    ///
    /// This is mainly a convenience for backends that can own themselves on a
    /// helper thread without taking over the caller's main thread.
    fn spawn(self) -> Result<Handle<Self>>
    where
        Self::Message: Clone,
    {
        self.builder().spawn()
    }

    /// Runs the tray as a standalone application.
    ///
    /// This mode is intended for tray-only apps where the tray runtime should
    /// own the application's top-level control flow.
    fn run(self) -> Result<()>
    where
        Self::Message: Clone,
    {
        self.builder().run()
    }
}

impl<T: Tray> TrayMethods for T {}

mod private {
    use crate::Tray;

    pub trait Sealed {}

    impl<T: Tray> Sealed for T {}
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
#[non_exhaustive]
pub enum TrayEvent<Message> {
    Menu(Message),
    Interaction(InteractionEvent),
    Scroll(ScrollEvent),
}

/// A semantic interaction with the tray icon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct InteractionEvent {
    pub kind: InteractionKind,
    pub position: Option<PhysicalPosition<i32>>,
    pub area: Option<(PhysicalPosition<i32>, PhysicalSize<i32>)>,
}

/// High-level meaning of a tray interaction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InteractionKind {
    #[default]
    PrimaryActivate,
    SecondaryActivate,
    ContextMenu,
}

/// A wheel/gesture scroll event over the tray icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ScrollEvent {
    pub delta: i32,
    pub axis: ScrollAxis,
    pub position: Option<PhysicalPosition<i32>>,
    pub area: Option<(PhysicalPosition<i32>, PhysicalSize<i32>)>,
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

    pub fn attach(self) -> Result<Handle<T>>
    where
        T::Message: Clone,
    {
        platform::attach(self)
    }

    pub fn spawn(self) -> Result<Handle<T>>
    where
        T::Message: Clone,
    {
        platform::spawn(self)
    }

    pub fn run(self) -> Result<()>
    where
        T::Message: Clone,
    {
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
#[derive(Debug)]
pub struct Handle<T: Tray> {
    tray_id: String,
    inner: PlatformHandle<T>,
}

impl<T: Tray> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Self {
            tray_id: self.tray_id.clone(),
            inner: self.inner.clone(),
        }
    }
}

impl<T: Tray> Handle<T> {
    pub(crate) fn new(tray_id: impl Into<String>, inner: PlatformHandle<T>) -> Self {
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

    #[cfg(target_os = "windows")]
    pub(crate) unsafe fn process_windows_message(
        &self,
        msg: *const windows_sys::Win32::UI::WindowsAndMessaging::MSG,
    ) -> bool {
        unsafe { self.inner.process_windows_message(msg) }
    }

    #[cfg(target_os = "windows")]
    pub(crate) unsafe fn register_accelerator_window(
        &self,
        hwnd: windows_sys::Win32::Foundation::HWND,
    ) -> core::result::Result<(), ClosedError> {
        unsafe { self.inner.register_accelerator_window(hwnd) }
    }

    #[cfg(target_os = "windows")]
    pub(crate) unsafe fn unregister_accelerator_window(
        &self,
        hwnd: windows_sys::Win32::Foundation::HWND,
    ) -> core::result::Result<(), ClosedError> {
        unsafe { self.inner.unregister_accelerator_window(hwnd) }
    }
}

impl From<ClosedError> for Error {
    fn from(_: ClosedError) -> Self {
        Error::Closed
    }
}
