#[cfg(target_os = "windows")]
#[path = "platform/windows/mod.rs"]
mod platform;

#[cfg(not(target_os = "windows"))]
#[path = "platform/unimplemented.rs"]
mod platform;

pub(crate) use platform::PlatformHandle;

pub(crate) fn attach<T: crate::Tray>(
    builder: crate::Builder<T>,
) -> crate::Result<crate::Handle<T>> {
    // TODO: This should be changed as we implement more stuff
    platform::spawn(builder)
}

pub(crate) fn spawn<T: crate::Tray>(builder: crate::Builder<T>) -> crate::Result<crate::Handle<T>> {
    platform::spawn(builder)
}

pub(crate) fn run<T: crate::Tray>(builder: crate::Builder<T>) -> crate::Result<()> {
    let _ = builder;
    Err(crate::Error::NotImplemented)
}
