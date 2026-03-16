use std::collections::HashMap;
use std::{fmt, io, ptr};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, TRUE, WPARAM};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ACCEL, AppendMenuW, CreateAcceleratorTableW, CreatePopupMenu, DestroyAcceleratorTable,
    DestroyMenu, HACCEL, HMENU, MENUITEMINFOW, MF_CHECKED, MF_DISABLED, MF_GRAYED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MFS_CHECKED, MFS_DISABLED, MFS_ENABLED, MFS_UNCHECKED,
    MFT_RADIOCHECK, MIIM_BITMAP, MIIM_FTYPE, MIIM_STATE, MIIM_STRING, MSG, SetForegroundWindow,
    SetMenuItemInfoW, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, TrackPopupMenu,
    TranslateAcceleratorW, WM_NCACTIVATE, WM_NCPAINT,
};

use super::icon::OwnedBitmap;
use super::util::encode_wide;
use super::{accelerator, dark_menu_bar};
use crate::menu::Accelerator;
use crate::model::{
    CommandState, MenuPatch, NormalizedCommandItem, NormalizedMenuItem, NormalizedSubmenu,
};
use crate::{Error, Icon, Result};

const MENU_SUBCLASS_ID: usize = 200;

#[derive(Debug)]
pub struct RenderedMenu<Message> {
    root: HMENU,
    items: Vec<NativeMenuItem>,
    command_map: HashMap<u32, Message>,
    accelerator_table: OwnedAcceleratorTable,
}

impl<Message: Clone> RenderedMenu<Message> {
    pub fn from_model(model_items: &[NormalizedMenuItem<Message>]) -> Result<Option<Self>> {
        let root = unsafe { CreatePopupMenu() };
        if root.is_null() {
            return Err(Error::Os(io::Error::last_os_error()));
        }

        let mut builder = MenuBuilder {
            next_command: 1,
            command_map: HashMap::new(),
        };
        let items = match builder.append_items(root, model_items) {
            Ok(items) => items,
            Err(error) => {
                unsafe {
                    DestroyMenu(root);
                }
                return Err(error);
            },
        };

        if items.is_empty() {
            unsafe {
                DestroyMenu(root);
            }
            Ok(None)
        } else {
            let mut rendered = Self {
                root,
                items,
                command_map: builder.command_map,
                accelerator_table: OwnedAcceleratorTable::new(),
            };
            rendered.sync_bindings(model_items)?;
            Ok(Some(rendered))
        }
    }

    pub fn handle(&self) -> HMENU {
        self.root
    }

    pub fn accelerator_handle(&self) -> HACCEL {
        self.accelerator_table.handle()
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

    pub fn sync_bindings(&mut self, items: &[NormalizedMenuItem<Message>]) -> Result<bool> {
        let mut command_map = HashMap::new();
        let mut accelerators = Vec::new();
        if !Self::collect_bindings(&self.items, items, &mut command_map, &mut accelerators)? {
            return Ok(false);
        }

        self.command_map = command_map;
        self.accelerator_table.replace(&accelerators)?;
        Ok(true)
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
        info.fMask = MIIM_STRING | MIIM_STATE | MIIM_BITMAP | MIIM_FTYPE;
        info.fState = command_state(item.state, item.enabled);
        info.fType = command_type(item.state);
        info.dwTypeData = text.as_ptr() as *mut _;
        info.cch = text.len().saturating_sub(1) as _;
        info.hbmpItem = bitmap_handle(bitmap.as_ref());

        let ok = unsafe { SetMenuItemInfoW(parent, position as u32, TRUE, &info) != 0 };
        if !ok {
            return false;
        }

        node.bitmap = bitmap;
        match &item.message {
            Some(message) => {
                self.command_map.insert(command, message.clone());
            },
            None => {
                self.command_map.remove(&command);
            },
        }
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
        info.fMask = MIIM_STRING | MIIM_STATE | MIIM_BITMAP | MIIM_FTYPE;
        info.fState = if item.enabled {
            MFS_ENABLED
        } else {
            MFS_DISABLED
        };
        info.fType = 0;
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

    fn collect_bindings(
        nodes: &[NativeMenuItem],
        items: &[NormalizedMenuItem<Message>],
        command_map: &mut HashMap<u32, Message>,
        accelerators: &mut Vec<ACCEL>,
    ) -> Result<bool> {
        if nodes.len() != items.len() {
            return Ok(false);
        }

        for (node, item) in nodes.iter().zip(items) {
            match (&node.kind, item) {
                (NativeMenuItemKind::Separator, NormalizedMenuItem::Separator) => {},
                (
                    NativeMenuItemKind::Command { command },
                    NormalizedMenuItem::Standard(item)
                    | NormalizedMenuItem::Check(item)
                    | NormalizedMenuItem::Radio(item),
                ) => {
                    if let Some(message) = &item.message {
                        command_map.insert(*command, message.clone());
                    }
                    if item.enabled && item.message.is_some() {
                        if let Some(accelerator) = item.accelerator.as_ref() {
                            let command = u16::try_from(*command).map_err(|_| {
                                Error::Unsupported(
                                    "too many menu items for the Windows accelerator table",
                                )
                            })?;
                            accelerators.push(
                                accelerator::to_accel(accelerator, command)
                                    .map_err(Error::Accelerator)?,
                            );
                        }
                    }
                },
                (
                    NativeMenuItemKind::Submenu { children, .. },
                    NormalizedMenuItem::Submenu(submenu),
                ) => {
                    if !Self::collect_bindings(
                        children,
                        &submenu.children,
                        command_map,
                        accelerators,
                    )? {
                        return Ok(false);
                    }
                },
                _ => return Ok(false),
            }
        }

        Ok(true)
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
struct OwnedAcceleratorTable {
    handle: HACCEL,
}

impl OwnedAcceleratorTable {
    fn new() -> Self {
        Self {
            handle: ptr::null_mut(),
        }
    }

    fn handle(&self) -> HACCEL {
        self.handle
    }

    fn replace(&mut self, accelerators: &[ACCEL]) -> Result<()> {
        let new_handle = if accelerators.is_empty() {
            ptr::null_mut()
        } else {
            let handle = unsafe {
                CreateAcceleratorTableW(accelerators.as_ptr(), accelerators.len() as i32)
            };
            if handle.is_null() {
                return Err(Error::Os(io::Error::last_os_error()));
            }
            handle
        };

        if !self.handle.is_null() {
            unsafe {
                DestroyAcceleratorTable(self.handle);
            }
        }

        self.handle = new_handle;
        Ok(())
    }
}

impl Drop for OwnedAcceleratorTable {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                DestroyAcceleratorTable(self.handle);
            }
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

impl<Message: Clone> MenuBuilder<Message> {
    fn append_items(
        &mut self,
        parent: HMENU,
        items: &[NormalizedMenuItem<Message>],
    ) -> Result<Vec<NativeMenuItem>> {
        let mut rendered = Vec::with_capacity(items.len());

        for item in items {
            if let Some(rendered_item) = self.append_item(parent, rendered.len() as u32, item)? {
                rendered.push(rendered_item);
            }
        }

        Ok(rendered)
    }

    fn append_item(
        &mut self,
        parent: HMENU,
        position: u32,
        item: &NormalizedMenuItem<Message>,
    ) -> Result<Option<NativeMenuItem>> {
        match item {
            NormalizedMenuItem::Separator => {
                unsafe {
                    AppendMenuW(parent, MF_SEPARATOR, 0, ptr::null());
                }
                Ok(Some(NativeMenuItem {
                    bitmap: None,
                    kind: NativeMenuItemKind::Separator,
                }))
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
    ) -> Result<Option<NativeMenuItem>> {
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
        set_menu_type_by_position(parent, position, item.state);
        set_menu_bitmap_by_position(parent, position, bitmap.as_ref());
        if let Some(message) = &item.message {
            self.command_map.insert(command, message.clone());
        }

        Ok(Some(NativeMenuItem {
            bitmap,
            kind: NativeMenuItemKind::Command { command },
        }))
    }

    fn append_submenu(
        &mut self,
        parent: HMENU,
        position: u32,
        submenu: &NormalizedSubmenu<Message>,
    ) -> Result<Option<NativeMenuItem>> {
        let popup = unsafe { CreatePopupMenu() };
        if popup.is_null() {
            return Err(Error::Os(io::Error::last_os_error()));
        }

        let children = match self.append_items(popup, &submenu.children) {
            Ok(children) => children,
            Err(error) => {
                unsafe {
                    DestroyMenu(popup);
                }
                return Err(error);
            },
        };
        let text = encode_wide(submenu.label.as_str());
        let mut flags = MF_POPUP;
        if !submenu.enabled {
            flags |= MF_DISABLED | MF_GRAYED;
        }

        if unsafe { AppendMenuW(parent, flags, popup as usize, text.as_ptr()) } == 0 {
            unsafe {
                DestroyMenu(popup);
            }
            return Err(Error::Os(io::Error::last_os_error()));
        }

        let bitmap = bitmap_from_icon(submenu.icon.as_ref());
        set_menu_bitmap_by_position(parent, position, bitmap.as_ref());

        Ok(Some(NativeMenuItem {
            bitmap,
            kind: NativeMenuItemKind::Submenu {
                menu: popup,
                children,
            },
        }))
    }

    fn next_command(&mut self) -> u32 {
        let command = self.next_command;
        self.next_command = self.next_command.saturating_add(1);
        command
    }
}

fn label_with_accelerator(label: &str, accelerator: Option<&Accelerator>) -> String {
    match accelerator {
        // Reference: Win32 menu shortcut text is conventionally appended after
        // "\t". Microsoft documents "\a" as a more general right-align escape,
        // but the shortcut-specific guidance uses "\t", and `muda` follows the
        // same pattern:
        // https://learn.microsoft.com/en-us/windows/win32/menurc/about-menus
        // https://learn.microsoft.com/en-us/windows/win32/menurc/menuitem-statement
        Some(accelerator) => format!("{label}\t{}", AcceleratorLabel(accelerator)),
        None => label.to_string(),
    }
}

struct AcceleratorLabel<'a>(&'a Accelerator);

impl fmt::Display for AcceleratorLabel<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        accelerator::fmt_label(self.0, f)
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

fn command_type(state: CommandState) -> u32 {
    match state {
        CommandState::Radio { .. } => MFT_RADIOCHECK,
        _ => 0,
    }
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

fn set_menu_type_by_position(parent: HMENU, position: u32, state: CommandState) {
    let mut info: MENUITEMINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MENUITEMINFOW>() as _;
    info.fMask = MIIM_FTYPE;
    info.fType = command_type(state);

    unsafe {
        SetMenuItemInfoW(parent, position, TRUE, &info);
    }
}

pub fn show_popup_menu(hwnd: HWND, menu: HMENU, anchor: Option<POINT>) -> Option<u32> {
    // Reference: muda/src/platform_impl/windows/mod.rs::show_context_menu.
    let mut point = anchor.unwrap_or(POINT { x: 0, y: 0 });
    unsafe {
        if anchor.is_none() {
            windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point);
        }
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

pub unsafe fn process_accelerator_message(hwnd: HWND, haccel: HACCEL, msg: *const MSG) -> bool {
    if hwnd.is_null() || haccel.is_null() || msg.is_null() {
        return false;
    }

    // Reference: muda/examples/winit.rs and Menu::init_for_hwnd docs, which run
    // TranslateAcceleratorW from the host loop for Windows accelerators.
    unsafe { TranslateAcceleratorW(hwnd, haccel, &*msg) == 1 }
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

#[cfg(test)]
mod tests {
    use super::RenderedMenu;
    use crate::menu::{Accelerator, Code, Modifiers};
    use crate::model::{CommandState, NormalizedCommandItem, NormalizedMenuItem};

    #[test]
    fn disabled_items_do_not_bind_accelerators() {
        let items = vec![NormalizedMenuItem::Standard(NormalizedCommandItem {
            message: Some(()),
            label: "Quit".to_string(),
            enabled: false,
            icon: None,
            icon_name: None,
            accelerator: Some(Accelerator::new(Some(Modifiers::CONTROL), Code::KeyQ)),
            state: CommandState::Standard,
        })];

        let rendered = RenderedMenu::from_model(&items).unwrap().unwrap();
        assert!(rendered.accelerator_handle().is_null());
    }
}
