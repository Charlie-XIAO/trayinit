use std::collections::HashMap;

use super::validate_menu;
use crate::{CheckItem, Menu, MenuItem, MenuItemId, MenuNode, Submenu, TrayResult};

pub type BackendMenuId = u32;
pub type BackendCommandId = u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuPlan {
    pub nodes: Vec<PlannedNode>,
    pub command_map: HashMap<BackendCommandId, MenuItemId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedNode {
    pub backend_id: BackendMenuId,
    pub explicit_id: Option<MenuItemId>,
    pub kind: PlannedNodeKind,
    pub children: Vec<PlannedNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedNodeKind {
    Item(PlannedItem),
    Check(PlannedCheckItem),
    Submenu(PlannedSubmenu),
    Separator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedItem {
    pub command_id: BackendCommandId,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedCheckItem {
    pub command_id: BackendCommandId,
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedSubmenu {
    pub label: String,
    pub enabled: bool,
}

struct PlanBuilder {
    next_backend_id: BackendMenuId,
    next_command_id: BackendCommandId,
    command_map: HashMap<BackendCommandId, MenuItemId>,
}

impl PlanBuilder {
    fn new(base_id: BackendMenuId) -> Self {
        Self {
            next_backend_id: base_id,
            next_command_id: 0,
            command_map: HashMap::new(),
        }
    }
}

pub fn plan_menu(menu: &Menu, base_id: BackendMenuId) -> TrayResult<MenuPlan> {
    validate_menu(menu)?;
    let mut builder = PlanBuilder::new(base_id);
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
