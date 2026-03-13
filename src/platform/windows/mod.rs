mod dark_menu_bar;
mod icon;
mod menu;
mod util;

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, mpsc};
use std::{io, ptr, thread};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, TRUE, WPARAM};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CW_USEDEFAULT, ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW,
    DestroyWindow, DispatchMessageW, GWL_USERDATA, GetCursorPos, GetMessageW, MSG, MSGFLT_ALLOW,
    PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageA, TranslateMessage,
    WM_CLOSE, WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_NCCREATE, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
    WS_OVERLAPPED,
};

use self::icon::OwnedIcon;
use self::menu::RenderedMenu;
use crate::{
    ActivateEvent, Builder, ClosedError, Error, Handle, PhysicalPosition, PhysicalSize, Rect,
    RuntimePreference, Tray, TrayEvent, TrayView,
};

const WM_USER_TRAYICON: u32 = 6002;
const WM_USER_REFRESH: u32 = 6003;

static NEXT_INTERNAL_ID: AtomicU32 = AtomicU32::new(1);
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

pub(crate) struct PlatformHandle<T: Tray> {
    shared: Arc<Shared<T>>,
}

impl<T: Tray> PlatformHandle<T> {
    fn new(shared: Arc<Shared<T>>) -> Self {
        Self { shared }
    }

    pub(crate) fn update<R>(
        &self,
        f: impl FnOnce(&mut T) -> R,
    ) -> core::result::Result<R, ClosedError> {
        if self.is_closed() {
            return Err(ClosedError);
        }

        let result = {
            let mut tray = self.shared.lock_tray();
            f(&mut tray)
        };

        self.refresh()?;
        Ok(result)
    }

    pub(crate) fn refresh(&self) -> core::result::Result<(), ClosedError> {
        self.shared.post_message(WM_USER_REFRESH)
    }

    pub(crate) fn shutdown(&self) -> crate::Result<()> {
        if self.is_closed() {
            return Ok(());
        }

        self.shared.post_close().map_err(Error::from)
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
    }
}

impl<T: Tray> Clone for PlatformHandle<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T: Tray> std::fmt::Debug for PlatformHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformHandle")
            .field("closed", &self.is_closed())
            .finish()
    }
}

pub(crate) fn spawn<T: Tray>(builder: Builder<T>) -> crate::Result<Handle<T>> {
    let Builder {
        tray,
        runtime_preference,
        linux: _,
    } = builder;

    if matches!(runtime_preference, RuntimePreference::CurrentThread) {
        return Err(Error::Unsupported(
            "current-thread Windows tray runtime is not implemented yet",
        ));
    }

    let tray_id = tray.id().to_string();
    let thread_name = format!("trayinit-{}", tray_id);
    let shared = Arc::new(Shared::new(tray));
    let init_shared = Arc::clone(&shared);
    let (init_tx, init_rx) = mpsc::sync_channel(1);

    thread::Builder::new()
        .name(thread_name)
        .spawn(move || backend_thread::<T>(init_shared, init_tx))
        .map_err(Error::Os)?;

    match init_rx.recv() {
        Ok(Ok(())) => Ok(Handle::new(tray_id, PlatformHandle::new(shared))),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(Error::Initialization(
            "Windows tray backend exited before initialization completed",
        )),
    }
}

fn backend_thread<T: Tray>(shared: Arc<Shared<T>>, init_tx: mpsc::SyncSender<crate::Result<()>>) {
    match run_backend_thread(shared) {
        Ok(()) => {
            let _ = init_tx.send(Ok(()));
            message_loop();
        },
        Err(error) => {
            let _ = init_tx.send(Err(error));
        },
    }
}

fn run_backend_thread<T: Tray>(shared: Arc<Shared<T>>) -> crate::Result<()> {
    // Reference: winit/src/platform_impl/windows/dpi.rs::become_dpi_aware.
    util::become_dpi_aware();
    // Reference:
    // tao/src/platform_impl/windows/dark_mode.rs::allow_dark_mode_for_app.
    dark_menu_bar::enable_dark_mode_for_app();

    let class_name = util::encode_wide("trayinit_hidden_window");
    register_window_class(&class_name)?;

    let user_data = Box::new(WindowUserData {
        ops: Box::new(WindowState::<T>::new(Arc::clone(&shared))),
    });
    let user_data_ptr = Box::into_raw(user_data);

    // Reference: tray-icon/src/platform_impl/windows/mod.rs::TrayIcon::new.
    // We keep the same hidden-window style combination so the helper window never
    // shows in the taskbar while still receiving tray callback messages.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            ptr::null(),
            WS_OVERLAPPED,
            CW_USEDEFAULT,
            0,
            CW_USEDEFAULT,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            util::get_instance_handle(),
            user_data_ptr.cast(),
        )
    };

    if hwnd.is_null() {
        unsafe {
            drop(Box::from_raw(user_data_ptr));
        }
        return Err(Error::Os(io::Error::last_os_error()));
    }

    // Reference: tray-icon/src/platform_impl/windows/mod.rs::TrayIcon::new.
    // Allow TaskbarCreated through UIPI so elevated apps can re-register after
    // explorer.exe restarts.
    unsafe {
        ChangeWindowMessageFilterEx(
            hwnd,
            taskbar_created_message(),
            MSGFLT_ALLOW,
            ptr::null_mut(),
        );
    }

    let initial_render = unsafe { (&mut *user_data_ptr).ops.initial_render() };
    if let Err(error) = initial_render {
        unsafe {
            DestroyWindow(hwnd);
        }
        return Err(error);
    }

    Ok(())
}

fn register_window_class(class_name: &[u16]) -> crate::Result<()> {
    let hinstance = util::get_instance_handle();
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        lpszClassName: class_name.as_ptr(),
        hInstance: hinstance,
        ..unsafe { std::mem::zeroed() }
    };

    let class = unsafe { RegisterClassW(&window_class) };
    if class == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(1410) {
            return Err(Error::Os(error));
        }
    }

    Ok(())
}

fn message_loop() {
    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        match result {
            -1 => break,
            0 => break,
            _ => unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            },
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let user_data_ptr = unsafe { util::get_window_long(hwnd, GWL_USERDATA) };
    let user_data_ptr = match (user_data_ptr, msg) {
        (0, WM_NCCREATE) => {
            let create_struct = unsafe { &mut *(lparam as *mut CREATESTRUCTW) };
            let user_data = unsafe { &mut *(create_struct.lpCreateParams as *mut WindowUserData) };
            user_data.ops.set_hwnd(hwnd);
            unsafe {
                util::set_window_long(hwnd, GWL_USERDATA, create_struct.lpCreateParams as isize);
            }
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        },
        (0, WM_CREATE) => return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        (0, _) => return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        _ => user_data_ptr as *mut WindowUserData,
    };

    let user_data = unsafe { &mut *user_data_ptr };

    match msg {
        WM_USER_REFRESH => {
            user_data.ops.on_refresh();
            return 0;
        },
        WM_USER_TRAYICON => {
            user_data.ops.on_tray_message(lparam);
            return 0;
        },
        WM_DESTROY => {
            user_data.ops.on_destroy();
            unsafe {
                drop(Box::from_raw(user_data_ptr));
                PostQuitMessage(0);
            }
            return 0;
        },
        _ if msg == taskbar_created_message() => {
            user_data.ops.on_taskbar_created();
            return 0;
        },
        _ => {},
    }

    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

trait WindowOps: Send {
    fn set_hwnd(&mut self, hwnd: HWND);
    fn initial_render(&mut self) -> crate::Result<()>;
    fn on_refresh(&mut self);
    fn on_taskbar_created(&mut self);
    fn on_tray_message(&mut self, lparam: LPARAM);
    fn on_destroy(&mut self);
}

struct WindowUserData {
    ops: Box<dyn WindowOps>,
}

struct WindowState<T: Tray> {
    shared: Arc<Shared<T>>,
    native: NativeState<T::MenuId>,
}

impl<T: Tray> WindowState<T> {
    fn new(shared: Arc<Shared<T>>) -> Self {
        Self {
            shared,
            native: NativeState::new(),
        }
    }

    fn render(&mut self) -> crate::Result<()> {
        let view = {
            let tray = self.shared.lock_tray();
            tray.view()
        };

        self.native.menu = RenderedMenu::from_items(&view.menu);
        self.native.menu_on_primary_click = view.menu_on_primary_click;
        self.native.tooltip = tooltip_text(&view);
        self.native.icon = match view.icon.as_ref() {
            Some(icon) => Some(OwnedIcon::from_icon(icon)?),
            None => None,
        };
        self.native.visible = view.visible;

        // Reference: tray-icon/src/platform_impl/windows/mod.rs::{register_tray_icon,
        // remove_tray_icon}. We use the same Shell_NotifyIconW lifecycle, but feed it
        // from the reactive TrayView snapshot instead of retained icon/menu handles.
        if self.native.visible {
            self.native.sync_tray_icon();
        } else {
            self.native.remove_tray_icon();
        }

        Ok(())
    }

    fn dispatch_event(&mut self, event: TrayEvent<T::MenuId>) {
        {
            let mut tray = self.shared.lock_tray();
            tray.event(event);
        }

        if let Err(error) = self.render() {
            eprintln!("trayinit: refresh after event failed: {error}");
        }
    }

    fn activate_event(&self) -> ActivateEvent {
        ActivateEvent {
            position: cursor_position(),
            area: get_tray_rect(self.native.internal_id, self.native.hwnd).map(rect_from_raw),
        }
    }

    fn show_menu(&mut self) -> bool {
        let Some(menu) = self.native.menu.as_ref() else {
            return false;
        };

        // Reference: muda/src/platform_impl/windows/mod.rs::show_context_menu.
        let command = menu::show_popup_menu(self.native.hwnd, menu.handle());
        if let Some(id) = command.and_then(|command| menu.resolve(command)) {
            self.dispatch_event(TrayEvent::Menu(id));
            true
        } else {
            false
        }
    }
}

impl<T: Tray> WindowOps for WindowState<T> {
    fn set_hwnd(&mut self, hwnd: HWND) {
        self.native.hwnd = hwnd;
        self.shared.hwnd.store(hwnd as isize, Ordering::Release);
        // Reference:
        // tao/src/platform_impl/windows/dark_mode.rs::allow_dark_mode_for_window.
        dark_menu_bar::enable_dark_mode_for_window(hwnd);
        // Reference: tray-icon/src/platform_impl/windows/mod.rs::TrayIcon::new
        // together with
        // muda/src/platform_impl/windows/mod.rs::attach_menu_subclass_for_hwnd.
        menu::attach_window_subclass(hwnd);
    }

    fn initial_render(&mut self) -> crate::Result<()> {
        self.render()
    }

    fn on_refresh(&mut self) {
        if let Err(error) = self.render() {
            eprintln!("trayinit: refresh failed: {error}");
        }
    }

    fn on_taskbar_created(&mut self) {
        // Reference: tray-icon/src/platform_impl/windows/mod.rs::tray_proc taskbar
        // restart branch. Explorer owns the tray area, so we must re-add the icon after
        // it restarts.
        self.native.registered = false;
        if self.native.visible {
            self.native.sync_tray_icon();
        }
    }

    fn on_tray_message(&mut self, lparam: LPARAM) {
        // Reference: tray-icon/src/platform_impl/windows/mod.rs::tray_proc.
        // We keep Windows message dispatch here, but translate it into the crate's
        // higher-level TrayEvent model instead of exposing raw click states publicly.
        match lparam as u32 {
            WM_LBUTTONUP => {
                if self.native.menu_on_primary_click && self.native.menu.is_some() {
                    let _ = self.show_menu();
                } else {
                    self.dispatch_event(TrayEvent::Activate(self.activate_event()));
                }
            },
            WM_RBUTTONUP => {
                if self.native.menu.is_some() {
                    let _ = self.show_menu();
                } else {
                    self.dispatch_event(TrayEvent::SecondaryActivate(self.activate_event()));
                }
            },
            WM_MBUTTONUP => {
                self.dispatch_event(TrayEvent::SecondaryActivate(self.activate_event()));
            },
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_LBUTTONDBLCLK
            | WM_RBUTTONDBLCLK | WM_MBUTTONDBLCLK => {},
            _ => {},
        }
    }

    fn on_destroy(&mut self) {
        menu::detach_window_subclass(self.native.hwnd);
        self.native.remove_tray_icon();
        self.shared.closed.store(true, Ordering::Release);
        self.shared.hwnd.store(0, Ordering::Release);
    }
}

struct Shared<T: Tray> {
    tray: Mutex<T>,
    hwnd: AtomicIsize,
    closed: AtomicBool,
}

impl<T: Tray> Shared<T> {
    fn new(tray: T) -> Self {
        Self {
            tray: Mutex::new(tray),
            hwnd: AtomicIsize::new(0),
            closed: AtomicBool::new(false),
        }
    }

    fn lock_tray(&self) -> MutexGuard<'_, T> {
        match self.tray.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn hwnd(&self) -> Option<HWND> {
        let hwnd = self.hwnd.load(Ordering::Acquire);
        (hwnd != 0).then_some(hwnd as HWND)
    }

    fn post_message(&self, msg: u32) -> core::result::Result<(), ClosedError> {
        let hwnd = self.hwnd().ok_or(ClosedError)?;
        if unsafe { PostMessageW(hwnd, msg, 0, 0) } == 0 {
            return Err(ClosedError);
        }
        Ok(())
    }

    fn post_close(&self) -> core::result::Result<(), ClosedError> {
        let hwnd = self.hwnd().ok_or(ClosedError)?;
        if unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) } == 0 {
            return Err(ClosedError);
        }
        Ok(())
    }
}

struct NativeState<Id> {
    hwnd: HWND,
    internal_id: u32,
    icon: Option<OwnedIcon>,
    menu: Option<RenderedMenu<Id>>,
    tooltip: Option<String>,
    visible: bool,
    registered: bool,
    menu_on_primary_click: bool,
}

// SAFETY: NativeState is only ever accessed on the dedicated Windows backend
// thread.
unsafe impl<Id: Send> Send for NativeState<Id> {}

impl<Id> NativeState<Id> {
    fn new() -> Self {
        Self {
            hwnd: ptr::null_mut(),
            internal_id: NEXT_INTERNAL_ID.fetch_add(1, Ordering::Relaxed),
            icon: None,
            menu: None,
            tooltip: None,
            visible: true,
            registered: false,
            menu_on_primary_click: false,
        }
    }

    fn sync_tray_icon(&mut self) {
        let icon_handle = self.icon.as_ref().map(|icon| icon.handle());
        let tooltip = self.tooltip.as_deref();

        let updated = if self.registered {
            modify_tray_icon(self.hwnd, self.internal_id, icon_handle, tooltip)
        } else {
            register_tray_icon(self.hwnd, self.internal_id, icon_handle, tooltip)
        };

        self.registered = updated;
    }

    fn remove_tray_icon(&mut self) {
        if self.registered {
            remove_tray_icon(self.hwnd, self.internal_id);
            self.registered = false;
        }
    }
}

fn taskbar_created_message() -> u32 {
    *TASKBAR_CREATED.get_or_init(|| unsafe { RegisterWindowMessageA(b"TaskbarCreated\0".as_ptr()) })
}

fn cursor_position() -> Option<PhysicalPosition> {
    let mut point = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut point) } == TRUE).then_some(PhysicalPosition::new(point.x, point.y))
}

fn rect_from_raw(rect: RECT) -> Rect {
    Rect::new(
        PhysicalPosition::new(rect.left, rect.top),
        PhysicalSize::new(
            rect.right.saturating_sub(rect.left) as u32,
            rect.bottom.saturating_sub(rect.top) as u32,
        ),
    )
}

fn tooltip_text<Id>(view: &TrayView<Id>) -> Option<String> {
    if let Some(tooltip) = &view.tooltip {
        match (tooltip.title.trim(), tooltip.body.trim()) {
            ("", "") => None,
            (title, "") => Some(title.to_string()),
            ("", body) => Some(body.to_string()),
            (title, body) => Some(format!("{title}: {body}")),
        }
    } else {
        view.title.clone()
    }
}

fn register_tray_icon(
    hwnd: HWND,
    tray_id: u32,
    hicon: Option<windows_sys::Win32::UI::WindowsAndMessaging::HICON>,
    tooltip: Option<&str>,
) -> bool {
    // Reference: tray-icon/src/platform_impl/windows/mod.rs::register_tray_icon.
    let mut icon_data = notify_icon_data(hwnd, tray_id, hicon, tooltip);
    unsafe { Shell_NotifyIconW(NIM_ADD, &mut icon_data) == TRUE }
}

fn modify_tray_icon(
    hwnd: HWND,
    tray_id: u32,
    hicon: Option<windows_sys::Win32::UI::WindowsAndMessaging::HICON>,
    tooltip: Option<&str>,
) -> bool {
    let mut icon_data = notify_icon_data(hwnd, tray_id, hicon, tooltip);
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &mut icon_data) == TRUE }
}

fn remove_tray_icon(hwnd: HWND, tray_id: u32) {
    // Reference: tray-icon/src/platform_impl/windows/mod.rs::remove_tray_icon.
    let mut icon_data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: tray_id,
        ..unsafe { std::mem::zeroed() }
    };

    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &mut icon_data);
    }
}

fn notify_icon_data(
    hwnd: HWND,
    tray_id: u32,
    hicon: Option<windows_sys::Win32::UI::WindowsAndMessaging::HICON>,
    tooltip: Option<&str>,
) -> NOTIFYICONDATAW {
    let mut flags = NIF_MESSAGE;
    let mut tip: [u16; 128] = [0; 128];
    let mut icon_handle = ptr::null_mut();

    if let Some(hicon) = hicon {
        flags |= NIF_ICON;
        icon_handle = hicon;
    }

    if let Some(tooltip) = tooltip {
        flags |= NIF_TIP;
        let encoded = util::encode_wide(tooltip);
        let copy_len = encoded.len().min(tip.len());
        tip[..copy_len].copy_from_slice(&encoded[..copy_len]);
    }

    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: tray_id,
        uFlags: flags,
        uCallbackMessage: WM_USER_TRAYICON,
        hIcon: icon_handle,
        szTip: tip,
        ..unsafe { std::mem::zeroed() }
    }
}

fn get_tray_rect(id: u32, hwnd: HWND) -> Option<RECT> {
    // Reference: tray-icon/src/platform_impl/windows/mod.rs::get_tray_rect.
    let identifier = NOTIFYICONIDENTIFIER {
        cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: hwnd,
        uID: id,
        ..unsafe { std::mem::zeroed() }
    };

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    (unsafe { Shell_NotifyIconGetRect(&identifier, &mut rect) } == 0).then_some(rect)
}
