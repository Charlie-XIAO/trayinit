use core::fmt;

use crate::Icon;

/// Declarative tray menu tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuItem<Id> {
    Action(ActionItem<Id>),
    Check(CheckItem<Id>),
    RadioGroup(RadioGroup<Id>),
    Submenu(Submenu<Id>),
    Separator,
}

impl<Id> MenuItem<Id> {
    pub fn id(&self) -> Option<&Id> {
        match self {
            MenuItem::Action(item) => Some(&item.id),
            MenuItem::Check(item) => Some(&item.id),
            MenuItem::RadioGroup(_) => None,
            MenuItem::Submenu(_) => None,
            MenuItem::Separator => None,
        }
    }
}

impl<Id> From<ActionItem<Id>> for MenuItem<Id> {
    fn from(value: ActionItem<Id>) -> Self {
        Self::Action(value)
    }
}

impl<Id> From<CheckItem<Id>> for MenuItem<Id> {
    fn from(value: CheckItem<Id>) -> Self {
        Self::Check(value)
    }
}

impl<Id> From<RadioGroup<Id>> for MenuItem<Id> {
    fn from(value: RadioGroup<Id>) -> Self {
        Self::RadioGroup(value)
    }
}

impl<Id> From<Submenu<Id>> for MenuItem<Id> {
    fn from(value: Submenu<Id>) -> Self {
        Self::Submenu(value)
    }
}

/// A normal clickable menu item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionItem<Id> {
    pub id: Id,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub icon: Option<Icon>,
    pub accelerator: Option<Accelerator>,
}

impl<Id> ActionItem<Id> {
    pub fn new(id: Id, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            enabled: true,
            visible: true,
            icon: None,
            accelerator: None,
        }
    }
}

/// A checkable menu item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckItem<Id> {
    pub id: Id,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub checked: bool,
    pub icon: Option<Icon>,
    pub accelerator: Option<Accelerator>,
}

impl<Id> CheckItem<Id> {
    pub fn new(id: Id, label: impl Into<String>, checked: bool) -> Self {
        Self {
            id,
            label: label.into(),
            enabled: true,
            visible: true,
            checked,
            icon: None,
            accelerator: None,
        }
    }
}

/// A radio item group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadioGroup<Id> {
    pub selected: Option<Id>,
    pub options: Vec<RadioItem<Id>>,
    pub enabled: bool,
    pub visible: bool,
}

impl<Id> RadioGroup<Id> {
    pub fn new(options: Vec<RadioItem<Id>>) -> Self {
        Self {
            selected: None,
            options,
            enabled: true,
            visible: true,
        }
    }
}

/// An option within a radio group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadioItem<Id> {
    pub id: Id,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub icon: Option<Icon>,
    pub accelerator: Option<Accelerator>,
}

impl<Id> RadioItem<Id> {
    pub fn new(id: Id, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            enabled: true,
            visible: true,
            icon: None,
            accelerator: None,
        }
    }
}

/// A non-clickable menu branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submenu<Id> {
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub icon: Option<Icon>,
    pub children: Vec<MenuItem<Id>>,
}

impl<Id> Submenu<Id> {
    pub fn new(label: impl Into<String>, children: Vec<MenuItem<Id>>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            visible: true,
            icon: None,
            children,
        }
    }
}

/// A keyboard accelerator.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Accelerator {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl Accelerator {
    pub fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }
}

impl fmt::Display for Accelerator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.control {
            f.write_str("Ctrl+")?;
        }
        if self.modifiers.alt {
            f.write_str("Alt+")?;
        }
        if self.modifiers.shift {
            f.write_str("Shift+")?;
        }
        if self.modifiers.super_key {
            f.write_str("Super+")?;
        }
        write!(f, "{}", self.key)
    }
}

/// Standard modifier set used by menu accelerators.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

impl Modifiers {
    pub const fn new() -> Self {
        Self {
            control: false,
            alt: false,
            shift: false,
            super_key: false,
        }
    }

    pub const fn control(mut self) -> Self {
        self.control = true;
        self
    }

    pub const fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub const fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub const fn super_key(mut self) -> Self {
        self.super_key = true;
        self
    }
}

/// A portable key identifier for accelerators.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Character(char),
    Named(String),
}

impl Key {
    pub fn named(value: impl Into<String>) -> Self {
        Self::Named(value.into())
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Character(character) => write!(f, "{}", character.to_ascii_uppercase()),
            Key::Named(value) => f.write_str(value),
        }
    }
}
