use std::sync::{Arc, Mutex};

use crate::backend::{self, BackendCommand, BackendCommandSender, BackendProxy};
use crate::{EventSink, TrayError, TrayResult, TrayState};

pub struct Tray {
    backend: BackendProxy,
    last_state: Arc<Mutex<Option<TrayState>>>,
}

#[derive(Clone)]
pub struct TrayHandle {
    sender: BackendCommandSender,
    last_state: Arc<Mutex<Option<TrayState>>>,
}

impl Tray {
    pub fn new<S>(initial_state: TrayState, sink: S) -> TrayResult<Self>
    where
        S: EventSink,
    {
        Self::new_with_sink(initial_state, Arc::new(sink))
    }

    pub fn new_with_sink(initial_state: TrayState, sink: Arc<dyn EventSink>) -> TrayResult<Self> {
        backend::validate_state(&initial_state)?;
        let backend = crate::platform::spawn(initial_state.clone(), sink)?;
        Ok(Self {
            backend,
            last_state: Arc::new(Mutex::new(Some(initial_state))),
        })
    }

    pub fn handle(&self) -> TrayHandle {
        TrayHandle::new(self.backend.sender(), self.last_state.clone())
    }

    pub fn set_state(&self, state: TrayState) -> TrayResult<()> {
        self.handle().set_state(state)
    }

    pub fn shutdown(self) -> TrayResult<()> {
        self.backend.close_and_join()
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let _ = self.backend.close_and_join();
    }
}

impl TrayHandle {
    pub(crate) fn new(
        sender: BackendCommandSender,
        last_state: Arc<Mutex<Option<TrayState>>>,
    ) -> Self {
        Self { sender, last_state }
    }

    pub fn set_state(&self, state: TrayState) -> TrayResult<()> {
        let mut last_state = self
            .last_state
            .lock()
            .map_err(|_| TrayError::BackendUnavailable("state lock poisoned".into()))?;
        set_state_inner(&self.sender, &mut last_state, state)
    }

    pub fn close(&self) -> TrayResult<()> {
        self.sender.send(BackendCommand::Close)
    }
}

pub(crate) fn set_state_inner(
    sender: &BackendCommandSender,
    last_state: &mut Option<TrayState>,
    state: TrayState,
) -> TrayResult<()> {
    backend::validate_state(&state)?;

    if last_state.as_ref() == Some(&state) {
        return Ok(());
    }

    sender.send(BackendCommand::SetState(state.clone()))?;
    *last_state = Some(state);
    Ok(())
}
