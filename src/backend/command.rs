use crate::TrayState;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendCommand {
    SetState(TrayState),
    Close,
}
