#[cfg(any(target_os = "windows", target_os = "linux"))]
use std::sync::Arc;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use std::sync::mpsc::Sender;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use std::thread::JoinHandle;

use super::{BackendCommand, BackendCommandSender};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::TrayError;
use crate::TrayResult;

enum BackendRuntimeInner {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    Threaded {
        sender: BackendCommandSender,
        join: Option<JoinHandle<()>>,
    },
    #[cfg(target_os = "macos")]
    Direct {
        sender: Option<BackendCommandSender>,
    },
}

pub(crate) struct BackendRuntime {
    inner: BackendRuntimeInner,
}

impl BackendRuntime {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub(crate) fn threaded(
        sender: Sender<BackendCommand>,
        wake: Arc<dyn Fn() + Send + Sync>,
        join: JoinHandle<()>,
    ) -> Self {
        let sender = BackendCommandSender::channel(sender, wake);
        Self {
            inner: BackendRuntimeInner::Threaded {
                sender,
                join: Some(join),
            },
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn direct(sender: BackendCommandSender) -> Self {
        Self {
            inner: BackendRuntimeInner::Direct {
                sender: Some(sender),
            },
        }
    }

    pub(crate) fn sender(&self) -> BackendCommandSender {
        match &self.inner {
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            BackendRuntimeInner::Threaded { sender, .. } => sender.clone(),
            #[cfg(target_os = "macos")]
            BackendRuntimeInner::Direct { sender } => sender
                .as_ref()
                .expect("backend sender requested after shutdown")
                .clone(),
        }
    }

    pub(crate) fn shutdown(&mut self) -> TrayResult<()> {
        match &mut self.inner {
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            BackendRuntimeInner::Threaded { sender, join } => {
                let Some(join) = join.take() else {
                    return Ok(());
                };

                let send_result = sender.send(BackendCommand::Close);

                if join.join().is_err() {
                    return Err(TrayError::BackendUnavailable(
                        "backend thread panicked during shutdown".into(),
                    ));
                }

                send_result
            },
            #[cfg(target_os = "macos")]
            BackendRuntimeInner::Direct { sender } => {
                let Some(command_sender) = sender.as_ref() else {
                    return Ok(());
                };
                command_sender.send(BackendCommand::Close)?;
                *sender = None;
                Ok(())
            },
        }
    }
}
