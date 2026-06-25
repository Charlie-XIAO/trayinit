//! Pure DBusMenu planning and property mapping.
//!
//! This is split out of the Linux backend because DBusMenu has a serialized,
//! testable tree model with stable integer IDs. Windows menu construction is
//! currently coupled to live Win32 `HMENU` handles, so its equivalent planning
//! remains in `platform::windows` until there is a useful pure layer to
//! extract.

use std::collections::HashMap;

use crate::backend::plan::{PlannedNode, PlannedNodeKind, plan_menu};
use crate::{Menu, MenuItemId, TrayResult};

pub(crate) const ROOT_ID: i32 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinuxMenu {
    pub(crate) revision: u32,
    root: LinuxMenuNode,
    action_map: HashMap<i32, MenuItemId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinuxMenuNode {
    pub(crate) id: i32,
    pub(crate) properties: LinuxMenuProperties,
    pub(crate) children: Vec<LinuxMenuNode>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LinuxMenuProperties {
    pub(crate) item_type: Option<&'static str>,
    pub(crate) label: Option<String>,
    pub(crate) enabled: Option<bool>,
    pub(crate) visible: Option<bool>,
    pub(crate) toggle_type: Option<&'static str>,
    pub(crate) toggle_state: Option<i32>,
    pub(crate) children_display: Option<&'static str>,
}

impl LinuxMenu {
    pub(crate) fn empty(revision: u32) -> Self {
        Self {
            revision,
            root: LinuxMenuNode {
                id: ROOT_ID,
                properties: LinuxMenuProperties::default(),
                children: Vec::new(),
            },
            action_map: HashMap::new(),
        }
    }

    pub(crate) fn from_menu(menu: Option<&Menu>, revision: u32) -> TrayResult<Self> {
        let Some(menu) = menu else {
            return Ok(Self::empty(revision));
        };

        let plan = plan_menu(menu)?;
        let mut action_map = HashMap::new();
        let children = plan
            .nodes
            .iter()
            .map(|node| convert_node(node, &mut action_map))
            .collect();
        let mut root = LinuxMenuNode {
            id: ROOT_ID,
            properties: LinuxMenuProperties::default(),
            children,
        };
        if !root.children.is_empty() {
            root.properties.children_display = Some("submenu");
        }

        Ok(Self {
            revision,
            root,
            action_map,
        })
    }

    pub(crate) fn action_for(&self, id: i32) -> Option<MenuItemId> {
        self.action_map.get(&id).cloned()
    }

    pub(crate) fn layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: &[String],
    ) -> Option<LinuxMenuNode> {
        let node = self.find_node(parent_id)?;
        Some(filter_node(
            node,
            if recursion_depth < 0 {
                None
            } else {
                Some(recursion_depth as usize)
            },
            property_names,
        ))
    }

    pub(crate) fn properties(
        &self,
        id: i32,
        property_names: &[String],
    ) -> Option<LinuxMenuProperties> {
        self.find_node(id)
            .map(|node| node.properties.filtered(property_names))
    }

    fn find_node(&self, id: i32) -> Option<&LinuxMenuNode> {
        find_node(&self.root, id)
    }
}

impl LinuxMenuProperties {
    pub(crate) fn is_empty(&self) -> bool {
        self.item_type.is_none()
            && self.label.is_none()
            && self.enabled.is_none()
            && self.visible.is_none()
            && self.toggle_type.is_none()
            && self.toggle_state.is_none()
            && self.children_display.is_none()
    }

    pub(crate) fn filtered(&self, property_names: &[String]) -> Self {
        if property_names.is_empty() {
            return self.clone();
        }

        let contains = |name: &str| property_names.iter().any(|property| property == name);
        Self {
            item_type: self.item_type.filter(|_| contains("type")),
            label: self.label.clone().filter(|_| contains("label")),
            enabled: self.enabled.filter(|_| contains("enabled")),
            visible: self.visible.filter(|_| contains("visible")),
            toggle_type: self.toggle_type.filter(|_| contains("toggle-type")),
            toggle_state: self.toggle_state.filter(|_| contains("toggle-state")),
            children_display: self
                .children_display
                .filter(|_| contains("children-display")),
        }
    }
}

pub(crate) fn icon_rgba_to_argb(rgba: &[u8]) -> Vec<u8> {
    let mut argb = Vec::with_capacity(rgba.len());
    for pixel in rgba.chunks_exact(4) {
        argb.extend_from_slice(&[pixel[3], pixel[0], pixel[1], pixel[2]]);
    }
    argb
}

fn convert_node(node: &PlannedNode, action_map: &mut HashMap<i32, MenuItemId>) -> LinuxMenuNode {
    let id = i32::try_from(node.backend_id).expect("menu id overflow");
    let mut children: Vec<_> = node
        .children
        .iter()
        .map(|node| convert_node(node, action_map))
        .collect();

    let mut properties = match &node.kind {
        PlannedNodeKind::Item(item) => {
            if let Some(id) = &node.explicit_id {
                action_map.insert(
                    i32::try_from(node.backend_id).expect("menu id overflow"),
                    id.clone(),
                );
            }
            LinuxMenuProperties {
                item_type: Some("standard"),
                label: Some(item.label.clone()),
                enabled: Some(item.enabled),
                visible: Some(true),
                ..Default::default()
            }
        },
        PlannedNodeKind::Check(item) => {
            if let Some(id) = &node.explicit_id {
                action_map.insert(
                    i32::try_from(node.backend_id).expect("menu id overflow"),
                    id.clone(),
                );
            }
            LinuxMenuProperties {
                item_type: Some("standard"),
                label: Some(item.label.clone()),
                enabled: Some(item.enabled),
                visible: Some(true),
                toggle_type: Some("checkmark"),
                toggle_state: Some(if item.checked { 1 } else { 0 }),
                ..Default::default()
            }
        },
        PlannedNodeKind::Submenu(submenu) => LinuxMenuProperties {
            item_type: Some("standard"),
            label: Some(submenu.label.clone()),
            enabled: Some(submenu.enabled),
            visible: Some(true),
            ..Default::default()
        },
        PlannedNodeKind::Separator => LinuxMenuProperties {
            item_type: Some("separator"),
            visible: Some(true),
            ..Default::default()
        },
    };

    if !children.is_empty() {
        properties.children_display = Some("submenu");
    }

    LinuxMenuNode {
        id,
        properties,
        children: std::mem::take(&mut children),
    }
}

fn filter_node(
    node: &LinuxMenuNode,
    recursion_depth: Option<usize>,
    property_names: &[String],
) -> LinuxMenuNode {
    let children = if recursion_depth == Some(0) {
        Vec::new()
    } else {
        node.children
            .iter()
            .map(|child| {
                filter_node(
                    child,
                    recursion_depth.map(|depth| depth.saturating_sub(1)),
                    property_names,
                )
            })
            .collect()
    };

    LinuxMenuNode {
        id: node.id,
        properties: node.properties.filtered(property_names),
        children,
    }
}

fn find_node(node: &LinuxMenuNode, id: i32) -> Option<&LinuxMenuNode> {
    if node.id == id {
        return Some(node);
    }

    node.children.iter().find_map(|node| find_node(node, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Menu, MenuNode};

    #[test]
    fn converts_rgba_to_argb() {
        let argb = icon_rgba_to_argb(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(argb, [4, 1, 2, 3, 8, 5, 6, 7]);
    }

    #[test]
    fn builds_layout_for_menu_nodes() {
        let menu = Menu::new([
            MenuNode::item("open", "Open"),
            MenuNode::check("enabled", "Enabled", true),
            MenuNode::separator(),
            MenuNode::submenu("More", [MenuNode::item("about", "About")]),
        ]);
        let menu = LinuxMenu::from_menu(Some(&menu), 7).unwrap();
        let root = menu.layout(ROOT_ID, -1, &[]).unwrap();

        assert_eq!(menu.revision, 7);
        assert_eq!(root.id, ROOT_ID);
        assert_eq!(root.children.len(), 4);
        assert_eq!(root.children[0].properties.label.as_deref(), Some("Open"));
        assert_eq!(root.children[1].properties.toggle_type, Some("checkmark"));
        assert_eq!(root.children[1].properties.toggle_state, Some(1));
        assert_eq!(root.children[2].properties.item_type, Some("separator"));
        assert_eq!(
            root.children[3].properties.children_display,
            Some("submenu")
        );
    }

    #[test]
    fn maps_actionable_items_only() {
        let menu = Menu::new([
            MenuNode::submenu_with_id("submenu", "More", [MenuNode::item("about", "About")]),
            MenuNode::separator(),
        ]);
        let menu = LinuxMenu::from_menu(Some(&menu), 1).unwrap();

        assert_eq!(menu.action_map.len(), 1);
        assert!(menu.action_map.values().any(|id| id.as_str() == "about"));
        assert!(menu.action_for(1).is_none());
    }

    #[test]
    fn filters_properties() {
        let menu = Menu::new([MenuNode::item("open", "Open")]);
        let menu = LinuxMenu::from_menu(Some(&menu), 1).unwrap();
        let props = menu.properties(1, &[String::from("label")]).unwrap();

        assert_eq!(props.label.as_deref(), Some("Open"));
        assert!(props.enabled.is_none());
        assert!(!props.is_empty());
        assert!(LinuxMenuProperties::default().is_empty());
    }
}
