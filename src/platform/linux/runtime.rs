use std::future::Future;
use std::time::Duration;

use crate::TrayResult;

#[cfg(feature = "linux-zbus-async-io")]
pub type Mutex<T> = async_lock::Mutex<T>;

#[cfg(feature = "linux-zbus-tokio")]
pub type Mutex<T> = tokio::sync::Mutex<T>;

#[cfg(feature = "linux-zbus-async-io")]
pub fn run<F>(future: F) -> TrayResult<()>
where
    F: Future<Output = ()>,
{
    async_io::block_on(future);
    Ok(())
}

#[cfg(feature = "linux-zbus-tokio")]
pub fn run<F>(future: F) -> TrayResult<()>
where
    F: Future<Output = ()>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|err| crate::TrayError::ThreadInit(err.to_string()))?;

    runtime.block_on(future);
    Ok(())
}

#[cfg(feature = "linux-zbus-async-io")]
pub async fn sleep(duration: Duration) {
    async_io::Timer::after(duration).await;
}

#[cfg(feature = "linux-zbus-tokio")]
pub async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}
