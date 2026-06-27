#[cfg(target_os = "macos")]
use std::cell::Cell;
#[cfg(target_os = "macos")]
use std::rc::Rc;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use std::sync::Arc;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use std::sync::mpsc::Sender;

use crate::{TrayError, TrayResult, TrayState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BackendCommand {
    SetState(TrayState),
    Close,
}

#[derive(Clone)]
pub(crate) struct BackendCommandSender {
    inner: BackendCommandSenderInner,
}

#[derive(Clone)]
enum BackendCommandSenderInner {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    Channel {
        sender: Sender<BackendCommand>,
        wake: Arc<dyn Fn() + Send + Sync>,
    },
    #[cfg(target_os = "macos")]
    Direct(Rc<DirectCommandSender>),
}

#[cfg(target_os = "macos")]
struct DirectCommandSender {
    closed: Cell<bool>,
    dispatch: Rc<dyn Fn(BackendCommand) -> TrayResult<()>>,
}

impl BackendCommandSender {
    pub(crate) fn send(&self, command: BackendCommand) -> TrayResult<()> {
        match &self.inner {
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            BackendCommandSenderInner::Channel { sender, wake } => {
                sender
                    .send(command)
                    .map_err(|_| TrayError::CommandQueueClosed)?;
                (wake)();
                Ok(())
            },
            #[cfg(target_os = "macos")]
            BackendCommandSenderInner::Direct(sender) => sender.send(command),
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub(crate) fn channel(
        sender: Sender<BackendCommand>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            inner: BackendCommandSenderInner::Channel { sender, wake },
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn direct(dispatch: Rc<dyn Fn(BackendCommand) -> TrayResult<()>>) -> Self {
        Self {
            inner: BackendCommandSenderInner::Direct(Rc::new(DirectCommandSender {
                closed: Cell::new(false),
                dispatch,
            })),
        }
    }
}

#[cfg(target_os = "macos")]
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

#[cfg(test)]
mod tests {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    mod threaded {
        use std::sync::{Arc, Mutex, mpsc};

        use super::super::*;
        use crate::{Menu, MenuNode, TrayEvent, TrayState};

        #[test]
        fn redundant_state_update_is_not_queued() {
            let (tx, rx) = mpsc::channel();
            let wake_count = Arc::new(Mutex::new(0usize));
            let wake_count_for_sender = wake_count.clone();
            let sender = BackendCommandSender::channel(
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
            let sender = BackendCommandSender::channel(tx, Arc::new(|| {}));
            let mut last = Some(TrayState::new());
            let state = TrayState::new().with_menu(Menu::new([MenuNode::item("", "Empty")]));

            assert!(crate::tray::set_state_inner(&sender, &mut last, state).is_err());
            assert!(rx.try_recv().is_err());
        }

        #[test]
        fn event_sink_can_update_state_immediately() {
            let (tx, rx) = mpsc::channel();
            let sender = BackendCommandSender::channel(tx, Arc::new(|| {}));
            let last = Arc::new(Mutex::new(Some(TrayState::new())));
            let handle = crate::TrayHandle::new(sender, last.clone());

            let sink = move |_event: TrayEvent| {
                handle
                    .set_state(
                        TrayState::new().with_menu(Menu::new([MenuNode::item("quit", "Quit")])),
                    )
                    .unwrap();
            };

            sink(TrayEvent::MenuItemActivated {
                item_id: "open".into(),
            });

            assert!(matches!(rx.try_recv(), Ok(BackendCommand::SetState(_))));
        }
    }

    #[cfg(target_os = "macos")]
    mod direct {
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::sync::{Arc, Mutex};

        use super::super::*;
        use crate::{TrayError, TrayState};

        #[test]
        fn close_is_shared_and_idempotent_across_clones() {
            let seen = Rc::new(RefCell::new(Vec::new()));
            let seen_for_sender = seen.clone();
            let sender = BackendCommandSender::direct(Rc::new(move |command| {
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
            let sender = BackendCommandSender::direct(Rc::new(|_command| Ok(())));
            let clone = sender.clone();

            sender.send(BackendCommand::Close).unwrap();

            assert_eq!(
                clone.send(BackendCommand::SetState(TrayState::new())),
                Err(TrayError::CommandQueueClosed)
            );
        }

        #[test]
        fn tray_handle_set_state_after_close_is_rejected() {
            let sender = BackendCommandSender::direct(Rc::new(|_command| Ok(())));
            let handle = crate::TrayHandle::new(sender.clone(), Arc::new(Mutex::new(None)));

            sender.send(BackendCommand::Close).unwrap();

            assert_eq!(
                handle.set_state(TrayState::new()),
                Err(TrayError::CommandQueueClosed)
            );
        }
    }
}
