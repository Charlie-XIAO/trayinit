use crate::Icon;
use crate::menu::{Accelerator, MenuItem};
use crate::tray::{Tray, TrayCategory, TrayStatus};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedTrayView<Message> {
    pub icon: Option<Icon>,
    pub icon_name: Option<String>,
    pub overlay_icon: Option<Icon>,
    pub overlay_icon_name: Option<String>,
    pub attention_icon: Option<Icon>,
    pub attention_icon_name: Option<String>,
    pub attention_movie_name: Option<String>,
    pub title: Option<String>,
    pub tooltip: Option<String>,
    pub visible: bool,
    pub status: TrayStatus,
    pub category: TrayCategory,
    pub menu_on_primary_click: bool,
    pub menu: Vec<NormalizedMenuItem<Message>>,
}

impl<Message> NormalizedTrayView<Message> {
    pub fn from_tray<T: Tray<Message = Message>>(tray: &T) -> Self {
        Self {
            icon: tray.icon(),
            icon_name: tray.icon_name(),
            overlay_icon: tray.overlay_icon(),
            overlay_icon_name: tray.overlay_icon_name(),
            attention_icon: tray.attention_icon(),
            attention_icon_name: tray.attention_icon_name(),
            attention_movie_name: tray.attention_movie_name(),
            title: tray.title(),
            tooltip: tray.tooltip(),
            visible: tray.visible(),
            status: tray.status(),
            category: tray.category(),
            menu_on_primary_click: tray.menu_on_primary_click(),
            menu: normalize_menu_items(tray.menu()),
        }
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuDiff<Message> {
    None,
    Patch(Vec<MenuPatch<Message>>),
    Rebuild,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuPatch<Message> {
    Command {
        path: MenuPath,
        item: NormalizedCommandItem<Message>,
    },
    Submenu {
        path: MenuPath,
        item: NormalizedSubmenu<Message>,
    },
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MenuPath(Vec<usize>);

#[cfg(any(target_os = "windows", test))]
impl MenuPath {
    pub fn new(segments: Vec<usize>) -> Self {
        Self(segments)
    }

    pub fn as_slice(&self) -> &[usize] {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NormalizedMenuItem<Message> {
    Standard(NormalizedCommandItem<Message>),
    Check(NormalizedCommandItem<Message>),
    Radio(NormalizedCommandItem<Message>),
    Submenu(NormalizedSubmenu<Message>),
    Separator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedCommandItem<Message> {
    pub message: Option<Message>,
    pub label: String,
    pub enabled: bool,
    pub icon: Option<Icon>,
    pub icon_name: Option<String>,
    pub accelerator: Option<Accelerator>,
    pub state: CommandState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandState {
    Standard,
    Check { checked: bool },
    Radio { selected: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedSubmenu<Message> {
    pub label: String,
    pub enabled: bool,
    pub icon: Option<Icon>,
    pub icon_name: Option<String>,
    pub children: Vec<NormalizedMenuItem<Message>>,
}

fn normalize_menu_items<Message>(
    items: Vec<MenuItem<Message>>,
) -> Vec<NormalizedMenuItem<Message>> {
    let mut normalized = Vec::new();

    for item in items {
        match item {
            MenuItem::Standard(item) if item.visible => {
                normalized.push(NormalizedMenuItem::Standard(NormalizedCommandItem {
                    message: item.message,
                    label: item.label,
                    enabled: item.enabled,
                    icon: item.icon,
                    icon_name: item.icon_name,
                    accelerator: item.accelerator,
                    state: CommandState::Standard,
                }));
            },
            MenuItem::Check(item) if item.visible => {
                normalized.push(NormalizedMenuItem::Check(NormalizedCommandItem {
                    message: item.message,
                    label: item.label,
                    enabled: item.enabled,
                    icon: item.icon,
                    icon_name: item.icon_name,
                    accelerator: item.accelerator,
                    state: CommandState::Check {
                        checked: item.checked,
                    },
                }));
            },
            MenuItem::RadioGroup(group) if group.visible => {
                for (index, option) in group.options.into_iter().enumerate() {
                    if !option.visible {
                        continue;
                    }

                    normalized.push(NormalizedMenuItem::Radio(NormalizedCommandItem {
                        message: option.message,
                        label: option.label,
                        enabled: group.enabled && option.enabled,
                        icon: option.icon,
                        icon_name: option.icon_name,
                        accelerator: option.accelerator,
                        state: CommandState::Radio {
                            selected: group.selected == Some(index),
                        },
                    }));
                }
            },
            MenuItem::Submenu(submenu) if submenu.visible => {
                let children = normalize_menu_items(submenu.children);
                if children.is_empty() {
                    continue;
                }

                normalized.push(NormalizedMenuItem::Submenu(NormalizedSubmenu {
                    label: submenu.label,
                    enabled: submenu.enabled,
                    icon: submenu.icon,
                    icon_name: submenu.icon_name,
                    children,
                }));
            },
            MenuItem::Separator => normalized.push(NormalizedMenuItem::Separator),
            _ => {},
        }
    }

    compact_menu_items(normalized)
}

fn compact_menu_items<Message>(
    items: Vec<NormalizedMenuItem<Message>>,
) -> Vec<NormalizedMenuItem<Message>> {
    let mut compacted = Vec::with_capacity(items.len());
    let mut pending_separator = false;

    for item in items {
        match item {
            NormalizedMenuItem::Separator => {
                if !compacted.is_empty() {
                    pending_separator = true;
                }
            },
            item => {
                if pending_separator {
                    compacted.push(NormalizedMenuItem::Separator);
                    pending_separator = false;
                }
                compacted.push(item);
            },
        }
    }

    compacted
}

#[cfg(any(target_os = "windows", test))]
pub fn diff_menu_items<Message: Clone>(
    old: &[NormalizedMenuItem<Message>],
    new: &[NormalizedMenuItem<Message>],
) -> MenuDiff<Message> {
    if !has_same_shape(old, new) {
        return MenuDiff::Rebuild;
    }

    let mut patches = Vec::new();
    let mut path = Vec::new();
    collect_menu_patches(old, new, &mut path, &mut patches);

    if patches.is_empty() {
        MenuDiff::None
    } else {
        MenuDiff::Patch(patches)
    }
}

#[cfg(any(target_os = "windows", test))]
fn has_same_shape<Message>(
    old: &[NormalizedMenuItem<Message>],
    new: &[NormalizedMenuItem<Message>],
) -> bool {
    if old.len() != new.len() {
        return false;
    }

    old.iter().zip(new).all(|(old, new)| match (old, new) {
        (NormalizedMenuItem::Standard(_), NormalizedMenuItem::Standard(_))
        | (NormalizedMenuItem::Check(_), NormalizedMenuItem::Check(_))
        | (NormalizedMenuItem::Radio(_), NormalizedMenuItem::Radio(_))
        | (NormalizedMenuItem::Separator, NormalizedMenuItem::Separator) => true,
        (NormalizedMenuItem::Submenu(old), NormalizedMenuItem::Submenu(new)) => {
            has_same_shape(&old.children, &new.children)
        },
        _ => false,
    })
}

#[cfg(any(target_os = "windows", test))]
fn collect_menu_patches<Message: Clone>(
    old: &[NormalizedMenuItem<Message>],
    new: &[NormalizedMenuItem<Message>],
    path: &mut Vec<usize>,
    patches: &mut Vec<MenuPatch<Message>>,
) {
    for (index, (old, new)) in old.iter().zip(new).enumerate() {
        path.push(index);

        match (old, new) {
            (NormalizedMenuItem::Standard(old), NormalizedMenuItem::Standard(new))
            | (NormalizedMenuItem::Check(old), NormalizedMenuItem::Check(new))
            | (NormalizedMenuItem::Radio(old), NormalizedMenuItem::Radio(new)) => {
                if old.label != new.label
                    || old.enabled != new.enabled
                    || old.icon != new.icon
                    || old.accelerator != new.accelerator
                    || old.state != new.state
                {
                    patches.push(MenuPatch::Command {
                        path: MenuPath::new(path.clone()),
                        item: new.clone(),
                    });
                }
            },
            (NormalizedMenuItem::Submenu(old), NormalizedMenuItem::Submenu(new)) => {
                if old.label != new.label || old.enabled != new.enabled || old.icon != new.icon {
                    patches.push(MenuPatch::Submenu {
                        path: MenuPath::new(path.clone()),
                        item: new.clone(),
                    });
                }
                collect_menu_patches(&old.children, &new.children, path, patches);
            },
            (NormalizedMenuItem::Separator, NormalizedMenuItem::Separator) => {},
            _ => {},
        }

        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandState, MenuDiff, NormalizedMenuItem, NormalizedTrayView, diff_menu_items};
    use crate::menu::{CheckItem, MenuItem, RadioGroup, RadioItem, StandardItem, Submenu};
    use crate::{Icon, Tray, TrayCategory, TrayEvent, TrayStatus};

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    enum Message {
        A,
        B,
        C,
    }

    struct TestTray {
        icon: Option<Icon>,
        title: Option<String>,
        tooltip: Option<String>,
        visible: bool,
        status: TrayStatus,
        category: TrayCategory,
        menu_on_primary_click: bool,
        menu: Vec<MenuItem<Message>>,
    }

    impl Tray for TestTray {
        type Message = Message;

        fn id(&self) -> &str {
            "test"
        }

        fn icon(&self) -> Option<Icon> {
            self.icon.clone()
        }

        fn title(&self) -> Option<String> {
            self.title.clone()
        }

        fn tooltip(&self) -> Option<String> {
            self.tooltip.clone()
        }

        fn visible(&self) -> bool {
            self.visible
        }

        fn status(&self) -> TrayStatus {
            self.status
        }

        fn category(&self) -> TrayCategory {
            self.category
        }

        fn menu_on_primary_click(&self) -> bool {
            self.menu_on_primary_click
        }

        fn menu(&self) -> Vec<MenuItem<Self::Message>> {
            self.menu.clone()
        }

        fn event(&mut self, _event: TrayEvent<Self::Message>) {}
    }

    fn test_tray(menu: Vec<MenuItem<Message>>) -> TestTray {
        TestTray {
            icon: None,
            title: None,
            tooltip: None,
            visible: true,
            status: TrayStatus::Active,
            category: TrayCategory::ApplicationStatus,
            menu_on_primary_click: false,
            menu,
        }
    }

    #[test]
    fn radio_group_normalizes_into_visible_items() {
        let mut hidden = RadioItem::new("Hidden", Message::B);
        hidden.visible = false;

        let tray = test_tray(vec![MenuItem::RadioGroup(
            RadioGroup::new(vec![
                RadioItem::new("A", Message::A),
                hidden,
                RadioItem::new("C", Message::C),
            ])
            .with_selected(2),
        )]);

        let normalized = NormalizedTrayView::from_tray(&tray);
        assert_eq!(normalized.menu.len(), 2);
        assert!(matches!(
            &normalized.menu[0],
            NormalizedMenuItem::Radio(item)
                if item.state == CommandState::Radio { selected: false }
        ));
        assert!(matches!(
            &normalized.menu[1],
            NormalizedMenuItem::Radio(item)
                if item.state == CommandState::Radio { selected: true }
        ));
    }

    #[test]
    fn menu_property_change_produces_patch() {
        let old = NormalizedTrayView::from_tray(&test_tray(vec![
            CheckItem::new("Enabled", false, Message::A).into(),
        ]));
        let new = NormalizedTrayView::from_tray(&test_tray(vec![
            CheckItem::new("Enabled", true, Message::A).into(),
        ]));

        assert!(matches!(
            diff_menu_items(&old.menu, &new.menu),
            MenuDiff::Patch(_)
        ));
    }

    #[test]
    fn message_change_does_not_trigger_visual_patch() {
        let old = NormalizedTrayView::from_tray(&test_tray(vec![
            StandardItem::new("A", Message::A).into(),
        ]));
        let new = NormalizedTrayView::from_tray(&test_tray(vec![
            StandardItem::new("A", Message::B).into(),
        ]));

        assert!(matches!(
            diff_menu_items(&old.menu, &new.menu),
            MenuDiff::None
        ));
    }

    #[test]
    fn menu_shape_change_requests_rebuild() {
        let old = NormalizedTrayView::from_tray(&test_tray(vec![
            StandardItem::new("A", Message::A).into(),
        ]));
        let new = NormalizedTrayView::from_tray(&test_tray(vec![
            Submenu::new("Group", vec![StandardItem::new("A", Message::A).into()]).into(),
        ]));

        assert_eq!(diff_menu_items(&old.menu, &new.menu), MenuDiff::Rebuild);
    }

    #[test]
    fn normalization_prunes_empty_submenus() {
        let tray = test_tray(vec![
            Submenu::new("Empty", vec![]).into(),
            Submenu::new("Hidden only", {
                let mut hidden = StandardItem::new("Hidden", Message::A);
                hidden.visible = false;
                vec![hidden.into()]
            })
            .into(),
        ]);

        let normalized = NormalizedTrayView::from_tray(&tray);
        assert!(normalized.menu.is_empty());
    }

    #[test]
    fn normalization_trims_and_collapses_separators() {
        let tray = test_tray(vec![
            MenuItem::Separator,
            StandardItem::new("A", Message::A).into(),
            MenuItem::Separator,
            MenuItem::Separator,
            CheckItem::new("B", false, Message::B).into(),
            MenuItem::Separator,
            Submenu::new("Only separators", vec![MenuItem::Separator]).into(),
            MenuItem::Separator,
        ]);

        let normalized = NormalizedTrayView::from_tray(&tray);
        assert_eq!(normalized.menu.len(), 3);
        assert!(matches!(
            normalized.menu[0],
            NormalizedMenuItem::Standard(_)
        ));
        assert!(matches!(normalized.menu[1], NormalizedMenuItem::Separator));
        assert!(matches!(normalized.menu[2], NormalizedMenuItem::Check(_)));
    }
}
