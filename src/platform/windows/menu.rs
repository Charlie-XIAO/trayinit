use std::collections::HashMap;
use std::ptr;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, TRUE, WPARAM};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, HMENU, MENUITEMINFOW, MF_CHECKED, MF_DISABLED,
    MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MFS_CHECKED, MFS_DISABLED,
    MFS_ENABLED, MFS_UNCHECKED, MIIM_BITMAP, MIIM_STATE, MIIM_STRING, SetForegroundWindow,
    SetMenuItemInfoW, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, TrackPopupMenu, WM_NCACTIVATE,
    WM_NCPAINT,
};

use super::dark_menu_bar;
use super::icon::OwnedBitmap;
use super::util::encode_wide;
use crate::Icon;
use crate::menu::Accelerator;
use crate::model::{
    CommandState, MenuPatch, NormalizedCommandItem, NormalizedMenuItem, NormalizedSubmenu,
};

const MENU_SUBCLASS_ID: usize = 200;

#[derive(Debug)]
pub struct RenderedMenu<Message> {
    root: HMENU,
    items: Vec<NativeMenuItem>,
    command_map: HashMap<u32, Message>,
}

impl<Message: Clone + Eq> RenderedMenu<Message> {
    pub fn from_model(items: &[NormalizedMenuItem<Message>]) -> Option<Self> {
        let root = unsafe { CreatePopupMenu() };
        if root.is_null() {
            return None;
        }

        let mut builder = MenuBuilder {
            next_command: 1,
            command_map: HashMap::new(),
        };
        let items = builder.append_items(root, items);

        if items.is_empty() {
            unsafe {
                DestroyMenu(root);
            }
            None
        } else {
            Some(Self {
                root,
                items,
                command_map: builder.command_map,
            })
        }
    }

    pub fn handle(&self) -> HMENU {
        self.root
    }

    pub fn resolve(&self, command: u32) -> Option<Message> {
        self.command_map.get(&command).cloned()
    }

    pub fn apply_patches(&mut self, patches: &[MenuPatch<Message>]) -> bool {
        for patch in patches {
            let ok = match patch {
                MenuPatch::Command { path, item } => {
                    self.apply_command_patch(path.as_slice(), item)
                },
                MenuPatch::Submenu { path, item } => {
                    self.apply_submenu_patch(path.as_slice(), item)
                },
            };

            if !ok {
                return false;
            }
        }

        true
    }

    fn apply_command_patch(
        &mut self,
        path: &[usize],
        item: &NormalizedCommandItem<Message>,
    ) -> bool {
        let Some((parent, position, node)) = Self::locate_mut(self.root, &mut self.items, path)
        else {
            return false;
        };

        let command = match node.kind {
            NativeMenuItemKind::Command { command } => command,
            _ => return false,
        };

        let text = encode_wide(label_with_accelerator(
            &item.label,
            item.accelerator.as_ref(),
        ));
        let bitmap = bitmap_from_icon(item.icon.as_ref());
        let mut info: MENUITEMINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MENUITEMINFOW>() as _;
        info.fMask = MIIM_STRING | MIIM_STATE | MIIM_BITMAP;
        info.fState = command_state(item.state, item.enabled);
        info.dwTypeData = text.as_ptr() as *mut _;
        info.cch = text.len().saturating_sub(1) as _;
        info.hbmpItem = bitmap_handle(bitmap.as_ref());

        let ok = unsafe { SetMenuItemInfoW(parent, position as u32, TRUE, &info) != 0 };
        if !ok {
            return false;
        }

        node.bitmap = bitmap;
        self.command_map.insert(command, item.message.clone());
        true
    }

    fn apply_submenu_patch(&mut self, path: &[usize], item: &NormalizedSubmenu<Message>) -> bool {
        let Some((parent, position, node)) = Self::locate_mut(self.root, &mut self.items, path)
        else {
            return false;
        };

        let NativeMenuItemKind::Submenu { .. } = node.kind else {
            return false;
        };

        let text = encode_wide(item.label.as_str());
        let bitmap = bitmap_from_icon(item.icon.as_ref());
        let mut info: MENUITEMINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MENUITEMINFOW>() as _;
        info.fMask = MIIM_STRING | MIIM_STATE | MIIM_BITMAP;
        info.fState = if item.enabled {
            MFS_ENABLED
        } else {
            MFS_DISABLED
        };
        info.dwTypeData = text.as_ptr() as *mut _;
        info.cch = text.len().saturating_sub(1) as _;
        info.hbmpItem = bitmap_handle(bitmap.as_ref());

        let ok = unsafe { SetMenuItemInfoW(parent, position as u32, TRUE, &info) != 0 };
        if !ok {
            return false;
        }

        node.bitmap = bitmap;
        true
    }

    fn locate_mut<'a>(
        parent: HMENU,
        items: &'a mut [NativeMenuItem],
        path: &[usize],
    ) -> Option<(HMENU, usize, &'a mut NativeMenuItem)> {
        let (index, rest) = path.split_first()?;
        let node = items.get_mut(*index)?;

        if rest.is_empty() {
            return Some((parent, *index, node));
        }

        match &mut node.kind {
            NativeMenuItemKind::Submenu { menu, children } => {
                Self::locate_mut(*menu, children, rest)
            },
            _ => None,
        }
    }
}

impl<Message> Drop for RenderedMenu<Message> {
    fn drop(&mut self) {
        unsafe {
            DestroyMenu(self.root);
        }
    }
}

#[derive(Debug)]
struct NativeMenuItem {
    bitmap: Option<OwnedBitmap>,
    kind: NativeMenuItemKind,
}

#[derive(Debug)]
enum NativeMenuItemKind {
    Separator,
    Command {
        command: u32,
    },
    Submenu {
        #[allow(dead_code)]
        menu: HMENU,
        children: Vec<NativeMenuItem>,
    },
}

struct MenuBuilder<Message> {
    next_command: u32,
    command_map: HashMap<u32, Message>,
}

impl<Message: Clone + Eq> MenuBuilder<Message> {
    fn append_items(
        &mut self,
        parent: HMENU,
        items: &[NormalizedMenuItem<Message>],
    ) -> Vec<NativeMenuItem> {
        let mut rendered = Vec::with_capacity(items.len());

        for item in items {
            if let Some(rendered_item) = self.append_item(parent, rendered.len() as u32, item) {
                rendered.push(rendered_item);
            }
        }

        rendered
    }

    fn append_item(
        &mut self,
        parent: HMENU,
        position: u32,
        item: &NormalizedMenuItem<Message>,
    ) -> Option<NativeMenuItem> {
        match item {
            NormalizedMenuItem::Separator => {
                unsafe {
                    AppendMenuW(parent, MF_SEPARATOR, 0, ptr::null());
                }
                Some(NativeMenuItem {
                    bitmap: None,
                    kind: NativeMenuItemKind::Separator,
                })
            },
            NormalizedMenuItem::Standard(item)
            | NormalizedMenuItem::Check(item)
            | NormalizedMenuItem::Radio(item) => self.append_command(parent, position, item),
            NormalizedMenuItem::Submenu(submenu) => self.append_submenu(parent, position, submenu),
        }
    }

    fn append_command(
        &mut self,
        parent: HMENU,
        position: u32,
        item: &NormalizedCommandItem<Message>,
    ) -> Option<NativeMenuItem> {
        let command = self.next_command();
        let text = encode_wide(label_with_accelerator(
            &item.label,
            item.accelerator.as_ref(),
        ));
        unsafe {
            AppendMenuW(
                parent,
                command_flags(item.state, item.enabled),
                command as usize,
                text.as_ptr(),
            );
        }

        let bitmap = bitmap_from_icon(item.icon.as_ref());
        set_menu_bitmap_by_position(parent, position, bitmap.as_ref());
        self.command_map.insert(command, item.message.clone());

        Some(NativeMenuItem {
            bitmap,
            kind: NativeMenuItemKind::Command { command },
        })
    }

    fn append_submenu(
        &mut self,
        parent: HMENU,
        position: u32,
        submenu: &NormalizedSubmenu<Message>,
    ) -> Option<NativeMenuItem> {
        let popup = unsafe { CreatePopupMenu() };
        if popup.is_null() {
            return None;
        }

        let children = self.append_items(popup, &submenu.children);
        let text = encode_wide(submenu.label.as_str());
        let mut flags = MF_POPUP;
        if !submenu.enabled {
            flags |= MF_DISABLED | MF_GRAYED;
        }

        unsafe {
            AppendMenuW(parent, flags, popup as usize, text.as_ptr());
        }

        let bitmap = bitmap_from_icon(submenu.icon.as_ref());
        set_menu_bitmap_by_position(parent, position, bitmap.as_ref());

        Some(NativeMenuItem {
            bitmap,
            kind: NativeMenuItemKind::Submenu {
                menu: popup,
                children,
            },
        })
    }

    fn next_command(&mut self) -> u32 {
        let command = self.next_command;
        self.next_command = self.next_command.saturating_add(1);
        command
    }
}

fn label_with_accelerator(label: &str, accelerator: Option<&Accelerator>) -> String {
    match accelerator {
        Some(accelerator) => format!("{label}\t{accelerator}"),
        None => label.to_string(),
    }
}

fn command_flags(state: CommandState, enabled: bool) -> u32 {
    let mut flags = MF_STRING;
    match state {
        CommandState::Standard => {},
        CommandState::Check { checked } | CommandState::Radio { selected: checked } => {
            if checked {
                flags |= MF_CHECKED;
            } else {
                flags |= MF_UNCHECKED;
            }
        },
    }

    if !enabled {
        flags |= MF_DISABLED | MF_GRAYED;
    }

    flags
}

fn command_state(state: CommandState, enabled: bool) -> u32 {
    let mut flags = if enabled { MFS_ENABLED } else { MFS_DISABLED };

    match state {
        CommandState::Standard => {},
        CommandState::Check { checked } | CommandState::Radio { selected: checked } => {
            if checked {
                flags |= MFS_CHECKED;
            } else {
                flags |= MFS_UNCHECKED;
            }
        },
    }

    flags
}

fn bitmap_from_icon(icon: Option<&Icon>) -> Option<OwnedBitmap> {
    let icon = icon?;
    OwnedBitmap::from_icon(icon).ok()
}

fn bitmap_handle(bitmap: Option<&OwnedBitmap>) -> windows_sys::Win32::Graphics::Gdi::HBITMAP {
    bitmap.map_or(ptr::null_mut(), OwnedBitmap::handle)
}

fn set_menu_bitmap_by_position(parent: HMENU, position: u32, bitmap: Option<&OwnedBitmap>) {
    let mut info: MENUITEMINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MENUITEMINFOW>() as _;
    info.fMask = MIIM_BITMAP;
    info.hbmpItem = bitmap_handle(bitmap);

    unsafe {
        SetMenuItemInfoW(parent, position, TRUE, &info);
    }
}

pub fn show_popup_menu(hwnd: HWND, menu: HMENU) -> Option<u32> {
    // Reference: muda/src/platform_impl/windows/mod.rs::show_context_menu.
    let mut point = POINT { x: 0, y: 0 };
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point);
        SetForegroundWindow(hwnd);
        let result = TrackPopupMenu(
            menu,
            TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_RETURNCMD,
            point.x,
            point.y,
            0,
            hwnd,
            ptr::null(),
        );
        (result > 0).then_some(result as u32)
    }
}

pub fn attach_window_subclass(hwnd: HWND) {
    unsafe {
        // Reference:
        // muda/src/platform_impl/windows/mod.rs::attach_menu_subclass_for_hwnd.
        SetWindowSubclass(hwnd, Some(menu_subclass_proc), MENU_SUBCLASS_ID, 0);
    }
}

pub fn detach_window_subclass(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }

    unsafe {
        RemoveWindowSubclass(hwnd, Some(menu_subclass_proc), MENU_SUBCLASS_ID);
    }
}

unsafe extern "system" fn menu_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uidsubclass: usize,
    _dwrefdata: usize,
) -> LRESULT {
    match msg {
        dark_menu_bar::WM_UAHDRAWMENU | dark_menu_bar::WM_UAHDRAWMENUITEM => {
            if dark_menu_bar::should_use_dark_mode(hwnd) {
                // Reference: muda/src/platform_impl/windows/mod.rs::menu_subclass_proc.
                dark_menu_bar::draw(hwnd, msg, wparam, lparam);
                0
            } else {
                unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
            }
        },
        WM_NCACTIVATE | WM_NCPAINT => {
            let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            if dark_menu_bar::should_use_dark_mode(hwnd) {
                dark_menu_bar::draw(hwnd, msg, wparam, lparam);
            }
            result
        },
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}
