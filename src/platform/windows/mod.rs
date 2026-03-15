mod accelerator;
mod dark_menu_bar;
mod icon;
mod menu;
mod util;

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, mpsc};
use std::{fmt, io, ptr, thread};

use dpi::{PhysicalPosition, PhysicalSize};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, TRUE, WPARAM};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_SELECT,
    NINF_KEY, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CW_USEDEFAULT, ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW,
    DestroyWindow, DispatchMessageW, GWL_USERDATA, GetCursorPos, GetMessageW, MSG, MSGFLT_ALLOW,
    PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageA, TranslateMessage,
    WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_NCCREATE, WM_RBUTTONDBLCLK,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT, WS_OVERLAPPED,
};

use self::icon::OwnedIcon;
use self::menu::RenderedMenu;
use crate::model::{MenuDiff, NormalizedMenuItem, NormalizedTrayView, diff_menu_items};
use crate::{
    Builder, ClosedError, Error, Handle, InteractionEvent, InteractionKind, Result,
    RuntimePreference, Tray, TrayEvent,
};

const WM_USER_TRAYICON: u32 = 6002;
const WM_USER_REFRESH: u32 = 6003;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

static NEXT_INTERNAL_ID: AtomicU32 = AtomicU32::new(1);
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

pub struct PlatformHandle<T: Tray> {
    shared: Arc<Shared<T>>,
}

impl<T: Tray> Clone for PlatformHandle<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T: Tray> PlatformHandle<T> {
    fn new(shared: Arc<Shared<T>>) -> Self {
        Self { shared }
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, ClosedError> {
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

    pub fn refresh(&self) -> Result<(), ClosedError> {
        self.shared.post_message(WM_USER_REFRESH)
    }

    pub fn shutdown(&self) -> Result<()> {
        if self.is_closed() {
            return Ok(());
        }

        self.shared.post_close().map_err(Error::from)
    }

    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
    }

    pub unsafe fn process_windows_message(
        &self,
        msg: *const windows_sys::Win32::UI::WindowsAndMessaging::MSG,
    ) -> bool {
        self.shared.process_windows_message(msg)
    }

    pub unsafe fn register_accelerator_window(&self, hwnd: HWND) -> Result<(), ClosedError> {
        self.shared.register_accelerator_window(hwnd)
    }

    pub unsafe fn unregister_accelerator_window(&self, hwnd: HWND) -> Result<(), ClosedError> {
        self.shared.unregister_accelerator_window(hwnd)
    }
}

impl<T: Tray> fmt::Debug for PlatformHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlatformHandle")
            .field("closed", &self.is_closed())
            .finish()
    }
}

pub fn spawn<T: Tray>(builder: Builder<T>) -> Result<Handle<T>>
where
    T::Message: Clone,
{
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
    #[cfg(feature = "tracing")]
    tracing::debug!(tray_id = %tray_id, "Starting Windows tray backend in spawn mode");
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

pub fn attach<T: Tray>(builder: Builder<T>) -> Result<Handle<T>>
where
    T::Message: Clone,
{
    if matches!(
        builder.runtime_preference_ref(),
        RuntimePreference::DedicatedThread
    ) {
        return spawn(builder);
    }

    let Builder {
        tray,
        runtime_preference: _,
        linux: _,
    } = builder;

    let tray_id = tray.id().to_string();
    #[cfg(feature = "tracing")]
    tracing::debug!(tray_id = %tray_id, "Starting Windows tray backend in attach mode");
    let shared = Arc::new(Shared::new(tray));
    initialize_backend::<T>(Arc::clone(&shared), false)?;

    Ok(Handle::new(tray_id, PlatformHandle::new(shared)))
}

pub fn run<T: Tray>(builder: Builder<T>) -> Result<()>
where
    T::Message: Clone,
{
    let Builder {
        tray,
        runtime_preference,
        linux: _,
    } = builder;

    if matches!(runtime_preference, RuntimePreference::DedicatedThread) {
        return Err(Error::Unsupported(
            "dedicated-thread Windows run() is not supported",
        ));
    }

    #[cfg(feature = "tracing")]
    tracing::debug!(tray_id = %tray.id(), "Starting Windows tray backend in run mode");
    let shared = Arc::new(Shared::new(tray));
    initialize_backend::<T>(Arc::clone(&shared), true)?;
    message_loop(&shared);
    Ok(())
}

fn backend_thread<T: Tray>(shared: Arc<Shared<T>>, init_tx: mpsc::SyncSender<Result<()>>)
where
    T::Message: Clone,
{
    match initialize_backend(Arc::clone(&shared), true) {
        Ok(()) => {
            let _ = init_tx.send(Ok(()));
            message_loop(&shared);
        },
        Err(error) => {
            let _ = init_tx.send(Err(error));
        },
    }
}

fn initialize_backend<T: Tray>(shared: Arc<Shared<T>>, owns_message_loop: bool) -> Result<()>
where
    T::Message: Clone,
{
    // Reference: winit/src/platform_impl/windows/dpi.rs::become_dpi_aware.
    util::become_dpi_aware();
    // Reference:
    // tao/src/platform_impl/windows/dark_mode.rs::allow_dark_mode_for_app.
    dark_menu_bar::enable_dark_mode_for_app();

    let class_name = util::encode_wide("trayinit_hidden_window");
    register_window_class(&class_name)?;

    let user_data = Box::new(WindowUserData {
        ops: Box::new(WindowState::<T>::new(
            Arc::clone(&shared),
            owns_message_loop,
        )),
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

fn register_window_class(class_name: &[u16]) -> Result<()> {
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

fn message_loop<T: Tray>(shared: &Shared<T>) {
    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        match result {
            -1 => break,
            0 => break,
            _ => {
                if shared.process_windows_message(&message) {
                    continue;
                }

                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
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
            user_data.ops.on_tray_message(wparam, lparam);
            return 0;
        },
        WM_COMMAND => {
            user_data.ops.on_command((wparam as u32) & 0xffff);
            return 0;
        },
        WM_DESTROY => {
            let should_post_quit = user_data.ops.on_destroy();
            unsafe {
                drop(Box::from_raw(user_data_ptr));
                if should_post_quit {
                    PostQuitMessage(0);
                }
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
    fn initial_render(&mut self) -> Result<()>;
    fn on_refresh(&mut self);
    fn on_taskbar_created(&mut self);
    fn on_tray_message(&mut self, wparam: WPARAM, lparam: LPARAM);
    fn on_command(&mut self, command: u32);
    fn on_destroy(&mut self) -> bool;
}

struct WindowUserData {
    ops: Box<dyn WindowOps>,
}

struct WindowState<T: Tray> {
    shared: Arc<Shared<T>>,
    native: NativeState<T::Message>,
    owns_message_loop: bool,
}

impl<T: Tray> WindowState<T>
where
    T::Message: Clone,
{
    fn new(shared: Arc<Shared<T>>, owns_message_loop: bool) -> Self {
        Self {
            shared,
            native: NativeState::new(),
            owns_message_loop,
        }
    }

    fn render(&mut self) -> Result<()> {
        let view = {
            let tray = self.shared.lock_tray();
            NormalizedTrayView::from_tray(&*tray)
        };

        let previous_view = self.native.view.as_ref();
        let icon_changed = previous_view.is_none_or(|old| old.icon != view.icon);
        let tooltip_changed = previous_view.is_none_or(|old| old.tooltip != view.tooltip);
        let visible_changed = previous_view.is_none_or(|old| old.visible != view.visible);
        let menu_on_primary_click_changed =
            previous_view.is_none_or(|old| old.menu_on_primary_click != view.menu_on_primary_click);
        let menu_diff = match self.native.menu_view.as_ref() {
            Some(menu) => diff_menu_items(menu, &view.menu),
            None => MenuDiff::Rebuild,
        };

        self.native.menu_on_primary_click = view.menu_on_primary_click;
        self.native.tooltip = view.tooltip.clone();

        if icon_changed {
            self.native.icon = match view.icon.as_ref() {
                Some(icon) => Some(OwnedIcon::from_icon(icon)?),
                None => None,
            };
        }

        if self.native.menu_is_open {
            if !matches!(menu_diff, MenuDiff::None) {
                self.native.menu_refresh_pending = true;
            } else if let Some(menu) = self.native.menu.as_mut() {
                if menu.sync_bindings(&view.menu)? {
                    self.native.menu_view = Some(view.menu.clone());
                } else {
                    self.native.menu_refresh_pending = true;
                }
            } else {
                self.native.menu_view = Some(view.menu.clone());
            }
        } else {
            self.native.menu_refresh_pending = false;
            match &menu_diff {
                MenuDiff::Rebuild => {
                    self.native.menu = RenderedMenu::from_model(&view.menu)?;
                },
                MenuDiff::None => {},
                MenuDiff::Patch(patches) => {
                    let mut applied = false;
                    if let Some(menu) = self.native.menu.as_mut() {
                        applied = menu.apply_patches(patches);
                    }

                    if !applied {
                        self.native.menu = RenderedMenu::from_model(&view.menu)?;
                    }
                },
            }

            let sync_ok = match self.native.menu.as_mut() {
                Some(menu) => menu.sync_bindings(&view.menu)?,
                None => view.menu.is_empty(),
            };

            if !sync_ok {
                self.native.menu = RenderedMenu::from_model(&view.menu)?;
            }

            self.native.menu_view = Some(view.menu.clone());
        }

        self.shared.set_haccel(self.native.accelerator_handle());

        self.native.visible = view.visible;
        self.native.view = Some(view);

        // Reference: tray-icon/src/platform_impl/windows/mod.rs::{register_tray_icon,
        // remove_tray_icon}. We use the same Shell_NotifyIconW lifecycle, but feed it
        // from the reactive TrayView snapshot instead of retained icon/menu handles.
        if self.native.visible {
            if !self.native.registered
                || icon_changed
                || tooltip_changed
                || visible_changed
                || menu_on_primary_click_changed
            {
                self.native.sync_tray_icon();
            }
        } else {
            self.native.remove_tray_icon();
        }

        Ok(())
    }

    fn dispatch_event(&mut self, event: TrayEvent<T::Message>) {
        {
            let mut tray = self.shared.lock_tray();
            tray.event(event);
        }

        if let Err(error) = self.render() {
            eprintln!("trayinit: refresh after event failed: {error}");
        }
        self.maybe_request_shutdown();
    }

    fn interaction_event(
        &self,
        kind: InteractionKind,
        position: Option<PhysicalPosition<i32>>,
    ) -> InteractionEvent {
        InteractionEvent {
            kind,
            position: position.or_else(cursor_position),
            area: get_tray_rect(self.native.internal_id, self.native.hwnd).map(rect_from_raw),
        }
    }

    fn show_menu(&mut self, anchor: Option<PhysicalPosition<i32>>) -> bool {
        let Some(menu) = self.native.menu.as_ref() else {
            return false;
        };

        // Reference: muda/src/platform_impl/windows/mod.rs::show_context_menu.
        self.native.menu_is_open = true;
        let command = menu::show_popup_menu(
            self.native.hwnd,
            menu.handle(),
            anchor.map(point_from_position),
        );
        self.native.menu_is_open = false;

        let selected_id = command.and_then(|command| {
            self.native
                .menu
                .as_ref()
                .and_then(|menu| menu.resolve(command))
        });

        if self.native.menu_refresh_pending {
            if let Err(error) = self.render() {
                eprintln!("trayinit: deferred menu refresh failed: {error}");
            }
        }

        if let Some(id) = selected_id {
            self.dispatch_event(TrayEvent::Menu(id));
            true
        } else {
            false
        }
    }

    fn maybe_request_shutdown(&self) {
        if self.shared.should_exit() {
            let _ = self.shared.post_close();
        }
    }
}

impl<T: Tray> WindowOps for WindowState<T>
where
    T::Message: Clone,
{
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

    fn initial_render(&mut self) -> Result<()> {
        self.render()?;
        self.maybe_request_shutdown();
        Ok(())
    }

    fn on_refresh(&mut self) {
        if let Err(error) = self.render() {
            eprintln!("trayinit: refresh failed: {error}");
        }
        self.maybe_request_shutdown();
    }

    fn on_taskbar_created(&mut self) {
        // Reference: tray-icon/src/platform_impl/windows/mod.rs::tray_proc taskbar
        // restart branch. Explorer owns the tray area, so we must re-add the icon after
        // it restarts.
        self.native.registered = false;
        self.native.uses_notifyicon_v4 = false;
        self.native.pending_shell_followup = None;
        if self.native.visible {
            self.native.sync_tray_icon();
        }
    }

    fn on_tray_message(&mut self, wparam: WPARAM, lparam: LPARAM) {
        // Reference: tray-icon/src/platform_impl/windows/mod.rs::tray_proc.
        // Windows already distinguishes left/right/middle button callbacks and
        // double-clicks for notification icons. We preserve that as trigger
        // detail while still emitting semantic interaction events.
        let notification = notification_code(lparam);
        let position = notification_anchor_position(self.native.uses_notifyicon_v4, wparam);
        let pending_followup = self.native.pending_shell_followup.take();

        match notification {
            NIN_KEYSELECT => {
                if pending_followup != Some(ShellFollowup::PrimaryActivate) {
                    if self.native.menu_on_primary_click && self.native.menu.is_some() {
                        let _ = self.show_menu(position);
                    } else {
                        self.dispatch_event(TrayEvent::Interaction(
                            self.interaction_event(InteractionKind::PrimaryActivate, position),
                        ));
                    }
                }
            },
            NIN_SELECT => {
                if pending_followup != Some(ShellFollowup::PrimaryActivate) {
                    self.dispatch_event(TrayEvent::Interaction(
                        self.interaction_event(InteractionKind::PrimaryActivate, position),
                    ));
                }
            },
            WM_CONTEXTMENU => {
                if pending_followup != Some(ShellFollowup::ContextMenu) {
                    if self.native.menu.is_some() {
                        let _ = self.show_menu(position);
                    } else {
                        self.dispatch_event(TrayEvent::Interaction(
                            self.interaction_event(InteractionKind::ContextMenu, position),
                        ));
                    }
                }
            },
            WM_LBUTTONUP => {
                // On some Explorer paths, a primary activation arrives first as
                // WM_LBUTTONUP and is then followed by NIN_SELECT. Record the
                // already-emitted semantic event so the shell follow-up can be
                // suppressed instead of producing a duplicate.
                self.native.pending_shell_followup = Some(ShellFollowup::PrimaryActivate);
                if self.native.menu_on_primary_click && self.native.menu.is_some() {
                    let _ = self.show_menu(position);
                } else {
                    self.dispatch_event(TrayEvent::Interaction(
                        self.interaction_event(InteractionKind::PrimaryActivate, position),
                    ));
                }
            },
            WM_RBUTTONUP => {
                // Explorer can likewise follow WM_RBUTTONUP with WM_CONTEXTMENU
                // for the same logical gesture.
                self.native.pending_shell_followup = Some(ShellFollowup::ContextMenu);
                if self.native.menu.is_some() {
                    let _ = self.show_menu(position);
                } else {
                    self.dispatch_event(TrayEvent::Interaction(
                        self.interaction_event(InteractionKind::ContextMenu, position),
                    ));
                }
            },
            WM_MBUTTONUP => {
                self.dispatch_event(TrayEvent::Interaction(
                    self.interaction_event(InteractionKind::SecondaryActivate, position),
                ));
            },
            // Windows delivers double-click messages in addition to the normal
            // click sequence. We keep the richer public type, but do not emit
            // semantic double-click interactions until the backend can suppress
            // duplicate single-click activation correctly.
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_LBUTTONDBLCLK
            | WM_RBUTTONDBLCLK | WM_MBUTTONDBLCLK => {
                self.native.pending_shell_followup = None;
            },
            _ => {
                self.native.pending_shell_followup = None;
            },
        }
    }

    fn on_command(&mut self, command: u32) {
        let selected_id = self
            .native
            .menu
            .as_ref()
            .and_then(|menu| menu.resolve(command));

        if let Some(id) = selected_id {
            self.dispatch_event(TrayEvent::Menu(id));
        }
    }

    fn on_destroy(&mut self) -> bool {
        menu::detach_window_subclass(self.native.hwnd);
        self.native.remove_tray_icon();
        self.shared.closed.store(true, Ordering::Release);
        self.shared.hwnd.store(0, Ordering::Release);
        self.shared.set_haccel(ptr::null_mut());
        self.shared.clear_accelerator_windows();
        self.owns_message_loop
    }
}

struct Shared<T: Tray> {
    tray: Mutex<T>,
    hwnd: AtomicIsize,
    haccel: AtomicIsize,
    accelerator_windows: Mutex<Vec<isize>>,
    closed: AtomicBool,
}

impl<T: Tray> Shared<T> {
    fn new(tray: T) -> Self {
        Self {
            tray: Mutex::new(tray),
            hwnd: AtomicIsize::new(0),
            haccel: AtomicIsize::new(0),
            accelerator_windows: Mutex::new(Vec::new()),
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

    fn haccel(&self) -> Option<windows_sys::Win32::UI::WindowsAndMessaging::HACCEL> {
        let haccel = self.haccel.load(Ordering::Acquire);
        (haccel != 0).then_some(haccel as _)
    }

    fn set_haccel(&self, haccel: windows_sys::Win32::UI::WindowsAndMessaging::HACCEL) {
        self.haccel.store(haccel as isize, Ordering::Release);
    }

    fn lock_accelerator_windows(&self) -> MutexGuard<'_, Vec<isize>> {
        match self.accelerator_windows.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn register_accelerator_window(&self, hwnd: HWND) -> Result<(), ClosedError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(ClosedError);
        }

        let hwnd = hwnd as isize;
        let mut windows = self.lock_accelerator_windows();
        if !windows.contains(&hwnd) {
            windows.push(hwnd);
        }
        Ok(())
    }

    fn unregister_accelerator_window(&self, hwnd: HWND) -> Result<(), ClosedError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(ClosedError);
        }

        let hwnd = hwnd as isize;
        let mut windows = self.lock_accelerator_windows();
        windows.retain(|registered| *registered != hwnd);
        Ok(())
    }

    fn clear_accelerator_windows(&self) {
        self.lock_accelerator_windows().clear();
    }

    fn should_exit(&self) -> bool {
        let tray = self.lock_tray();
        tray.should_exit()
    }

    fn post_message(&self, msg: u32) -> Result<(), ClosedError> {
        let hwnd = self.hwnd().ok_or(ClosedError)?;
        if unsafe { PostMessageW(hwnd, msg, 0, 0) } == 0 {
            return Err(ClosedError);
        }
        Ok(())
    }

    fn post_close(&self) -> Result<(), ClosedError> {
        let hwnd = self.hwnd().ok_or(ClosedError)?;
        if unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) } == 0 {
            return Err(ClosedError);
        }
        Ok(())
    }

    fn process_windows_message(&self, msg: *const MSG) -> bool {
        let Some(tray_hwnd) = self.hwnd() else {
            return false;
        };
        let Some(haccel) = self.haccel() else {
            return false;
        };
        if msg.is_null() {
            return false;
        }

        let source_hwnd = unsafe { (*msg).hwnd as isize };
        if !self.lock_accelerator_windows().contains(&source_hwnd) {
            return false;
        }

        unsafe { menu::process_accelerator_message(tray_hwnd, haccel, msg) }
    }
}

struct NativeState<Id> {
    hwnd: HWND,
    internal_id: u32,
    view: Option<NormalizedTrayView<Id>>,
    menu_view: Option<Vec<NormalizedMenuItem<Id>>>,
    icon: Option<OwnedIcon>,
    menu: Option<RenderedMenu<Id>>,
    menu_is_open: bool,
    menu_refresh_pending: bool,
    tooltip: Option<String>,
    visible: bool,
    registered: bool,
    menu_on_primary_click: bool,
    uses_notifyicon_v4: bool,
    // Explorer sometimes reports one logical tray interaction twice: first as a
    // mouse-style callback, then as a higher-level shell semantic notification.
    // Keep only enough state to suppress that second notification.
    pending_shell_followup: Option<ShellFollowup>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellFollowup {
    PrimaryActivate,
    ContextMenu,
}

// SAFETY: NativeState is only ever accessed on the dedicated Windows backend
// thread.
unsafe impl<Id> Send for NativeState<Id> {}

impl<Id: Clone> NativeState<Id> {
    fn new() -> Self {
        Self {
            hwnd: ptr::null_mut(),
            internal_id: NEXT_INTERNAL_ID.fetch_add(1, Ordering::Relaxed),
            view: None,
            menu_view: None,
            icon: None,
            menu: None,
            menu_is_open: false,
            menu_refresh_pending: false,
            tooltip: None,
            visible: true,
            registered: false,
            menu_on_primary_click: false,
            uses_notifyicon_v4: false,
            pending_shell_followup: None,
        }
    }

    fn sync_tray_icon(&mut self) {
        let icon_handle = self.icon.as_ref().map(|icon| icon.handle());
        let tooltip = self.tooltip.as_deref();

        let updated = if self.registered {
            modify_tray_icon(self.hwnd, self.internal_id, icon_handle, tooltip)
        } else {
            let (registered, uses_notifyicon_v4) =
                register_tray_icon(self.hwnd, self.internal_id, icon_handle, tooltip);
            self.uses_notifyicon_v4 = uses_notifyicon_v4;
            registered
        };

        self.registered = updated;
    }

    fn remove_tray_icon(&mut self) {
        if self.registered {
            remove_tray_icon(self.hwnd, self.internal_id);
            self.registered = false;
        }
        self.uses_notifyicon_v4 = false;
        self.pending_shell_followup = None;
    }

    fn accelerator_handle(&self) -> windows_sys::Win32::UI::WindowsAndMessaging::HACCEL {
        self.menu
            .as_ref()
            .map_or(ptr::null_mut(), RenderedMenu::accelerator_handle)
    }
}

fn taskbar_created_message() -> u32 {
    *TASKBAR_CREATED.get_or_init(|| unsafe { RegisterWindowMessageA(b"TaskbarCreated\0".as_ptr()) })
}

fn cursor_position() -> Option<PhysicalPosition<i32>> {
    let mut point = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut point) } == TRUE).then_some(PhysicalPosition::new(point.x, point.y))
}

fn rect_from_raw(rect: RECT) -> (PhysicalPosition<i32>, PhysicalSize<i32>) {
    (
        PhysicalPosition::new(rect.left, rect.top),
        PhysicalSize::new(
            rect.right.saturating_sub(rect.left),
            rect.bottom.saturating_sub(rect.top),
        ),
    )
}

fn register_tray_icon(
    hwnd: HWND,
    tray_id: u32,
    hicon: Option<windows_sys::Win32::UI::WindowsAndMessaging::HICON>,
    tooltip: Option<&str>,
) -> (bool, bool) {
    // Reference: tray-icon/src/platform_impl/windows/mod.rs::register_tray_icon.
    let mut icon_data = notify_icon_data(hwnd, tray_id, hicon, tooltip);
    let added = unsafe { Shell_NotifyIconW(NIM_ADD, &mut icon_data) == TRUE };
    if !added {
        return (false, false);
    }

    // Reference: Shell_NotifyIconW documentation for NIM_SETVERSION and
    // NOTIFYICON_VERSION_4 callback semantics.
    let uses_notifyicon_v4 = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &mut icon_data) == TRUE };
    (true, uses_notifyicon_v4)
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
        Anonymous: windows_sys::Win32::UI::Shell::NOTIFYICONDATAW_0 {
            uVersion: NOTIFYICON_VERSION_4,
        },
        ..unsafe { std::mem::zeroed() }
    }
}

fn notification_code(lparam: LPARAM) -> u32 {
    (lparam as u32) & 0xffff
}

fn notification_anchor_position(
    uses_notifyicon_v4: bool,
    wparam: WPARAM,
) -> Option<PhysicalPosition<i32>> {
    if !uses_notifyicon_v4 {
        return None;
    }

    Some(PhysicalPosition::new(
        signed_word((wparam as u32) & 0xffff),
        signed_word(((wparam as u32) >> 16) & 0xffff),
    ))
}

fn signed_word(value: u32) -> i32 {
    (value as i16) as i32
}

fn point_from_position(position: PhysicalPosition<i32>) -> POINT {
    POINT {
        x: position.x,
        y: position.y,
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
