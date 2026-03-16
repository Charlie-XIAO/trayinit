mod error;
mod icon;
pub mod menu;
mod model;
mod platform;
mod tray;

pub use dpi;

pub use crate::error::{ClosedError, Error, IconError, Result};
pub use crate::icon::Icon;
pub use crate::tray::{
    Builder, Handle, InteractionEvent, InteractionKind, LinuxOptions, RuntimePreference,
    ScrollAxis, ScrollEvent, Tray, TrayEvent, TrayMethods, TrayStatus,
};

#[cfg(target_os = "windows")]
pub mod windows {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::MSG;

    use crate::{ClosedError, Handle, Tray};

    /// Runs tray accelerator translation for a raw Win32 message.
    ///
    /// This is intended for host-integrated Windows runtimes such as `attach()`
    /// with `winit`'s `with_msg_hook(...)`, after at least one host window has
    /// been registered with [`register_accelerator_window`].
    ///
    /// The tray's own hidden helper window is not a meaningful keyboard-focus
    /// target. In practice, Windows tray accelerators only work when the app
    /// has a real focusable host window whose incoming messages are passed
    /// here.
    ///
    /// Only messages whose `msg.hwnd` exactly matches a previously registered
    /// host `HWND` are considered. Child windows are not matched implicitly; if
    /// a framework delivers accelerator-relevant messages to multiple HWNDs,
    /// either register each one explicitly or perform your own filtering before
    /// calling this helper.
    pub unsafe fn process_message<T: Tray>(handle: &Handle<T>, msg: *const MSG) -> bool {
        unsafe { handle.process_windows_message(msg) }
    }

    /// Registers a focusable host window for Windows tray accelerator routing.
    ///
    /// The tray itself still uses its hidden helper window internally. This
    /// registration only tells the accelerator hook which incoming host-window
    /// messages are allowed to drive the tray's accelerator table.
    ///
    /// This mirrors the Win32 model used by menu libraries such as `muda`:
    /// accelerator translation is tied to a real application window, not the
    /// tray's hidden notification-area helper window.
    ///
    /// Registration is exact by `HWND`; child windows are not included
    /// automatically.
    ///
    /// # Safety
    ///
    /// `hwnd` must remain a valid window handle while registered.
    pub unsafe fn register_accelerator_window<T: Tray>(
        handle: &Handle<T>,
        hwnd: HWND,
    ) -> core::result::Result<(), ClosedError> {
        unsafe { handle.register_accelerator_window(hwnd) }
    }

    /// Removes a previously registered host window from accelerator routing.
    ///
    /// # Safety
    ///
    /// `hwnd` must be the same handle previously passed to
    /// [`register_accelerator_window`].
    pub unsafe fn unregister_accelerator_window<T: Tray>(
        handle: &Handle<T>,
        hwnd: HWND,
    ) -> core::result::Result<(), ClosedError> {
        unsafe { handle.unregister_accelerator_window(hwnd) }
    }
}
