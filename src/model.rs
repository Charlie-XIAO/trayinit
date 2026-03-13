use crate::{
    Icon,
    menu::{Accelerator, MenuItem},
    tray::{TrayStatus, TrayView},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NormalizedTrayView<Id> {
    pub icon: Option<Icon>,
    pub title: Option<String>,
    pub tooltip: Option<String>,
    pub visible: bool,
    pub status: TrayStatus,
    pub menu_on_primary_click: bool,
    pub menu: Vec<NormalizedMenuItem<Id>>,
}

impl<Id: Clone + Eq> NormalizedTrayView<Id> {
    pub(crate) fn from_view(view: TrayView<Id>) -> Self {
        Self {
            icon: view.icon,
            title: view.title,
            tooltip: view.tooltip,
            visible: view.visible,
            status: view.status,
            menu_on_primary_click: view.menu_on_primary_click,
            menu: normalize_menu_items(view.menu),
        }
    }

    pub(crate) fn diff(&self, new: &Self) -> TrayViewDiff<Id> {
        TrayViewDiff {
            icon_changed: self.icon != new.icon,
            title_changed: self.title != new.title,
            tooltip_changed: self.tooltip != new.tooltip,
            visible_changed: self.visible != new.visible,
            status_changed: self.status != new.status,
            menu_on_primary_click_changed: self.menu_on_primary_click != new.menu_on_primary_click,
            menu: diff_menu_items(&self.menu, &new.menu),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrayViewDiff<Id> {
    pub icon_changed: bool,
    pub title_changed: bool,
    pub tooltip_changed: bool,
    pub visible_changed: bool,
    pub status_changed: bool,
    pub menu_on_primary_click_changed: bool,
    pub menu: MenuDiff<Id>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MenuDiff<Id> {
    None,
    Patch(Vec<MenuPatch<Id>>),
    Rebuild,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MenuPatch<Id> {
    Command {
        path: MenuPath,
        item: NormalizedCommandItem<Id>,
    },
    Submenu {
        path: MenuPath,
        item: NormalizedSubmenu<Id>,
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
pub(crate) enum NormalizedMenuItem<Id> {
    Standard(NormalizedCommandItem<Id>),
    Check(NormalizedCommandItem<Id>),
    Radio(NormalizedCommandItem<Id>),
    Submenu(NormalizedSubmenu<Id>),
    Separator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NormalizedCommandItem<Id> {
    pub id: Id,
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
pub(crate) struct NormalizedSubmenu<Id> {
    pub label: String,
    pub enabled: bool,
    pub icon: Option<Icon>,
    pub children: Vec<NormalizedMenuItem<Id>>,
}

fn normalize_menu_items<Id: Clone + Eq>(items: Vec<MenuItem<Id>>) -> Vec<NormalizedMenuItem<Id>> {
    let mut normalized = Vec::new();

    for item in items {
        match item {
            MenuItem::Standard(item) if item.visible => {
                normalized.push(NormalizedMenuItem::Standard(NormalizedCommandItem {
                    id: item.id,
                    label: item.label,
                    enabled: item.enabled,
                    icon: item.icon,
                    accelerator: item.accelerator,
                    state: CommandState::Standard,
                }));
            },
            MenuItem::Check(item) if item.visible => {
                normalized.push(NormalizedMenuItem::Check(NormalizedCommandItem {
                    id: item.id,
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
                        id: option.id.clone(),
                        label: option.label,
                        enabled: group.enabled && option.enabled,
                        icon: option.icon,
                        accelerator: option.accelerator,
                        state: CommandState::Radio {
                            selected: group.selected.as_ref() == Some(&option.id),
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

fn diff_menu_items<Id: Clone + Eq>(
    old: &[NormalizedMenuItem<Id>],
    new: &[NormalizedMenuItem<Id>],
) -> MenuDiff<Id> {
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

fn has_same_shape<Id>(old: &[NormalizedMenuItem<Id>], new: &[NormalizedMenuItem<Id>]) -> bool {
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

fn collect_menu_patches<Id: Clone + Eq>(
    old: &[NormalizedMenuItem<Id>],
    new: &[NormalizedMenuItem<Id>],
    path: &mut Vec<usize>,
    patches: &mut Vec<MenuPatch<Id>>,
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
    use super::{CommandState, MenuDiff, NormalizedMenuItem, NormalizedTrayView};
    use crate::{
        menu::{CheckItem, MenuItem, RadioGroup, RadioItem, StandardItem, Submenu},
        tray::TrayView,
    };

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    enum Id {
        A,
        B,
        C,
    }

    #[test]
    fn radio_group_normalizes_into_visible_items() {
        let mut hidden = RadioItem::new(Id::B, "Hidden");
        hidden.visible = false;

        let view = TrayView {
            menu: vec![MenuItem::RadioGroup(RadioGroup {
                selected: Some(Id::C),
                options: vec![
                    RadioItem::new(Id::A, "A"),
                    hidden,
                    RadioItem::new(Id::C, "C"),
                ],
                enabled: true,
                visible: true,
            })],
            ..Default::default()
        };

        let normalized = NormalizedTrayView::from_view(view);
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
        let old = NormalizedTrayView::from_view(TrayView {
            menu: vec![CheckItem::new(Id::A, "Enabled", false).into()],
            ..Default::default()
        });
        let new = NormalizedTrayView::from_view(TrayView {
            menu: vec![CheckItem::new(Id::A, "Enabled", true).into()],
            ..Default::default()
        });

        assert!(matches!(old.diff(&new).menu, MenuDiff::Patch(_)));
    }

    #[test]
    fn menu_shape_change_requests_rebuild() {
        let old = NormalizedTrayView::from_view(TrayView {
            menu: vec![StandardItem::new(Id::A, "A").into()],
            ..Default::default()
        });
        let new = NormalizedTrayView::from_view(TrayView {
            menu: vec![Submenu::new("Group", vec![StandardItem::new(Id::A, "A").into()]).into()],
            ..Default::default()
        });

        assert_eq!(old.diff(&new).menu, MenuDiff::Rebuild);
    }
}
