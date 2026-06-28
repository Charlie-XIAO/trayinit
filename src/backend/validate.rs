use std::collections::HashSet;

use crate::{InvalidState, Menu, MenuItemId, MenuNode, TrayResult, TrayState};

pub fn validate_state(state: &TrayState) -> TrayResult<()> {
    if let Some(icon) = &state.icon {
        crate::icon::validate_rgba(icon.rgba(), icon.width(), icon.height())
            .map_err(InvalidState::InvalidIcon)?;
    }

    if let Some(menu) = &state.menu {
        validate_menu(menu)?;
    }

    Ok(())
}

pub fn validate_menu(menu: &Menu) -> TrayResult<()> {
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
