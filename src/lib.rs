mod backend;
mod error;
mod event;
mod icon;
mod menu;
mod model;
mod tray;

pub use error::{InvalidState, TrayError, TrayResult};
pub use event::{ChannelEventSink, EventSink, TrayEvent, TrayIconEventKind, TrayStatus, channel};
pub use icon::{Icon, IconError};
pub use menu::{CheckItem, Menu, MenuItem, MenuItemId, MenuNode, Submenu};
pub use model::{ActivationMode, PhysicalPosition, PhysicalRect, TrayState};
pub use tray::{Tray, TrayHandle};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_minimal_state() {
        let state = TrayState::new();
        assert!(backend::validate_state(&state).is_ok());
    }

    #[test]
    fn default_state_matches_new_state() {
        assert_eq!(TrayState::default(), TrayState::new());
    }

    #[test]
    fn invalid_icon_length_is_rejected() {
        let err = Icon::from_rgba(vec![0; 3], 1, 1).unwrap_err();
        assert_eq!(
            err,
            IconError::InvalidRgbaLength {
                expected: 4,
                actual: 3
            }
        );
    }

    #[test]
    fn duplicate_action_ids_are_rejected() {
        let state = TrayState::new().with_menu(Menu::new([
            MenuNode::item("open", "Open"),
            MenuNode::check("open", "Enabled", true),
        ]));

        assert!(matches!(
            backend::validate_state(&state),
            Err(TrayError::InvalidState(InvalidState::DuplicateMenuItemId(id))) if id.as_str() == "open"
        ));
    }

    #[test]
    fn optional_submenu_id_is_allowed_and_generated_in_plan() {
        let menu = Menu::new([MenuNode::submenu(
            "More",
            [MenuNode::item("about", "About")],
        )]);

        let plan = backend::plan::plan_menu(&menu).unwrap();
        assert_eq!(plan.nodes.len(), 1);
        assert!(plan.nodes[0].explicit_id.is_none());
        assert_eq!(plan.command_map.len(), 1);
    }

    #[test]
    fn explicit_submenu_id_participates_in_duplicate_validation() {
        let state = TrayState::new().with_menu(Menu::new([
            MenuNode::submenu_with_id("tools", "Tools", [MenuNode::item("about", "About")]),
            MenuNode::item("tools", "Tools item"),
        ]));

        assert!(matches!(
            backend::validate_state(&state),
            Err(TrayError::InvalidState(InvalidState::DuplicateMenuItemId(id))) if id.as_str() == "tools"
        ));
    }

    #[test]
    fn separators_do_not_consume_identity() {
        let state = TrayState::new().with_menu(Menu::new([
            MenuNode::separator(),
            MenuNode::item("quit", "Quit"),
            MenuNode::separator(),
        ]));

        assert!(backend::validate_state(&state).is_ok());
    }

    #[test]
    fn menu_plan_maps_actionable_items_only() {
        let menu = Menu::new([
            MenuNode::item("open", "Open"),
            MenuNode::separator(),
            MenuNode::check("enabled", "Enabled", false),
            MenuNode::submenu("More", [MenuNode::item("about", "About")]),
        ]);

        let plan = backend::plan::plan_menu(&menu).unwrap();
        let mut command_pairs: Vec<_> = plan
            .command_map
            .iter()
            .map(|(command_id, id)| (*command_id, id.as_str()))
            .collect();
        command_pairs.sort_by_key(|(command_id, _)| *command_id);
        let ids: Vec<_> = command_pairs.into_iter().map(|(_, id)| id).collect();
        assert_eq!(ids, ["open", "enabled", "about"]);
    }
}
