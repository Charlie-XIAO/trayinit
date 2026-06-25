use std::collections::HashMap;

use crate::{CheckItem, Menu, MenuItem, MenuItemId, MenuNode, Submenu, TrayResult};

use super::validate_menu;

pub(crate) type BackendMenuId = u32;
pub(crate) type BackendCommandId = u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MenuPlan {
    pub(crate) nodes: Vec<PlannedNode>,
    pub(crate) command_map: HashMap<BackendCommandId, MenuItemId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlannedNode {
    pub(crate) backend_id: BackendMenuId,
    pub(crate) explicit_id: Option<MenuItemId>,
    pub(crate) kind: PlannedNodeKind,
    pub(crate) children: Vec<PlannedNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlannedNodeKind {
    Item(PlannedItem),
    Check(PlannedCheckItem),
    Submenu(PlannedSubmenu),
    Separator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlannedItem {
    pub(crate) command_id: BackendCommandId,
    pub(crate) label: String,
    pub(crate) enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlannedCheckItem {
    pub(crate) command_id: BackendCommandId,
    pub(crate) label: String,
    pub(crate) checked: bool,
    pub(crate) enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlannedSubmenu {
    pub(crate) label: String,
    pub(crate) enabled: bool,
}

#[derive(Default)]
struct PlanBuilder {
    next_backend_id: BackendMenuId,
    next_command_id: BackendCommandId,
    command_map: HashMap<BackendCommandId, MenuItemId>,
}

pub(crate) fn plan_menu(menu: &Menu) -> TrayResult<MenuPlan> {
    validate_menu(menu)?;
    let mut builder = PlanBuilder::default();
    let nodes = builder.plan_nodes(menu.nodes());
    Ok(MenuPlan {
        nodes,
        command_map: builder.command_map,
    })
}

impl PlanBuilder {
    fn plan_nodes(&mut self, nodes: &[MenuNode]) -> Vec<PlannedNode> {
        nodes.iter().map(|node| self.plan_node(node)).collect()
    }

    fn plan_node(&mut self, node: &MenuNode) -> PlannedNode {
        let backend_id = self.alloc_backend_id();
        match node {
            MenuNode::Item(item) => self.plan_item(backend_id, item),
            MenuNode::Check(item) => self.plan_check(backend_id, item),
            MenuNode::Submenu(submenu) => self.plan_submenu(backend_id, submenu),
            MenuNode::Separator => PlannedNode {
                backend_id,
                explicit_id: None,
                kind: PlannedNodeKind::Separator,
                children: Vec::new(),
            },
        }
    }

    fn plan_item(&mut self, backend_id: BackendMenuId, item: &MenuItem) -> PlannedNode {
        let command_id = self.alloc_command_id();
        self.command_map.insert(command_id, item.id.clone());
        PlannedNode {
            backend_id,
            explicit_id: Some(item.id.clone()),
            kind: PlannedNodeKind::Item(PlannedItem {
                command_id,
                label: item.label.clone(),
                enabled: item.enabled,
            }),
            children: Vec::new(),
        }
    }

    fn plan_check(&mut self, backend_id: BackendMenuId, item: &CheckItem) -> PlannedNode {
        let command_id = self.alloc_command_id();
        self.command_map.insert(command_id, item.id.clone());
        PlannedNode {
            backend_id,
            explicit_id: Some(item.id.clone()),
            kind: PlannedNodeKind::Check(PlannedCheckItem {
                command_id,
                label: item.label.clone(),
                checked: item.checked,
                enabled: item.enabled,
            }),
            children: Vec::new(),
        }
    }

    fn plan_submenu(&mut self, backend_id: BackendMenuId, submenu: &Submenu) -> PlannedNode {
        let children = self.plan_nodes(&submenu.children);
        PlannedNode {
            backend_id,
            explicit_id: submenu.id.clone(),
            kind: PlannedNodeKind::Submenu(PlannedSubmenu {
                label: submenu.label.clone(),
                enabled: submenu.enabled,
            }),
            children,
        }
    }

    fn alloc_backend_id(&mut self) -> BackendMenuId {
        self.next_backend_id += 1;
        self.next_backend_id
    }

    fn alloc_command_id(&mut self) -> BackendCommandId {
        self.next_command_id += 1;
        self.next_command_id
    }
}
