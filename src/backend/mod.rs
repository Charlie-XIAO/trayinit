#[cfg(windows)]
#[path = "windows/mod.rs"]
mod platform;

#[cfg(windows)]
pub(crate) use platform::PlatformHandle;

#[cfg(windows)]
pub(crate) fn attach<T: crate::Tray>(
    builder: crate::Builder<T>,
) -> crate::Result<crate::Handle<T>> {
    // For the current Windows backend, host-integrated mode can reuse the same
    // worker-thread implementation. The public API is separated now so a
    // future current-thread Win32 backend can slot in without changing callers.
    platform::spawn(builder)
}

#[cfg(windows)]
pub(crate) fn spawn<T: crate::Tray>(builder: crate::Builder<T>) -> crate::Result<crate::Handle<T>> {
    platform::spawn(builder)
}

#[cfg(not(windows))]
pub(crate) fn attach<T: crate::Tray>(
    builder: crate::Builder<T>,
) -> crate::Result<crate::Handle<T>> {
    let _ = builder;
    todo!("tray backend is not implemented for this platform yet")
}

#[cfg(not(windows))]
pub(crate) fn spawn<T: crate::Tray>(builder: crate::Builder<T>) -> crate::Result<crate::Handle<T>> {
    let _ = builder;
    todo!("tray backend is not implemented for this platform yet")
}

pub(crate) fn run<T: crate::Tray>(builder: crate::Builder<T>) -> crate::Result<()> {
    let _ = builder;
    Err(crate::Error::NotImplemented)
}
