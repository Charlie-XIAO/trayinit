use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::backend::{self, BackendCommand, BackendCommandSender, BackendRuntime};
use crate::platform::PlatformOptions;
use crate::{EventSink, TrayError, TrayId, TrayOptions, TrayResult, TrayState};

static TRAY_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);

pub struct Tray {
    id: TrayId,
    backend: BackendRuntime,
    last_state: Arc<Mutex<Option<TrayState>>>,
}

#[derive(Clone)]
pub struct TrayHandle {
    id: TrayId,
    sender: BackendCommandSender,
    last_state: Arc<Mutex<Option<TrayState>>>,
}

impl Tray {
    pub fn new<S>(initial_state: TrayState, sink: S) -> TrayResult<Self>
    where
        S: EventSink,
    {
        Self::new_with_options(initial_state, sink, TrayOptions::new())
    }

    pub fn new_with_options<S>(
        initial_state: TrayState,
        sink: S,
        options: TrayOptions,
    ) -> TrayResult<Self>
    where
        S: EventSink,
    {
        backend::validate_state(&initial_state)?;
        let (id, platform_options) = resolve_options(options)?;

        let backend = crate::platform::spawn(
            initial_state.clone(),
            Arc::new(sink),
            platform_options,
            id.clone(),
        )?;
        Ok(Self {
            id,
            backend,
            last_state: Arc::new(Mutex::new(Some(initial_state))),
        })
    }

    pub fn handle(&self) -> TrayHandle {
        TrayHandle::new(
            self.id.clone(),
            self.backend.sender(),
            self.last_state.clone(),
        )
    }

    pub fn id(&self) -> &TrayId {
        &self.id
    }

    pub fn set_state(&self, state: TrayState) -> TrayResult<()> {
        self.handle().set_state(state)
    }

    pub fn shutdown(mut self) -> TrayResult<()> {
        self.backend.shutdown()
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let _ = self.backend.shutdown();
    }
}

impl TrayHandle {
    pub(crate) fn new(
        id: TrayId,
        sender: BackendCommandSender,
        last_state: Arc<Mutex<Option<TrayState>>>,
    ) -> Self {
        Self {
            id,
            sender,
            last_state,
        }
    }

    pub fn id(&self) -> &TrayId {
        &self.id
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

fn next_tray_id() -> TrayId {
    let id = TRAY_ID_COUNTER.fetch_add(1, Ordering::AcqRel);
    TrayId::new(format!("tray-{id}"))
}

fn resolve_options(options: TrayOptions) -> TrayResult<(TrayId, PlatformOptions)> {
    let TrayOptions { id, platform } = options;

    let id = id.unwrap_or_else(next_tray_id);
    if !id.is_valid() {
        return Err(TrayError::InvalidTrayId);
    }
    Ok((id, platform))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tray_ids_are_distinct() {
        let (first, _) = resolve_options(TrayOptions::new()).unwrap();
        let (second, _) = resolve_options(TrayOptions::new()).unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn explicit_tray_id_is_preserved() {
        let (id, _) = resolve_options(TrayOptions::new().with_id("main")).unwrap();

        assert_eq!(id.as_str(), "main");
    }

    #[test]
    fn blank_explicit_tray_id_is_rejected() {
        let err = resolve_options(TrayOptions::new().with_id("   ")).unwrap_err();

        assert_eq!(err, TrayError::InvalidTrayId);
    }
}
