use core::fmt;

use crate::Icon;

/// Declarative tray menu tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuItem<Message> {
    Standard(StandardItem<Message>),
    Check(CheckItem<Message>),
    RadioGroup(RadioGroup<Message>),
    Submenu(Submenu<Message>),
    Separator,
}

impl<Message> MenuItem<Message> {
    pub fn message(&self) -> Option<&Message> {
        match self {
            MenuItem::Standard(item) => Some(&item.message),
            MenuItem::Check(item) => Some(&item.message),
            MenuItem::RadioGroup(_) => None,
            MenuItem::Submenu(_) => None,
            MenuItem::Separator => None,
        }
    }
}

impl<Message> From<StandardItem<Message>> for MenuItem<Message> {
    fn from(value: StandardItem<Message>) -> Self {
        Self::Standard(value)
    }
}

impl<Message> From<CheckItem<Message>> for MenuItem<Message> {
    fn from(value: CheckItem<Message>) -> Self {
        Self::Check(value)
    }
}

impl<Message> From<RadioGroup<Message>> for MenuItem<Message> {
    fn from(value: RadioGroup<Message>) -> Self {
        Self::RadioGroup(value)
    }
}

impl<Message> From<Submenu<Message>> for MenuItem<Message> {
    fn from(value: Submenu<Message>) -> Self {
        Self::Submenu(value)
    }
}

/// A normal clickable menu item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandardItem<Message> {
    pub message: Message,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub icon: Option<Icon>,
    pub accelerator: Option<Accelerator>,
}

impl<Message> StandardItem<Message> {
    pub fn new(label: impl Into<String>, message: Message) -> Self {
        Self {
            message,
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
pub struct CheckItem<Message> {
    pub message: Message,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub checked: bool,
    pub icon: Option<Icon>,
    pub accelerator: Option<Accelerator>,
}

impl<Message> CheckItem<Message> {
    pub fn new(label: impl Into<String>, checked: bool, message: Message) -> Self {
        Self {
            message,
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
pub struct RadioGroup<Message> {
    pub selected: Option<Message>,
    pub options: Vec<RadioItem<Message>>,
    pub enabled: bool,
    pub visible: bool,
}

impl<Message> RadioGroup<Message> {
    pub fn new(options: Vec<RadioItem<Message>>) -> Self {
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
pub struct RadioItem<Message> {
    pub message: Message,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub icon: Option<Icon>,
    pub accelerator: Option<Accelerator>,
}

impl<Message> RadioItem<Message> {
    pub fn new(label: impl Into<String>, message: Message) -> Self {
        Self {
            message,
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
pub struct Submenu<Message> {
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub icon: Option<Icon>,
    pub children: Vec<MenuItem<Message>>,
}

impl<Message> Submenu<Message> {
    pub fn new(label: impl Into<String>, children: Vec<MenuItem<Message>>) -> Self {
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
