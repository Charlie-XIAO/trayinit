use crate::TrayState;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BackendCommand {
    SetState(TrayState),
    Close,
}
