use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use zbus::zvariant::{OwnedValue, Str, Type, Value};

use crate::menu::{Accelerator, Code, Modifiers};
use crate::model::{CommandState, NormalizedCommandItem, NormalizedMenuItem, NormalizedSubmenu};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuSnapshot {
    root_children: Vec<usize>,
    entries: Vec<MenuEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    kind: MenuEntryKind,
    label: String,
    enabled: bool,
    visible: bool,
    shortcut: Vec<Vec<String>>,
    toggle_type: ToggleType,
    toggle_state: ToggleState,
    children: Vec<usize>,
    path: Option<Vec<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuEntryKind {
    Standard,
    Separator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToggleType {
    Checkmark,
    Radio,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToggleState {
    Off,
    On,
    Indeterminate,
}

#[derive(Debug, Default, Type, Serialize, Deserialize, Value, OwnedValue)]
pub struct Layout {
    pub id: i32,
    pub properties: HashMap<String, OwnedValue>,
    pub children: Vec<OwnedValue>,
}

impl MenuSnapshot {
    pub fn from_normalized<Message>(items: &[NormalizedMenuItem<Message>]) -> Self {
        let mut entries = Vec::new();
        let root_children = flatten_items(items, &mut entries, &mut Vec::new());
        Self {
            root_children,
            entries,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.root_children.is_empty()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn message_path_for_id(&self, id_offset: i32, id: i32) -> Option<&[usize]> {
        let index = self.index_from_id(id_offset, id)?;
        self.entries.get(index)?.path.as_deref()
    }

    pub fn properties_for_id(
        &self,
        id_offset: i32,
        id: i32,
        property_names: &[String],
    ) -> Option<HashMap<String, OwnedValue>> {
        let index = self.index_from_id(id_offset, id)?;
        Some(self.entries.get(index)?.to_dbus_map(property_names))
    }

    pub fn layout_for_id(
        &self,
        id_offset: i32,
        parent_id: i32,
        recursion_depth: Option<usize>,
        property_names: &[String],
    ) -> Option<Layout> {
        if parent_id == 0 {
            return Some(self.build_layout(None, id_offset, recursion_depth, property_names));
        }

        let index = self.index_from_id(id_offset, parent_id)?;
        Some(self.build_layout(Some(index), id_offset, recursion_depth, property_names))
    }

    fn build_layout(
        &self,
        index: Option<usize>,
        id_offset: i32,
        recursion_depth: Option<usize>,
        property_names: &[String],
    ) -> Layout {
        let (id, children, mut properties) = match index {
            Some(index) => {
                let entry = &self.entries[index];
                (
                    id_offset + index as i32 + 1,
                    entry.children.as_slice(),
                    entry.to_dbus_map(property_names),
                )
            },
            None => (0, self.root_children.as_slice(), HashMap::new()),
        };

        let child_depth = recursion_depth.map(|depth| depth.saturating_sub(1));
        let built_children = if recursion_depth.is_some_and(|depth| depth == 0) {
            Vec::new()
        } else {
            children
                .iter()
                .map(|child| {
                    self.build_layout(Some(*child), id_offset, child_depth, property_names)
                        .try_into()
                        .expect("layout should convert into an owned D-Bus value")
                })
                .collect()
        };

        if index.is_some() && !children.is_empty() {
            properties.insert(
                "children-display".into(),
                OwnedValue::from(Str::from_static("submenu")),
            );
        }

        Layout {
            id,
            properties,
            children: built_children,
        }
    }

    fn index_from_id(&self, id_offset: i32, id: i32) -> Option<usize> {
        if id <= id_offset {
            return None;
        }

        let index = usize::try_from(id - id_offset - 1).ok()?;
        (index < self.entries.len()).then_some(index)
    }
}

impl MenuEntry {
    fn to_dbus_map(&self, property_names: &[String]) -> HashMap<String, OwnedValue> {
        let mut properties = HashMap::new();

        if property_names.is_empty() || property_names.iter().any(|name| name == "type") {
            if self.kind == MenuEntryKind::Separator {
                properties.insert(
                    "type".into(),
                    OwnedValue::from(Str::from_static("separator")),
                );
            }
        }

        if property_names.is_empty() || property_names.iter().any(|name| name == "label") {
            properties.insert(
                "label".into(),
                OwnedValue::from(Str::from(self.label.clone())),
            );
        }

        if property_names.is_empty() || property_names.iter().any(|name| name == "enabled") {
            properties.insert("enabled".into(), self.enabled.into());
        }

        if property_names.is_empty() || property_names.iter().any(|name| name == "visible") {
            properties.insert("visible".into(), self.visible.into());
        }

        if !self.shortcut.is_empty()
            && (property_names.is_empty() || property_names.iter().any(|name| name == "shortcut"))
        {
            properties.insert(
                "shortcut".into(),
                Value::from(self.shortcut.clone())
                    .try_into()
                    .expect("shortcut should convert into an owned D-Bus value"),
            );
        }

        if self.toggle_type != ToggleType::None
            && (property_names.is_empty()
                || property_names.iter().any(|name| name == "toggle-type"))
        {
            let toggle_type = match self.toggle_type {
                ToggleType::Checkmark => Str::from_static("checkmark"),
                ToggleType::Radio => Str::from_static("radio"),
                ToggleType::None => Str::from_static(""),
            };
            properties.insert("toggle-type".into(), OwnedValue::from(toggle_type));
        }

        if self.toggle_type != ToggleType::None
            && (property_names.is_empty()
                || property_names.iter().any(|name| name == "toggle-state"))
        {
            let toggle_state = match self.toggle_state {
                ToggleState::Off => 0_i32,
                ToggleState::On => 1_i32,
                ToggleState::Indeterminate => -1_i32,
            };
            properties.insert("toggle-state".into(), toggle_state.into());
        }

        properties
    }
}

fn flatten_items<Message>(
    items: &[NormalizedMenuItem<Message>],
    entries: &mut Vec<MenuEntry>,
    prefix: &mut Vec<usize>,
) -> Vec<usize> {
    let mut children = Vec::with_capacity(items.len());

    for (index, item) in items.iter().enumerate() {
        prefix.push(index);
        let entry_index = entries.len();
        match item {
            NormalizedMenuItem::Standard(item) => {
                entries.push(command_entry(item, prefix.clone(), ToggleType::None));
            },
            NormalizedMenuItem::Check(item) => {
                entries.push(command_entry(item, prefix.clone(), ToggleType::Checkmark));
            },
            NormalizedMenuItem::Radio(item) => {
                entries.push(command_entry(item, prefix.clone(), ToggleType::Radio));
            },
            NormalizedMenuItem::Submenu(submenu) => {
                entries.push(submenu_entry(submenu));
                let submenu_children = flatten_items(&submenu.children, entries, prefix);
                entries[entry_index].children = submenu_children;
            },
            NormalizedMenuItem::Separator => {
                entries.push(MenuEntry {
                    kind: MenuEntryKind::Separator,
                    label: String::new(),
                    enabled: false,
                    visible: true,
                    shortcut: Vec::new(),
                    toggle_type: ToggleType::None,
                    toggle_state: ToggleState::Indeterminate,
                    children: Vec::new(),
                    path: None,
                });
            },
        }
        children.push(entry_index);
        prefix.pop();
    }

    children
}

fn command_entry<Message>(
    item: &NormalizedCommandItem<Message>,
    path: Vec<usize>,
    toggle_type: ToggleType,
) -> MenuEntry {
    let toggle_state = match item.state {
        CommandState::Standard => ToggleState::Indeterminate,
        CommandState::Check { checked } => {
            if checked {
                ToggleState::On
            } else {
                ToggleState::Off
            }
        },
        CommandState::Radio { selected } => {
            if selected {
                ToggleState::On
            } else {
                ToggleState::Off
            }
        },
    };

    MenuEntry {
        kind: MenuEntryKind::Standard,
        label: item.label.clone(),
        enabled: item.enabled,
        visible: true,
        shortcut: item
            .accelerator
            .as_ref()
            .map(shortcut_metadata)
            .unwrap_or_default(),
        toggle_type,
        toggle_state,
        children: Vec::new(),
        path: Some(path),
    }
}

fn submenu_entry<Message>(submenu: &NormalizedSubmenu<Message>) -> MenuEntry {
    MenuEntry {
        kind: MenuEntryKind::Standard,
        label: submenu.label.clone(),
        enabled: submenu.enabled,
        visible: true,
        shortcut: Vec::new(),
        toggle_type: ToggleType::None,
        toggle_state: ToggleState::Indeterminate,
        children: Vec::new(),
        path: None,
    }
}

pub fn message_at_path<Message: Clone>(
    items: &[NormalizedMenuItem<Message>],
    path: &[usize],
) -> Option<Message> {
    let (index, rest) = path.split_first()?;
    match items.get(*index)? {
        NormalizedMenuItem::Standard(item)
        | NormalizedMenuItem::Check(item)
        | NormalizedMenuItem::Radio(item) => {
            if rest.is_empty() {
                Some(item.message.clone())
            } else {
                None
            }
        },
        NormalizedMenuItem::Submenu(submenu) => message_at_path(&submenu.children, rest),
        NormalizedMenuItem::Separator => None,
    }
}

fn shortcut_metadata(accelerator: &Accelerator) -> Vec<Vec<String>> {
    let mut shortcut = Vec::new();

    if accelerator.modifiers().contains(Modifiers::CONTROL) {
        shortcut.push("Control".to_string());
    }
    if accelerator.modifiers().contains(Modifiers::ALT) {
        shortcut.push("Alt".to_string());
    }
    if accelerator.modifiers().contains(Modifiers::SHIFT) {
        shortcut.push("Shift".to_string());
    }
    if accelerator.modifiers().contains(Modifiers::SUPER) {
        shortcut.push("Super".to_string());
    }

    shortcut.push(shortcut_key(accelerator.key()));
    vec![shortcut]
}

fn shortcut_key(code: Code) -> String {
    match code {
        Code::KeyA => "A",
        Code::KeyB => "B",
        Code::KeyC => "C",
        Code::KeyD => "D",
        Code::KeyE => "E",
        Code::KeyF => "F",
        Code::KeyG => "G",
        Code::KeyH => "H",
        Code::KeyI => "I",
        Code::KeyJ => "J",
        Code::KeyK => "K",
        Code::KeyL => "L",
        Code::KeyM => "M",
        Code::KeyN => "N",
        Code::KeyO => "O",
        Code::KeyP => "P",
        Code::KeyQ => "Q",
        Code::KeyR => "R",
        Code::KeyS => "S",
        Code::KeyT => "T",
        Code::KeyU => "U",
        Code::KeyV => "V",
        Code::KeyW => "W",
        Code::KeyX => "X",
        Code::KeyY => "Y",
        Code::KeyZ => "Z",
        Code::Digit0 => "0",
        Code::Digit1 => "1",
        Code::Digit2 => "2",
        Code::Digit3 => "3",
        Code::Digit4 => "4",
        Code::Digit5 => "5",
        Code::Digit6 => "6",
        Code::Digit7 => "7",
        Code::Digit8 => "8",
        Code::Digit9 => "9",
        Code::F1 => "F1",
        Code::F2 => "F2",
        Code::F3 => "F3",
        Code::F4 => "F4",
        Code::F5 => "F5",
        Code::F6 => "F6",
        Code::F7 => "F7",
        Code::F8 => "F8",
        Code::F9 => "F9",
        Code::F10 => "F10",
        Code::F11 => "F11",
        Code::F12 => "F12",
        Code::Enter => "Return",
        Code::Space => "space",
        Code::Tab => "Tab",
        Code::Escape => "Escape",
        Code::Delete => "Delete",
        Code::Insert => "Insert",
        Code::Home => "Home",
        Code::End => "End",
        Code::PageUp => "Page_Up",
        Code::PageDown => "Page_Down",
        Code::ArrowLeft => "Left",
        Code::ArrowRight => "Right",
        Code::ArrowUp => "Up",
        Code::ArrowDown => "Down",
        _ => return format!("{code:?}"),
    }
    .to_string()
}
