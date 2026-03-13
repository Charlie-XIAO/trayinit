use crate::{
    Icon,
    menu::{Accelerator, MenuItem},
    tray::{Tray, TrayStatus},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NormalizedTrayView<Message> {
    pub icon: Option<Icon>,
    pub title: Option<String>,
    pub tooltip: Option<String>,
    pub visible: bool,
    pub status: TrayStatus,
    pub menu_on_primary_click: bool,
    pub menu: Vec<NormalizedMenuItem<Message>>,
}

impl<Message: Clone + Eq> NormalizedTrayView<Message> {
    pub(crate) fn from_tray<T: Tray<Message = Message>>(tray: &T) -> Self {
        Self {
            icon: tray.icon(),
            title: tray.title(),
            tooltip: tray.tooltip(),
            visible: tray.visible(),
            status: tray.status(),
            menu_on_primary_click: tray.menu_on_primary_click(),
            menu: normalize_menu_items(tray.menu()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MenuDiff<Message> {
    None,
    Patch(Vec<MenuPatch<Message>>),
    Rebuild,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MenuPatch<Message> {
    Command {
        path: MenuPath,
        item: NormalizedCommandItem<Message>,
    },
    Submenu {
        path: MenuPath,
        item: NormalizedSubmenu<Message>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MenuPath(Vec<usize>);

impl MenuPath {
    pub(crate) fn new(segments: Vec<usize>) -> Self {
        Self(segments)
    }

    pub(crate) fn as_slice(&self) -> &[usize] {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NormalizedMenuItem<Message> {
    Standard(NormalizedCommandItem<Message>),
    Check(NormalizedCommandItem<Message>),
    Radio(NormalizedCommandItem<Message>),
    Submenu(NormalizedSubmenu<Message>),
    Separator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NormalizedCommandItem<Message> {
    pub message: Message,
    pub label: String,
    pub enabled: bool,
    pub icon: Option<Icon>,
    pub accelerator: Option<Accelerator>,
    pub state: CommandState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CommandState {
    Standard,
    Check { checked: bool },
    Radio { selected: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NormalizedSubmenu<Message> {
    pub label: String,
    pub enabled: bool,
    pub icon: Option<Icon>,
    pub children: Vec<NormalizedMenuItem<Message>>,
}

fn normalize_menu_items<Message: Clone + Eq>(
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
                    accelerator: item.accelerator,
                    state: CommandState::Check {
                        checked: item.checked,
                    },
                }));
            },
            MenuItem::RadioGroup(group) if group.visible => {
                for option in group.options {
                    if !option.visible {
                        continue;
                    }

                    normalized.push(NormalizedMenuItem::Radio(NormalizedCommandItem {
                        message: option.message.clone(),
                        label: option.label,
                        enabled: group.enabled && option.enabled,
                        icon: option.icon,
                        accelerator: option.accelerator,
                        state: CommandState::Radio {
                            selected: group.selected.as_ref() == Some(&option.message),
                        },
                    }));
                }
            },
            MenuItem::Submenu(submenu) if submenu.visible => {
                normalized.push(NormalizedMenuItem::Submenu(NormalizedSubmenu {
                    label: submenu.label,
                    enabled: submenu.enabled,
                    icon: submenu.icon,
                    children: normalize_menu_items(submenu.children),
                }));
            },
            MenuItem::Separator => normalized.push(NormalizedMenuItem::Separator),
            _ => {},
        }
    }

    normalized
}

pub(crate) fn diff_menu_items<Message: Clone + Eq>(
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

fn collect_menu_patches<Message: Clone + Eq>(
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
                if old != new {
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
    use crate::{
        Icon, Tray, TrayEvent, TrayStatus,
        menu::{CheckItem, MenuItem, RadioGroup, RadioItem, StandardItem, Submenu},
    };

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
            menu_on_primary_click: false,
            menu,
        }
    }

    #[test]
    fn radio_group_normalizes_into_visible_items() {
        let mut hidden = RadioItem::new("Hidden", Message::B);
        hidden.visible = false;

        let tray = test_tray(vec![MenuItem::RadioGroup(RadioGroup {
            selected: Some(Message::C),
            options: vec![
                RadioItem::new("A", Message::A),
                hidden,
                RadioItem::new("C", Message::C),
            ],
            enabled: true,
            visible: true,
        })]);

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
    fn menu_shape_change_requests_rebuild() {
        let old = NormalizedTrayView::from_tray(&test_tray(vec![
            StandardItem::new("A", Message::A).into(),
        ]));
        let new = NormalizedTrayView::from_tray(&test_tray(vec![
            Submenu::new("Group", vec![StandardItem::new("A", Message::A).into()]).into(),
        ]));

        assert_eq!(diff_menu_items(&old.menu, &new.menu), MenuDiff::Rebuild);
    }
}
