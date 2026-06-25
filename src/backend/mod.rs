pub(crate) mod plan;

use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::{InvalidState, Menu, MenuItemId, MenuNode, TrayError, TrayResult, TrayState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BackendCommand {
    SetState(TrayState),
    Close,
}

#[derive(Clone)]
pub(crate) struct BackendCommandSender {
    sender: Sender<BackendCommand>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

pub(crate) struct BackendProxy {
    sender: BackendCommandSender,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl BackendCommandSender {
    pub(crate) fn send(&self, command: BackendCommand) -> TrayResult<()> {
        self.sender
            .send(command)
            .map_err(|_| TrayError::CommandQueueClosed)?;
        (self.wake)();
        Ok(())
    }
}

impl BackendProxy {
    pub(crate) fn new(
        sender: Sender<BackendCommand>,
        wake: Arc<dyn Fn() + Send + Sync>,
        join: JoinHandle<()>,
    ) -> Self {
        Self {
            sender: BackendCommandSender { sender, wake },
            join: Mutex::new(Some(join)),
        }
    }

    pub(crate) fn sender(&self) -> BackendCommandSender {
        self.sender.clone()
    }

    pub(crate) fn close_and_join(&self) -> TrayResult<()> {
        let join = self
            .join
            .lock()
            .map_err(|_| TrayError::BackendUnavailable("join lock poisoned".into()))?
            .take();

        let Some(join) = join else {
            return Ok(());
        };

        let send_result = self.sender.send(BackendCommand::Close);

        if join.join().is_err() {
            return Err(TrayError::BackendUnavailable(
                "backend thread panicked during shutdown".into(),
            ));
        }

        send_result
    }
}

pub(crate) fn validate_state(state: &TrayState) -> TrayResult<()> {
    if let Some(icon) = &state.icon {
        crate::icon::validate_rgba(icon.rgba(), icon.width(), icon.height())
            .map_err(InvalidState::InvalidIcon)?;
    }

    if let Some(menu) = &state.menu {
        validate_menu(menu)?;
    }

    Ok(())
}

pub(crate) fn validate_menu(menu: &Menu) -> TrayResult<()> {
    let mut seen = HashSet::new();
    validate_nodes(menu.nodes(), &mut seen)
}

fn validate_nodes(nodes: &[MenuNode], seen: &mut HashSet<MenuItemId>) -> TrayResult<()> {
    for node in nodes {
        match node {
            MenuNode::Item(item) => validate_id(&item.id, seen)?,
            MenuNode::Check(item) => validate_id(&item.id, seen)?,
            MenuNode::Submenu(submenu) => {
                if let Some(id) = &submenu.id {
                    validate_id(id, seen)?;
                }
                validate_nodes(&submenu.children, seen)?;
            },
            MenuNode::Separator => {},
        }
    }
    Ok(())
}

fn validate_id(id: &MenuItemId, seen: &mut HashSet<MenuItemId>) -> TrayResult<()> {
    if !id.is_valid() {
        return Err(InvalidState::EmptyMenuItemId.into());
    }

    if !seen.insert(id.clone()) {
        return Err(InvalidState::DuplicateMenuItemId(id.clone()).into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, mpsc};

    use super::*;
    use crate::{Menu, MenuNode, TrayEvent};

    #[test]
    fn redundant_state_update_is_not_queued() {
        let (tx, rx) = mpsc::channel();
        let wake_count = Arc::new(Mutex::new(0usize));
        let wake_count_for_sender = wake_count.clone();
        let sender = BackendCommandSender {
            sender: tx,
            wake: Arc::new(move || {
                *wake_count_for_sender.lock().unwrap() += 1;
            }),
        };
        let mut last = Some(TrayState::new());
        let state = TrayState::new();

        crate::tray::set_state_inner(&sender, &mut last, state).unwrap();

        assert!(rx.try_recv().is_err());
        assert_eq!(*wake_count.lock().unwrap(), 0);
    }

    #[test]
    fn invalid_state_update_is_not_queued() {
        let (tx, rx) = mpsc::channel();
        let sender = BackendCommandSender {
            sender: tx,
            wake: Arc::new(|| {}),
        };
        let mut last = Some(TrayState::new());
        let state = TrayState::new().with_menu(Menu::new([MenuNode::item("", "Empty")]));

        assert!(crate::tray::set_state_inner(&sender, &mut last, state).is_err());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn event_sink_can_update_state_immediately() {
        let (tx, rx) = mpsc::channel();
        let sender = BackendCommandSender {
            sender: tx,
            wake: Arc::new(|| {}),
        };
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
}
