use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

use crate::backend::BackendCommand;
use crate::{TrayError, TrayResult};

#[derive(Clone)]
pub struct BackendCommandSender {
    sender: Sender<BackendCommand>,
    wake: Arc<dyn Fn() + Send + Sync>,
    closed: Arc<AtomicBool>,
}

pub struct BackendRuntime {
    sender: BackendCommandSender,
    join: Option<JoinHandle<()>>,
}

impl BackendCommandSender {
    pub fn new(sender: Sender<BackendCommand>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            sender,
            wake,
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn send(&self, command: BackendCommand) -> TrayResult<()> {
        let close = matches!(command, BackendCommand::Close);

        if close {
            if self.closed.swap(true, Ordering::AcqRel) {
                return Ok(());
            }
        } else if self.closed.load(Ordering::Acquire) {
            return Err(TrayError::CommandQueueClosed);
        }

        self.sender
            .send(command)
            .map_err(|_| TrayError::CommandQueueClosed)?;
        (self.wake)();
        Ok(())
    }
}

impl BackendRuntime {
    pub fn new(
        sender: Sender<BackendCommand>,
        wake: Arc<dyn Fn() + Send + Sync>,
        join: JoinHandle<()>,
    ) -> Self {
        Self {
            sender: BackendCommandSender::new(sender, wake),
            join: Some(join),
        }
    }

    pub fn sender(&self) -> BackendCommandSender {
        self.sender.clone()
    }

    pub fn shutdown(&mut self) -> TrayResult<()> {
        let Some(join) = self.join.take() else {
            return Ok(());
        };

        let send_result = self.sender.send(BackendCommand::Close);

        if join.join().is_err() {
            return Err(TrayError::BackendUnavailable(
                "backend thread panicked during shutdown".into(),
            ));
        }

        match send_result {
            Err(TrayError::CommandQueueClosed) => Ok(()),
            result => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;

    use super::*;
    use crate::{Menu, MenuNode, TrayError, TrayEvent, TrayId, TrayState};

    #[test]
    fn redundant_state_update_is_not_queued() {
        let (tx, rx) = mpsc::channel();
        let wake_count = Arc::new(Mutex::new(0usize));
        let wake_count_for_sender = wake_count.clone();
        let sender = BackendCommandSender::new(
            tx,
            Arc::new(move || {
                *wake_count_for_sender.lock().unwrap() += 1;
            }),
        );
        let mut last = Some(TrayState::new());
        let state = TrayState::new();

        crate::tray::set_state_inner(&sender, &mut last, state).unwrap();

        assert!(rx.try_recv().is_err());
        assert_eq!(*wake_count.lock().unwrap(), 0);
    }

    #[test]
    fn invalid_state_update_is_not_queued() {
        let (tx, rx) = mpsc::channel();
        let sender = BackendCommandSender::new(tx, Arc::new(|| {}));
        let mut last = Some(TrayState::new());
        let state = TrayState::new().with_menu(Menu::new([MenuNode::item("", "Empty")]));

        assert!(crate::tray::set_state_inner(&sender, &mut last, state).is_err());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn event_sink_can_update_state_immediately() {
        let (tx, rx) = mpsc::channel();
        let sender = BackendCommandSender::new(tx, Arc::new(|| {}));
        let last = Arc::new(Mutex::new(Some(TrayState::new())));
        let handle = crate::TrayHandle::new(TrayId::new("test"), sender, last.clone());

        let sink = move |_event: TrayEvent| {
            handle
                .set_state(TrayState::new().with_menu(Menu::new([MenuNode::item("quit", "Quit")])))
                .unwrap();
        };

        sink(TrayEvent::MenuItemActivated {
            tray_id: TrayId::new("test"),
            item_id: "open".into(),
        });

        assert!(matches!(rx.try_recv(), Ok(BackendCommand::SetState(_))));
    }

    #[test]
    fn shutdown_succeeds_if_thread_already_exited() {
        let (tx, rx) = mpsc::channel();
        let join = thread::spawn(move || {
            drop(rx);
        });
        let mut runtime = BackendRuntime::new(tx, Arc::new(|| {}), join);

        assert_eq!(runtime.shutdown(), Ok(()));
        assert_eq!(runtime.shutdown(), Ok(()));
    }

    #[test]
    fn close_is_shared_and_idempotent_across_clones() {
        let (tx, rx) = mpsc::channel();
        let wake_count = Arc::new(Mutex::new(0usize));
        let wake_count_for_sender = wake_count.clone();
        let sender = BackendCommandSender::new(
            tx,
            Arc::new(move || {
                *wake_count_for_sender.lock().unwrap() += 1;
            }),
        );
        let clone = sender.clone();

        clone.send(BackendCommand::Close).unwrap();
        sender.send(BackendCommand::Close).unwrap();

        assert_eq!(rx.try_recv(), Ok(BackendCommand::Close));
        assert!(rx.try_recv().is_err());
        assert_eq!(*wake_count.lock().unwrap(), 1);
    }

    #[test]
    fn set_state_after_close_is_rejected_across_clones() {
        let (tx, rx) = mpsc::channel();
        let sender = BackendCommandSender::new(tx, Arc::new(|| {}));
        let clone = sender.clone();

        sender.send(BackendCommand::Close).unwrap();

        assert_eq!(
            clone.send(BackendCommand::SetState(TrayState::new())),
            Err(TrayError::CommandQueueClosed)
        );
        assert_eq!(rx.try_recv(), Ok(BackendCommand::Close));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn tray_handle_set_state_after_close_is_rejected_without_updating_last_state() {
        let (tx, rx) = mpsc::channel();
        let sender = BackendCommandSender::new(tx, Arc::new(|| {}));
        let last_state = Arc::new(Mutex::new(Some(TrayState::new())));
        let handle =
            crate::TrayHandle::new(TrayId::new("test"), sender.clone(), last_state.clone());
        let next_state = TrayState::new().with_menu(Menu::new([MenuNode::item("quit", "Quit")]));

        sender.send(BackendCommand::Close).unwrap();

        assert_eq!(
            handle.set_state(next_state),
            Err(TrayError::CommandQueueClosed)
        );
        assert_eq!(*last_state.lock().unwrap(), Some(TrayState::new()));
        assert_eq!(rx.try_recv(), Ok(BackendCommand::Close));
        assert!(rx.try_recv().is_err());
    }
}
