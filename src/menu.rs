use core::fmt;

pub use keyboard_types::{Code, Modifiers};

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
    pub icon_name: Option<String>,
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
            icon_name: None,
            accelerator: None,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn with_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = Some(icon_name.into());
        self
    }

    pub fn with_accelerator(mut self, accelerator: Accelerator) -> Self {
        self.accelerator = Some(accelerator);
        self
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
    pub icon_name: Option<String>,
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
            icon_name: None,
            accelerator: None,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn with_checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Sets a custom icon for the item.
    ///
    /// On Windows tray popup menus, custom item bitmaps occupy the same left
    /// gutter as the native checkmark/radio indicator. Adding an icon to a
    /// check item may therefore hide the native checked marker.
    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn with_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = Some(icon_name.into());
        self
    }

    pub fn with_accelerator(mut self, accelerator: Accelerator) -> Self {
        self.accelerator = Some(accelerator);
        self
    }
}

/// A radio item group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadioGroup<Message> {
    /// Selected option index within `options`.
    pub selected: Option<usize>,
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

    pub fn with_selected(mut self, index: usize) -> Self {
        assert!(
            index < self.options.len(),
            "Radio selection index {index} is out of range for {} options",
            self.options.len()
        );
        self.selected = Some(index);
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
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
    pub icon_name: Option<String>,
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
            icon_name: None,
            accelerator: None,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Sets a custom icon for the item.
    ///
    /// On Windows tray popup menus, custom item bitmaps occupy the same left
    /// gutter as the native checkmark/radio indicator. Adding an icon to a
    /// radio item may therefore hide the native selected marker.
    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn with_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = Some(icon_name.into());
        self
    }

    pub fn with_accelerator(mut self, accelerator: Accelerator) -> Self {
        self.accelerator = Some(accelerator);
        self
    }
}

/// A non-clickable menu branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submenu<Message> {
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub icon: Option<Icon>,
    pub icon_name: Option<String>,
    pub children: Vec<MenuItem<Message>>,
}

impl<Message> Submenu<Message> {
    pub fn new(label: impl Into<String>, children: Vec<MenuItem<Message>>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            visible: true,
            icon: None,
            icon_name: None,
            children,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn with_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = Some(icon_name.into());
        self
    }
}

/// A menu keyboard accelerator.
///
/// Platform notes:
/// - macOS: maps naturally to native menu key equivalents.
/// - Linux: exported as menu shortcut metadata; actual host behavior is
///   desktop-dependent.
/// - Windows tray popups: the shortcut text is displayed in the menu, but
///   activation requires a real registered host window to supply keyboard
///   messages. Unsupported modifiers such as `SUPER` are rejected on Windows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Accelerator {
    modifiers: Modifiers,
    key: Code,
}

impl Accelerator {
    pub fn new(modifiers: Option<Modifiers>, key: Code) -> Self {
        let mut modifiers = modifiers.unwrap_or_else(Modifiers::empty);
        if modifiers.contains(Modifiers::META) {
            modifiers.remove(Modifiers::META);
            modifiers.insert(Modifiers::SUPER);
        }

        Self { modifiers, key }
    }

    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    pub fn key(&self) -> Code {
        self.key
    }

    pub fn matches(&self, modifiers: Modifiers, key: Code) -> bool {
        let base_mods = Modifiers::SHIFT | Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER;
        self.modifiers == modifiers & base_mods && self.key == key
    }
}

impl fmt::Display for Accelerator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let modifiers = self.modifiers;
        if modifiers.contains(Modifiers::CONTROL) {
            f.write_str("Ctrl+")?;
        }
        if modifiers.contains(Modifiers::ALT) {
            f.write_str("Alt+")?;
        }
        if modifiers.contains(Modifiers::SHIFT) {
            f.write_str("Shift+")?;
        }
        if modifiers.contains(Modifiers::SUPER) {
            f.write_str("Super+")?;
        }
        write_display_code(f, self.key)
    }
}

#[cfg(target_os = "macos")]
pub const CMD_OR_CTRL: Modifiers = Modifiers::SUPER;
#[cfg(not(target_os = "macos"))]
pub const CMD_OR_CTRL: Modifiers = Modifiers::CONTROL;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AcceleratorError {
    #[error("unsupported accelerator key on this platform: {0:?}")]
    UnsupportedKey(Code),
    #[error("unsupported accelerator modifiers on this platform: {0:?}")]
    UnsupportedModifiers(Modifiers),
}

fn write_display_code(f: &mut fmt::Formatter<'_>, code: Code) -> fmt::Result {
    match code {
        Code::KeyA => f.write_str("A"),
        Code::KeyB => f.write_str("B"),
        Code::KeyC => f.write_str("C"),
        Code::KeyD => f.write_str("D"),
        Code::KeyE => f.write_str("E"),
        Code::KeyF => f.write_str("F"),
        Code::KeyG => f.write_str("G"),
        Code::KeyH => f.write_str("H"),
        Code::KeyI => f.write_str("I"),
        Code::KeyJ => f.write_str("J"),
        Code::KeyK => f.write_str("K"),
        Code::KeyL => f.write_str("L"),
        Code::KeyM => f.write_str("M"),
        Code::KeyN => f.write_str("N"),
        Code::KeyO => f.write_str("O"),
        Code::KeyP => f.write_str("P"),
        Code::KeyQ => f.write_str("Q"),
        Code::KeyR => f.write_str("R"),
        Code::KeyS => f.write_str("S"),
        Code::KeyT => f.write_str("T"),
        Code::KeyU => f.write_str("U"),
        Code::KeyV => f.write_str("V"),
        Code::KeyW => f.write_str("W"),
        Code::KeyX => f.write_str("X"),
        Code::KeyY => f.write_str("Y"),
        Code::KeyZ => f.write_str("Z"),
        Code::Digit0 => f.write_str("0"),
        Code::Digit1 => f.write_str("1"),
        Code::Digit2 => f.write_str("2"),
        Code::Digit3 => f.write_str("3"),
        Code::Digit4 => f.write_str("4"),
        Code::Digit5 => f.write_str("5"),
        Code::Digit6 => f.write_str("6"),
        Code::Digit7 => f.write_str("7"),
        Code::Digit8 => f.write_str("8"),
        Code::Digit9 => f.write_str("9"),
        Code::Comma => f.write_str(","),
        Code::Minus => f.write_str("-"),
        Code::Period => f.write_str("."),
        Code::Space => f.write_str("Space"),
        Code::Equal => f.write_str("="),
        Code::Semicolon => f.write_str(";"),
        Code::Slash => f.write_str("/"),
        Code::Backslash => f.write_str("\\"),
        Code::Quote => f.write_str("'"),
        Code::Backquote => f.write_str("`"),
        Code::BracketLeft => f.write_str("["),
        Code::BracketRight => f.write_str("]"),
        Code::Tab => f.write_str("Tab"),
        Code::Escape => f.write_str("Esc"),
        Code::Delete => f.write_str("Del"),
        Code::Insert => f.write_str("Ins"),
        Code::PageUp => f.write_str("PgUp"),
        Code::PageDown => f.write_str("PgDn"),
        Code::ArrowLeft => f.write_str("Left"),
        Code::ArrowRight => f.write_str("Right"),
        Code::ArrowUp => f.write_str("Up"),
        Code::ArrowDown => f.write_str("Down"),
        Code::Enter => f.write_str("Enter"),
        Code::Home => f.write_str("Home"),
        Code::End => f.write_str("End"),
        Code::F1 => f.write_str("F1"),
        Code::F2 => f.write_str("F2"),
        Code::F3 => f.write_str("F3"),
        Code::F4 => f.write_str("F4"),
        Code::F5 => f.write_str("F5"),
        Code::F6 => f.write_str("F6"),
        Code::F7 => f.write_str("F7"),
        Code::F8 => f.write_str("F8"),
        Code::F9 => f.write_str("F9"),
        Code::F10 => f.write_str("F10"),
        Code::F11 => f.write_str("F11"),
        Code::F12 => f.write_str("F12"),
        _ => write!(f, "{code:?}"),
    }
}
