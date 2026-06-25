use crate::{Icon, Menu};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayState {
    pub icon: Option<Icon>,
    pub tooltip: Option<String>,
    pub title: Option<String>,
    pub menu: Option<Menu>,
    pub visible: bool,
    pub activation_mode: ActivationMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationMode {
    PlatformDefault,
    MenuOnPrimaryClick,
    MenuOnSecondaryClick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalRect {
    pub position: PhysicalPosition,
    pub width: u32,
    pub height: u32,
}

impl TrayState {
    pub fn new() -> Self {
        Self {
            icon: None,
            tooltip: None,
            title: None,
            menu: None,
            visible: true,
            activation_mode: ActivationMode::PlatformDefault,
        }
    }

    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_menu(mut self, menu: Menu) -> Self {
        self.menu = Some(menu);
        self
    }

    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn with_activation_mode(mut self, activation_mode: ActivationMode) -> Self {
        self.activation_mode = activation_mode;
        self
    }
}

impl Default for TrayState {
    fn default() -> Self {
        Self::new()
    }
}
