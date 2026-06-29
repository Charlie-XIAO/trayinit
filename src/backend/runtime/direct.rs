use std::cell::Cell;
use std::rc::Rc;

use crate::backend::BackendCommand;
use crate::{TrayError, TrayResult};

#[derive(Clone)]
pub struct BackendCommandSender {
    inner: Rc<DirectCommandSender>,
}

struct DirectCommandSender {
    closed: Cell<bool>,
    dispatch: Rc<dyn Fn(BackendCommand) -> TrayResult<()>>,
}

pub struct BackendRuntime {
    sender: Option<BackendCommandSender>,
}

impl BackendCommandSender {
    pub fn new(dispatch: Rc<dyn Fn(BackendCommand) -> TrayResult<()>>) -> Self {
        Self {
            inner: Rc::new(DirectCommandSender {
                closed: Cell::new(false),
                dispatch,
            }),
        }
    }

    pub fn send(&self, command: BackendCommand) -> TrayResult<()> {
        self.inner.send(command)
    }
}

impl DirectCommandSender {
    fn send(&self, command: BackendCommand) -> TrayResult<()> {
        let close = matches!(command, BackendCommand::Close);

        if self.closed.get() {
            return if close {
                Ok(())
            } else {
                Err(TrayError::CommandQueueClosed)
            };
        }

        (self.dispatch)(command)?;
        if close {
            self.closed.set(true);
        }
        Ok(())
    }
}

impl BackendRuntime {
    pub fn new(sender: BackendCommandSender) -> Self {
        Self {
            sender: Some(sender),
        }
    }

    pub fn sender(&self) -> BackendCommandSender {
        self.sender
            .as_ref()
            .expect("backend sender requested after shutdown")
            .clone()
    }

    pub fn shutdown(&mut self) -> TrayResult<()> {
        let Some(sender) = self.sender.as_ref() else {
            return Ok(());
        };
        sender.send(BackendCommand::Close)?;
        self.sender = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::{TrayId, TrayState};

    #[test]
    fn close_is_shared_and_idempotent_across_clones() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen_for_sender = seen.clone();
        let sender = BackendCommandSender::new(Rc::new(move |command| {
            seen_for_sender.borrow_mut().push(command);
            Ok(())
        }));
        let clone = sender.clone();

        clone.send(BackendCommand::Close).unwrap();
        sender.send(BackendCommand::Close).unwrap();

        assert_eq!(seen.borrow().as_slice(), &[BackendCommand::Close]);
    }

    #[test]
    fn set_state_after_close_is_rejected_across_clones() {
        let sender = BackendCommandSender::new(Rc::new(|_command| Ok(())));
        let clone = sender.clone();

        sender.send(BackendCommand::Close).unwrap();

        assert_eq!(
            clone.send(BackendCommand::SetState(TrayState::new())),
            Err(TrayError::CommandQueueClosed)
        );
    }

    #[test]
    fn tray_handle_set_state_after_close_is_rejected() {
        let sender = BackendCommandSender::new(Rc::new(|_command| Ok(())));
        let handle = crate::TrayHandle::new(
            TrayId::new("test"),
            sender.clone(),
            Arc::new(Mutex::new(None)),
        );

        sender.send(BackendCommand::Close).unwrap();

        assert_eq!(
            handle.set_state(TrayState::new()),
            Err(TrayError::CommandQueueClosed)
        );
    }
}
