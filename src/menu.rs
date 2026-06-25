use std::{fmt, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MenuItemId(Arc<str>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    nodes: Vec<MenuNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuNode {
    Item(MenuItem),
    Check(CheckItem),
    Submenu(Submenu),
    Separator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub id: MenuItemId,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckItem {
    pub id: MenuItemId,
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submenu {
    pub id: Option<MenuItemId>,
    pub label: String,
    pub enabled: bool,
    pub children: Vec<MenuNode>,
}

impl MenuItemId {
    pub fn new(id: impl Into<String>) -> Self {
        let id: String = id.into();
        Self(Arc::from(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn is_valid(&self) -> bool {
        !self.0.trim().is_empty()
    }
}

impl From<&str> for MenuItemId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for MenuItemId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for MenuItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Menu {
    pub fn new(nodes: impl IntoIterator<Item = MenuNode>) -> Self {
        Self {
            nodes: nodes.into_iter().collect(),
        }
    }

    pub fn empty() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn nodes(&self) -> &[MenuNode] {
        &self.nodes
    }

    pub fn push(&mut self, node: MenuNode) {
        self.nodes.push(node);
    }
}

impl MenuNode {
    pub fn item(id: impl Into<MenuItemId>, label: impl Into<String>) -> Self {
        Self::Item(MenuItem {
            id: id.into(),
            label: label.into(),
            enabled: true,
        })
    }

    pub fn check(id: impl Into<MenuItemId>, label: impl Into<String>, checked: bool) -> Self {
        Self::Check(CheckItem {
            id: id.into(),
            label: label.into(),
            checked,
            enabled: true,
        })
    }

    pub fn submenu(label: impl Into<String>, children: impl IntoIterator<Item = MenuNode>) -> Self {
        Self::Submenu(Submenu {
            id: None,
            label: label.into(),
            enabled: true,
            children: children.into_iter().collect(),
        })
    }

    pub fn submenu_with_id(
        id: impl Into<MenuItemId>,
        label: impl Into<String>,
        children: impl IntoIterator<Item = MenuNode>,
    ) -> Self {
        Self::Submenu(Submenu {
            id: Some(id.into()),
            label: label.into(),
            enabled: true,
            children: children.into_iter().collect(),
        })
    }

    pub fn separator() -> Self {
        Self::Separator
    }

    pub fn disabled(mut self) -> Self {
        match &mut self {
            Self::Item(item) => item.enabled = false,
            Self::Check(item) => item.enabled = false,
            Self::Submenu(item) => item.enabled = false,
            Self::Separator => {}
        }
        self
    }
}
