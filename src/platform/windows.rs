use std::collections::HashMap;
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CS_HREDRAW, CS_VREDRAW, ChangeWindowMessageFilterEx, CreatePopupMenu,
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW,
    GWLP_USERDATA, GetCursorPos, GetMessageW, GetWindowLongPtrW, HICON, MF_CHECKED, MF_ENABLED,
    MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSG, MSGFLT_ALLOW, PostMessageW,
    PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow,
    SetWindowLongPtrW, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, WM_APP, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

use crate::backend::plan::{BackendCommandId, MenuPlan, PlannedNode, PlannedNodeKind, plan_menu};
use crate::backend::{BackendCommand, BackendRuntime};
use crate::{
    ActivationMode, EventSink, Icon, MenuItemId, TrayError, TrayEvent, TrayIconEventKind,
    TrayResult, TrayState, TrayStatus,
};

const TRAY_UID: u32 = 1;
const WM_TRAYICON: u32 = WM_APP + 1;
const WM_BACKEND_COMMAND: u32 = WM_APP + 2;

#[derive(Debug, Default)]
pub struct PlatformOptions;

pub(crate) fn spawn(
    initial_state: TrayState,
    sink: Arc<dyn EventSink>,
    _options: PlatformOptions,
) -> TrayResult<BackendRuntime> {
    let (command_tx, command_rx) = mpsc::channel();
    let (init_tx, init_rx) = mpsc::channel();

    let join = thread::Builder::new()
        .name("trayinit-windows-backend".into())
        .spawn(move || backend_thread(initial_state, sink, command_rx, init_tx))
        .map_err(|err| TrayError::ThreadInit(err.to_string()))?;

    let hwnd = init_rx.recv().map_err(|_| {
        TrayError::ThreadInit("backend thread exited during initialization".into())
    })??;

    let wake = Arc::new(move || unsafe {
        let _ = PostMessageW(hwnd as HWND, WM_BACKEND_COMMAND, 0, 0);
    });

    Ok(BackendRuntime::new(command_tx, wake, join))
}

fn backend_thread(
    initial_state: TrayState,
    sink: Arc<dyn EventSink>,
    command_rx: Receiver<BackendCommand>,
    init_tx: mpsc::Sender<TrayResult<isize>>,
) {
    let class_name = wide_z("TrayinitHiddenWindow");
    let taskbar_created_msg = unsafe { RegisterWindowMessageW(wide_z("TaskbarCreated").as_ptr()) };

    let hinstance = unsafe { GetModuleHandleW(null()) };
    if hinstance.is_null() {
        let _ = init_tx.send(Err(TrayError::ThreadInit(
            "GetModuleHandleW returned null".into(),
        )));
        return;
    }

    let registered = unsafe {
        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&wnd_class)
    };

    if registered == 0 {
        let err = unsafe { GetLastError() };
        if err != ERROR_CLASS_ALREADY_EXISTS {
            let _ = init_tx.send(Err(TrayError::ThreadInit(format!(
                "RegisterClassW failed with {err}"
            ))));
            return;
        }
    }

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            class_name.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            hinstance,
            null_mut(),
        )
    };

    if hwnd.is_null() {
        let err = unsafe { GetLastError() };
        let _ = init_tx.send(Err(TrayError::ThreadInit(format!(
            "CreateWindowExW failed with {err}"
        ))));
        return;
    }

    let mut state = ThreadState {
        hwnd,
        command_rx,
        sink,
        taskbar_created_msg,
        native: NativeTray::new(initial_state),
        closed: false,
    };
    let state_ptr = &mut state as *mut ThreadState;

    unsafe {
        let _ = ChangeWindowMessageFilterEx(hwnd, taskbar_created_msg, MSGFLT_ALLOW, null_mut());
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
    }

    state.apply_current_state();
    let _ = init_tx.send(Ok(hwnd as isize));

    unsafe {
        let mut msg = MSG {
            hwnd: null_mut(),
            message: 0,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: POINT { x: 0, y: 0 },
        };

        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    state.native.delete_icon(hwnd);
    state.native.destroy_menu();
    state.native.destroy_icon();
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
    }
}

struct ThreadState {
    hwnd: HWND,
    command_rx: Receiver<BackendCommand>,
    sink: Arc<dyn EventSink>,
    taskbar_created_msg: u32,
    native: NativeTray,
    closed: bool,
}

struct NativeTray {
    state: TrayState,
    icon_added: bool,
    hicon: HICON,
    hmenu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU,
    command_map: HashMap<BackendCommandId, MenuItemId>,
}

enum ApplyError {
    TemporarilyUnavailable(String),
    Backend(String),
}

impl ThreadState {
    fn drain_commands(&mut self) {
        loop {
            match self.command_rx.try_recv() {
                Ok(BackendCommand::SetState(state)) => {
                    self.native.state = state;
                    self.apply_current_state();
                },
                Ok(BackendCommand::Close) => {
                    self.closed = true;
                    self.native.delete_icon(self.hwnd);
                    unsafe {
                        DestroyWindow(self.hwnd);
                    }
                    break;
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.closed = true;
                    self.native.delete_icon(self.hwnd);
                    unsafe {
                        DestroyWindow(self.hwnd);
                    }
                    break;
                },
            }
        }
    }

    fn apply_current_state(&mut self) {
        if let Err(err) = self.native.apply(self.hwnd) {
            let status = match err {
                ApplyError::TemporarilyUnavailable(message) => {
                    TrayStatus::TemporarilyUnavailable(message)
                },
                ApplyError::Backend(message) => TrayStatus::BackendError(message),
            };
            let event = TrayEvent::StatusChanged { status };
            self.sink.send(event);
        }
    }

    fn handle_taskbar_created(&mut self) {
        self.native.icon_added = false;
        self.apply_current_state();
    }

    fn handle_tray_message(&mut self, message: u32) {
        let kind = match message {
            WM_LBUTTONDBLCLK => Some(TrayIconEventKind::DoubleClick),
            WM_LBUTTONUP => Some(TrayIconEventKind::PrimaryClick),
            WM_RBUTTONUP => Some(TrayIconEventKind::SecondaryClick),
            _ => None,
        };

        let Some(kind) = kind else {
            return;
        };

        let show_menu = match self.native.state.activation_mode {
            ActivationMode::PlatformDefault | ActivationMode::MenuOnSecondaryClick => {
                kind == TrayIconEventKind::SecondaryClick
            },
            ActivationMode::MenuOnPrimaryClick => kind == TrayIconEventKind::PrimaryClick,
        };

        let event = TrayEvent::IconActivated {
            kind,
            position: cursor_position(),
            rect: None,
        };
        self.sink.send(event);

        if show_menu
            && !self.native.hmenu.is_null()
            && let Some(item_id) = self.native.track_menu(self.hwnd)
        {
            let event = TrayEvent::MenuItemActivated { item_id };
            self.sink.send(event);
        }
    }
}

impl NativeTray {
    fn new(state: TrayState) -> Self {
        Self {
            state,
            icon_added: false,
            hicon: null_mut(),
            hmenu: null_mut(),
            command_map: HashMap::new(),
        }
    }

    fn apply(&mut self, hwnd: HWND) -> Result<(), ApplyError> {
        self.replace_menu()?;
        let next_hicon = self.prepare_icon()?;
        self.sync_shell_icon(hwnd, next_hicon)
    }

    fn prepare_icon(&self) -> Result<Option<HICON>, ApplyError> {
        if !self.state.visible {
            return Ok(None);
        }

        self.state
            .icon
            .as_ref()
            .map(create_hicon)
            .transpose()
            .map_err(ApplyError::Backend)
    }

    fn replace_menu(&mut self) -> Result<(), ApplyError> {
        self.destroy_menu();
        self.command_map.clear();

        let Some(menu) = &self.state.menu else {
            return Ok(());
        };

        let plan = plan_menu(menu).map_err(|err| ApplyError::Backend(err.to_string()))?;
        let hmenu = build_menu(&plan).map_err(ApplyError::Backend)?;
        self.hmenu = hmenu;
        self.command_map = plan.command_map;
        Ok(())
    }

    fn sync_shell_icon(&mut self, hwnd: HWND, next_hicon: Option<HICON>) -> Result<(), ApplyError> {
        let Some(next_hicon) = next_hicon else {
            self.delete_icon(hwnd);
            self.destroy_icon();
            return Ok(());
        };

        let nid = notify_icon_data(hwnd, next_hicon, self.state.tooltip.as_deref());
        let op = if self.icon_added { NIM_MODIFY } else { NIM_ADD };
        let ok = unsafe { Shell_NotifyIconW(op, &nid) };
        if ok == 0 {
            let message = shell_notify_error(op);
            unsafe {
                DestroyIcon(next_hicon);
            }

            return if op == NIM_ADD {
                Err(ApplyError::TemporarilyUnavailable(message))
            } else {
                Err(ApplyError::Backend(message))
            };
        }

        // Keep the previous HICON alive until the shell has accepted the new
        // handle. This mirrors the reference implementations' RAII lifetime.
        self.destroy_icon();
        self.hicon = next_hicon;
        self.icon_added = true;
        Ok(())
    }

    fn delete_icon(&mut self, hwnd: HWND) {
        if !self.icon_added {
            return;
        }

        let nid = notify_icon_data(hwnd, self.hicon, None);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
        self.icon_added = false;
    }

    fn destroy_icon(&mut self) {
        if !self.hicon.is_null() {
            unsafe {
                DestroyIcon(self.hicon);
            }
            self.hicon = null_mut();
        }
    }

    fn destroy_menu(&mut self) {
        if !self.hmenu.is_null() {
            unsafe {
                DestroyMenu(self.hmenu);
            }
            self.hmenu = null_mut();
        }
    }

    fn track_menu(&mut self, hwnd: HWND) -> Option<MenuItemId> {
        let mut point = POINT { x: 0, y: 0 };
        let ok = unsafe { GetCursorPos(&mut point) };
        if ok == 0 {
            return None;
        }

        unsafe {
            SetForegroundWindow(hwnd);
        }

        let command = unsafe {
            TrackPopupMenu(
                self.hmenu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
                point.x,
                point.y,
                0,
                hwnd,
                null(),
            )
        };

        if command == 0 {
            return None;
        }

        self.command_map
            .get(&(command as BackendCommandId))
            .cloned()
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ThreadState };

    if !state_ptr.is_null() {
        let state = unsafe { &mut *state_ptr };
        if msg == WM_BACKEND_COMMAND {
            state.drain_commands();
            return 0;
        }

        if msg == WM_TRAYICON {
            state.handle_tray_message(lparam as u32);
            return 0;
        }

        if msg == state.taskbar_created_msg {
            state.handle_taskbar_created();
            return 0;
        }

        if msg == WM_DESTROY {
            if !state.closed {
                state.native.delete_icon(hwnd);
            }
            unsafe {
                PostQuitMessage(0);
            }
            return 0;
        }
    }

    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn notify_icon_data(hwnd: HWND, hicon: HICON, tooltip: Option<&str>) -> NOTIFYICONDATAW {
    let mut nid = unsafe { std::mem::zeroed::<NOTIFYICONDATAW>() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAYICON;
    nid.hIcon = hicon;

    if let Some(tooltip) = tooltip {
        copy_wide_truncated(&mut nid.szTip, tooltip);
    }

    nid
}

fn shell_notify_error(op: u32) -> String {
    let code = unsafe { GetLastError() };
    let op_name = match op {
        NIM_ADD => "NIM_ADD",
        NIM_MODIFY => "NIM_MODIFY",
        NIM_DELETE => "NIM_DELETE",
        _ => "unknown",
    };
    format!("Shell_NotifyIconW({op_name}) failed with GetLastError={code}")
}

fn build_menu(
    plan: &MenuPlan,
) -> Result<windows_sys::Win32::UI::WindowsAndMessaging::HMENU, String> {
    let hmenu = unsafe { CreatePopupMenu() };
    if hmenu.is_null() {
        return Err("CreatePopupMenu failed".into());
    }

    for node in &plan.nodes {
        if let Err(err) = append_node(hmenu, node) {
            unsafe {
                DestroyMenu(hmenu);
            }
            return Err(err);
        }
    }

    Ok(hmenu)
}

fn append_node(
    hmenu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU,
    node: &PlannedNode,
) -> Result<(), String> {
    let ok = match &node.kind {
        PlannedNodeKind::Item(item) => {
            let label = wide_z(&item.label);
            let flags = MF_STRING | enabled_flag(item.enabled);
            unsafe { AppendMenuW(hmenu, flags, item.command_id as usize, label.as_ptr()) }
        },
        PlannedNodeKind::Check(item) => {
            let label = wide_z(&item.label);
            let check = if item.checked {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            };
            let flags = MF_STRING | check | enabled_flag(item.enabled);
            unsafe { AppendMenuW(hmenu, flags, item.command_id as usize, label.as_ptr()) }
        },
        PlannedNodeKind::Submenu(submenu) => {
            let child_menu = unsafe { CreatePopupMenu() };
            if child_menu.is_null() {
                return Err("CreatePopupMenu for submenu failed".into());
            }

            for child in &node.children {
                if let Err(err) = append_node(child_menu, child) {
                    unsafe {
                        DestroyMenu(child_menu);
                    }
                    return Err(err);
                }
            }

            let label = wide_z(&submenu.label);
            let flags = MF_POPUP | enabled_flag(submenu.enabled);
            unsafe { AppendMenuW(hmenu, flags, child_menu as usize, label.as_ptr()) }
        },
        PlannedNodeKind::Separator => unsafe { AppendMenuW(hmenu, MF_SEPARATOR, 0, null()) },
    };

    if ok == 0 {
        Err("AppendMenuW failed".into())
    } else {
        Ok(())
    }
}

fn enabled_flag(enabled: bool) -> u32 {
    if enabled { MF_ENABLED } else { MF_GRAYED }
}

fn create_hicon(icon: &Icon) -> Result<HICON, String> {
    let pixels = icon
        .width()
        .checked_mul(icon.height())
        .and_then(|pixels| usize::try_from(pixels).ok())
        .ok_or_else(|| "icon dimensions overflow".to_string())?;
    let mut bgra = Vec::with_capacity(pixels * 4);
    for rgba in icon.rgba().chunks_exact(4) {
        bgra.push(rgba[2]);
        bgra.push(rgba[1]);
        bgra.push(rgba[0]);
        bgra.push(rgba[3]);
    }

    let mask = create_and_mask(icon.rgba(), icon.width(), icon.height())?;

    let hicon = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::CreateIcon(
            null_mut(),
            icon.width() as i32,
            icon.height() as i32,
            1,
            32,
            mask.as_ptr(),
            bgra.as_ptr(),
        )
    };

    if hicon.is_null() {
        Err("CreateIcon failed".into())
    } else {
        Ok(hicon)
    }
}

fn create_and_mask(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let stride = and_mask_stride(width)?;
    let height = usize::try_from(height).map_err(|_| "icon height overflow".to_string())?;
    let width = usize::try_from(width).map_err(|_| "icon width overflow".to_string())?;
    let mask_len = stride
        .checked_mul(height)
        .ok_or_else(|| "icon mask dimensions overflow".to_string())?;
    let mut mask = vec![0u8; mask_len];

    for y in 0..height {
        for x in 0..width {
            let pixel = y
                .checked_mul(width)
                .and_then(|row| row.checked_add(x))
                .ok_or_else(|| "icon pixel offset overflow".to_string())?;
            let alpha_offset = pixel
                .checked_mul(4)
                .and_then(|offset| offset.checked_add(3))
                .ok_or_else(|| "icon rgba offset overflow".to_string())?;
            let alpha = rgba
                .get(alpha_offset)
                .ok_or_else(|| "icon rgba buffer is shorter than dimensions".to_string())?;
            if *alpha == 0 {
                let byte = y * stride + x / 8;
                let bit = 0x80 >> (x % 8);
                mask[byte] |= bit;
            }
        }
    }

    Ok(mask)
}

fn and_mask_stride(width: u32) -> Result<usize, String> {
    let width = usize::try_from(width).map_err(|_| "icon width overflow".to_string())?;
    // CreateIcon expects a 1-bpp AND mask whose scanlines are padded to 32 bits.
    width
        .checked_add(31)
        .map(|width| (width / 32) * 4)
        .ok_or_else(|| "icon mask stride overflow".to_string())
}

fn cursor_position() -> Option<crate::PhysicalPosition> {
    let mut point = POINT { x: 0, y: 0 };
    let ok = unsafe { GetCursorPos(&mut point) };
    if ok == 0 {
        None
    } else {
        Some(crate::PhysicalPosition {
            x: point.x,
            y: point.y,
        })
    }
}

fn wide_z(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn copy_wide_truncated(dst: &mut [u16], value: &str) {
    if dst.is_empty() {
        return;
    }

    let max = dst.len() - 1;
    for (slot, code_unit) in dst.iter_mut().take(max).zip(value.encode_utf16()) {
        *slot = code_unit;
    }
    dst[max] = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn and_mask_stride_is_padded_to_32_bits() {
        assert_eq!(and_mask_stride(16).unwrap(), 4);
        assert_eq!(and_mask_stride(17).unwrap(), 4);
        assert_eq!(and_mask_stride(32).unwrap(), 4);
        assert_eq!(and_mask_stride(33).unwrap(), 8);
    }

    #[test]
    fn and_mask_len_uses_padded_stride_per_row() {
        assert_eq!(
            create_and_mask(&vec![255; 16 * 16 * 4], 16, 16)
                .unwrap()
                .len(),
            64
        );
        assert_eq!(create_and_mask(&vec![255; 17 * 4], 17, 1).unwrap().len(), 4);
        assert_eq!(create_and_mask(&vec![255; 32 * 4], 32, 1).unwrap().len(), 4);
        assert_eq!(create_and_mask(&vec![255; 33 * 4], 33, 1).unwrap().len(), 8);
    }

    #[test]
    fn and_mask_sets_bits_for_fully_transparent_pixels() {
        let mut rgba = vec![255; 9 * 2 * 4];
        rgba[3] = 0;
        rgba[8 * 4 + 3] = 0;
        rgba[(9 + 1) * 4 + 3] = 0;
        rgba[(9 + 2) * 4 + 3] = 128;

        let mask = create_and_mask(&rgba, 9, 2).unwrap();

        assert_eq!(mask.len(), 8);
        assert_eq!(mask[0], 0x80);
        assert_eq!(mask[1], 0x80);
        assert_eq!(mask[4], 0x40);
    }
}
