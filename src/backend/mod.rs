#[cfg(windows)]
#[path = "windows/mod.rs"]
mod platform;

#[cfg(windows)]
pub(crate) use platform::PlatformHandle;

#[cfg(windows)]
pub(crate) fn spawn<T: crate::Tray>(builder: crate::Builder<T>) -> crate::Result<crate::Handle<T>> {
    platform::spawn(builder)
}

#[cfg(not(windows))]
pub(crate) fn spawn<T: crate::Tray>(builder: crate::Builder<T>) -> crate::Result<crate::Handle<T>> {
    let _ = builder;
    todo!("tray backend is not implemented for this platform yet")
}
