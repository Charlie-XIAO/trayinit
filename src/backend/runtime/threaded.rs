use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

use crate::backend::BackendCommand;
use crate::{TrayError, TrayResult};

#[derive(Clone)]
pub(crate) struct BackendCommandSender {
    sender: Sender<BackendCommand>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

pub(crate) struct BackendRuntime {
    sender: BackendCommandSender,
    join: Option<JoinHandle<()>>,
}

impl BackendCommandSender {
    pub(crate) fn new(sender: Sender<BackendCommand>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { sender, wake }
    }

    pub(crate) fn send(&self, command: BackendCommand) -> TrayResult<()> {
        self.sender
            .send(command)
            .map_err(|_| TrayError::CommandQueueClosed)?;
        (self.wake)();
        Ok(())
    }
}

impl BackendRuntime {
    pub(crate) fn new(
        sender: Sender<BackendCommand>,
        wake: Arc<dyn Fn() + Send + Sync>,
        join: JoinHandle<()>,
    ) -> Self {
        Self {
            sender: BackendCommandSender::new(sender, wake),
            join: Some(join),
        }
    }

    pub(crate) fn sender(&self) -> BackendCommandSender {
        self.sender.clone()
    }

    pub(crate) fn shutdown(&mut self) -> TrayResult<()> {
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
    use crate::{Menu, MenuNode, TrayEvent, TrayState};

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
        let handle = crate::TrayHandle::new(sender, last.clone());

        let sink = move |_event: TrayEvent| {
            handle
                .set_state(TrayState::new().with_menu(Menu::new([MenuNode::item("quit", "Quit")])))
                .unwrap();
        };

        sink(TrayEvent::MenuItemActivated {
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
}
