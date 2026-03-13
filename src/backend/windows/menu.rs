use std::{collections::HashMap, ptr};

use windows_sys::Win32::{
    Foundation::{FALSE, HWND, LPARAM, LRESULT, POINT, WPARAM},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, HMENU, MENUITEMINFOW, MF_CHECKED,
            MF_DISABLED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MIIM_BITMAP,
            SetForegroundWindow, SetMenuItemInfoW, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD,
            TrackPopupMenu, WM_NCACTIVATE, WM_NCPAINT,
        },
    },
};

use crate::{Accelerator, MenuItem};

use super::util::encode_wide;
use super::{dark_menu_bar, icon::OwnedBitmap};

const MENU_SUBCLASS_ID: usize = 200;

#[derive(Debug)]
pub(crate) struct RenderedMenu<Id> {
    root: HMENU,
    command_map: HashMap<u32, Id>,
    _bitmaps: Vec<OwnedBitmap>,
}

impl<Id: Clone + Eq> RenderedMenu<Id> {
    pub(crate) fn from_items(items: &[MenuItem<Id>]) -> Option<Self> {
        let root = unsafe { CreatePopupMenu() };
        if root.is_null() {
            return None;
        }

        let mut builder = MenuBuilder {
            next_command: 1,
            added_items: 0,
            command_map: HashMap::new(),
            bitmaps: Vec::new(),
        };
        builder.append_items(root, items);

        if builder.added_items == 0 {
            unsafe {
                DestroyMenu(root);
            }
            None
        } else {
            Some(Self {
                root,
                command_map: builder.command_map,
                _bitmaps: builder.bitmaps,
            })
        }
    }

    pub(crate) fn handle(&self) -> HMENU {
        self.root
    }

    pub(crate) fn resolve(&self, command: u32) -> Option<Id> {
        self.command_map.get(&command).cloned()
    }
}

impl<Id> Drop for RenderedMenu<Id> {
    fn drop(&mut self) {
        unsafe {
            DestroyMenu(self.root);
        }
    }
}

struct MenuBuilder<Id> {
    next_command: u32,
    added_items: usize,
    command_map: HashMap<u32, Id>,
    bitmaps: Vec<OwnedBitmap>,
}

impl<Id: Clone + Eq> MenuBuilder<Id> {
    fn append_items(&mut self, parent: HMENU, items: &[MenuItem<Id>]) {
        for item in items {
            self.append_item(parent, item);
        }
    }

    fn append_item(&mut self, parent: HMENU, item: &MenuItem<Id>) {
        match item {
            MenuItem::Separator => {
                unsafe {
                    AppendMenuW(parent, MF_SEPARATOR, 0, ptr::null());
                }
                self.added_items += 1;
            }
            MenuItem::Action(action) if action.visible => {
                let command = self.next_command();
                let text = encode_wide(label_with_accelerator(
                    &action.label,
                    action.accelerator.as_ref(),
                ));
                let mut flags = MF_STRING;
                if !action.enabled {
                    flags |= MF_DISABLED | MF_GRAYED;
                }
                unsafe {
                    AppendMenuW(parent, flags, command as usize, text.as_ptr());
                }
                self.command_map.insert(command, action.id.clone());
                self.add_icon(parent, command, action.icon.as_ref());
                self.added_items += 1;
            }
            MenuItem::Check(check) if check.visible => {
                let command = self.next_command();
                let text = encode_wide(label_with_accelerator(
                    &check.label,
                    check.accelerator.as_ref(),
                ));
                let mut flags = MF_STRING;
                if check.checked {
                    flags |= MF_CHECKED;
                } else {
                    flags |= MF_UNCHECKED;
                }
                if !check.enabled {
                    flags |= MF_DISABLED | MF_GRAYED;
                }
                unsafe {
                    AppendMenuW(parent, flags, command as usize, text.as_ptr());
                }
                self.command_map.insert(command, check.id.clone());
                self.add_icon(parent, command, check.icon.as_ref());
                self.added_items += 1;
            }
            MenuItem::RadioGroup(group) if group.visible => {
                for option in group.options.iter().filter(|option| option.visible) {
                    let command = self.next_command();
                    let text = encode_wide(label_with_accelerator(
                        &option.label,
                        option.accelerator.as_ref(),
                    ));
                    let mut flags = MF_STRING;
                    if group.selected.as_ref() == Some(&option.id) {
                        flags |= MF_CHECKED;
                    } else {
                        flags |= MF_UNCHECKED;
                    }
                    if !group.enabled || !option.enabled {
                        flags |= MF_DISABLED | MF_GRAYED;
                    }
                    unsafe {
                        AppendMenuW(parent, flags, command as usize, text.as_ptr());
                    }
                    self.command_map.insert(command, option.id.clone());
                    self.add_icon(parent, command, option.icon.as_ref());
                    self.added_items += 1;
                }
            }
            MenuItem::Submenu(submenu) if submenu.visible => {
                let popup = unsafe { CreatePopupMenu() };
                if popup.is_null() {
                    return;
                }
                self.append_items(popup, &submenu.children);
                let text = encode_wide(submenu.label.as_str());
                let mut flags = MF_POPUP;
                if !submenu.enabled {
                    flags |= MF_DISABLED | MF_GRAYED;
                }
                unsafe {
                    AppendMenuW(parent, flags, popup as usize, text.as_ptr());
                }
                self.added_items += 1;
            }
            _ => {}
        }
    }

    fn next_command(&mut self) -> u32 {
        let command = self.next_command;
        self.next_command = self.next_command.saturating_add(1);
        command
    }

    fn add_icon(&mut self, parent: HMENU, command: u32, icon: Option<&crate::Icon>) {
        let Some(icon) = icon else {
            return;
        };
        let Ok(bitmap) = OwnedBitmap::from_icon(icon) else {
            return;
        };

        let info = menu_bitmap_info(bitmap.handle());
        unsafe {
            SetMenuItemInfoW(parent, command, FALSE, &info);
        }
        self.bitmaps.push(bitmap);
    }
}

fn label_with_accelerator(label: &str, accelerator: Option<&Accelerator>) -> String {
    match accelerator {
        Some(accelerator) => format!("{label}\t{accelerator}"),
        None => label.to_string(),
    }
}

fn menu_bitmap_info(bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP) -> MENUITEMINFOW {
    let mut info: MENUITEMINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MENUITEMINFOW>() as _;
    info.fMask = MIIM_BITMAP;
    info.hbmpItem = bitmap;
    info
}

pub(crate) fn show_popup_menu(hwnd: HWND, menu: HMENU) -> Option<u32> {
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

pub(crate) fn attach_window_subclass(hwnd: HWND) {
    unsafe {
        // Reference: muda/src/platform_impl/windows/mod.rs::attach_menu_subclass_for_hwnd.
        SetWindowSubclass(hwnd, Some(menu_subclass_proc), MENU_SUBCLASS_ID, 0);
    }
}

pub(crate) fn detach_window_subclass(hwnd: HWND) {
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
        }
        WM_NCACTIVATE | WM_NCPAINT => {
            let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            if dark_menu_bar::should_use_dark_mode(hwnd) {
                dark_menu_bar::draw(hwnd, msg, wparam, lparam);
            }
            result
        }
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}
