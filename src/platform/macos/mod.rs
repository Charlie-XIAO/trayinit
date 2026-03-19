mod accelerator;
mod icon;
mod util;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::{fmt, ptr, thread_local};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{AnyThread, DeclaredClass, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSCellImagePosition, NSControlStateValueOff,
    NSControlStateValueOn, NSEvent, NSEventModifierFlags, NSEventType, NSMenu, NSMenuItem,
    NSStatusBar, NSStatusItem, NSTrackingArea, NSTrackingAreaOptions, NSVariableStatusItemLength,
    NSView, NSWindow,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGDisplayPixelsHigh, CGMainDisplayID};
use objc2_foundation::{MainThreadMarker, NSObject, NSPoint, NSString};

use self::accelerator::{key_equivalent, modifier_mask};
use self::icon::to_nsimage;
use self::util::strip_mnemonic;
use crate::menu::Accelerator;
use crate::model::{CommandState, NormalizedCommandItem, NormalizedMenuItem, NormalizedTrayView};
use crate::tray::{Builder, InteractionEvent, InteractionKind, RuntimePreference};
use crate::{ClosedError, Error, Handle, Icon, Result, Tray, TrayEvent};

pub struct PlatformHandle<T: Tray> {
    shared: Weak<Shared<T>>,
}

impl<T: Tray> Clone for PlatformHandle<T> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<T: Tray> fmt::Debug for PlatformHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlatformHandle")
            .field("closed", &self.is_closed())
            .finish()
    }
}

impl<T: Tray> PlatformHandle<T> {
    fn new(shared: &Rc<Shared<T>>) -> Self {
        Self {
            shared: Rc::downgrade(shared),
        }
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> core::result::Result<R, ClosedError>
    where
        T::Message: Clone,
    {
        let Some(shared) = self.shared.upgrade() else {
            return Err(ClosedError);
        };
        if shared.closed.get() {
            return Err(ClosedError);
        }

        let result = {
            let mut state = shared.state.borrow_mut();
            f(&mut state.tray)
        };

        shared.render_or_log();
        shared.maybe_shutdown();
        Ok(result)
    }

    pub fn refresh(&self) -> core::result::Result<(), ClosedError>
    where
        T::Message: Clone,
    {
        let Some(shared) = self.shared.upgrade() else {
            return Err(ClosedError);
        };
        if shared.closed.get() {
            return Err(ClosedError);
        }

        shared.render_or_log();
        shared.maybe_shutdown();
        Ok(())
    }

    pub fn shutdown(&self) -> Result<()> {
        let Some(shared) = self.shared.upgrade() else {
            return Ok(());
        };
        shared.shutdown();
        Ok(())
    }

    pub fn is_closed(&self) -> bool {
        self.shared
            .upgrade()
            .is_none_or(|shared| shared.closed.get())
    }
}

pub fn spawn<T: Tray>(builder: Builder<T>) -> Result<Handle<T>>
where
    T::Message: Clone,
{
    let _ = builder;
    Err(Error::Unsupported(
        "macOS tray runtime currently only supports attach() on the main thread",
    ))
}

pub fn attach<T: Tray>(builder: Builder<T>) -> Result<Handle<T>>
where
    T::Message: Clone,
{
    if matches!(
        builder.runtime_preference_ref(),
        RuntimePreference::DedicatedThread
    ) {
        return Err(Error::Unsupported(
            "macOS tray runtime requires the main thread; dedicated-thread attach() is unsupported",
        ));
    }

    let Builder {
        tray,
        runtime_preference: _,
        linux: _,
    } = builder;

    let shared = Shared::new(tray, false)?;
    let tray_id = shared.state.borrow().tray_id.clone();
    shared.clone().register();
    shared.maybe_shutdown();

    Ok(Handle::new(tray_id, PlatformHandle::new(&shared)))
}

pub fn run<T: Tray>(builder: Builder<T>) -> Result<()>
where
    T::Message: Clone,
{
    if matches!(
        builder.runtime_preference_ref(),
        RuntimePreference::DedicatedThread
    ) {
        return Err(Error::Unsupported(
            "macOS run() requires the main thread; dedicated-thread run() is unsupported",
        ));
    }

    let Builder {
        tray,
        runtime_preference: _,
        linux: _,
    } = builder;

    let mtm = MainThreadMarker::new().ok_or(Error::Unsupported(
        "macOS tray runtime must run on the main thread",
    ))?;
    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();

    let shared = Shared::new(tray, true)?;
    shared.clone().register();
    shared.maybe_shutdown();
    if shared.closed.get() {
        return Ok(());
    }

    app.run();
    shared.shutdown();
    Ok(())
}

trait RuntimeOps {
    fn on_menu_event(&self, menu_id: &str);
    fn on_interaction(&self, event: InteractionEvent);
    fn on_menu_tracking_changed(&self, open: bool);
}

#[derive(Default)]
struct Registry {
    trays: HashMap<String, Rc<dyn RuntimeOps>>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
}

fn with_runtime<R>(tray_id: &str, f: impl FnOnce(&Rc<dyn RuntimeOps>) -> R) -> Option<R> {
    let runtime = REGISTRY.with(|registry| registry.borrow().trays.get(tray_id).cloned());
    runtime.as_ref().map(f)
}

fn dispatch_menu_action(tray_id: &str, menu_id: &str) {
    let _ = with_runtime(tray_id, |runtime| runtime.on_menu_event(menu_id));
}

fn dispatch_interaction(tray_id: &str, event: InteractionEvent) {
    let _ = with_runtime(tray_id, |runtime| runtime.on_interaction(event));
}

fn dispatch_menu_tracking(tray_id: &str, open: bool) {
    let _ = with_runtime(tray_id, |runtime| runtime.on_menu_tracking_changed(open));
}

struct Shared<T: Tray> {
    closed: Cell<bool>,
    owns_app_loop: bool,
    menu_tracking: Cell<bool>,
    pending_render: Cell<bool>,
    state: RefCell<State<T>>,
}

struct State<T: Tray> {
    tray_id: String,
    tray: T,
    native: NativeTray,
    menu_messages: HashMap<String, T::Message>,
    menu_model: Vec<NormalizedMenuItem<T::Message>>,
    has_menu: bool,
}

impl<T: Tray> Shared<T> {
    fn register(self: Rc<Self>)
    where
        T::Message: Clone,
    {
        let tray_id = self.state.borrow().tray_id.clone();
        let runtime: Rc<dyn RuntimeOps> = self;
        REGISTRY.with(|registry| {
            registry.borrow_mut().trays.insert(tray_id, runtime);
        });
    }

    fn maybe_shutdown(&self) {
        if self.closed.get() {
            return;
        }

        if self.state.borrow().tray.should_exit() {
            self.shutdown();
        }
    }

    fn shutdown(&self) {
        if self.closed.replace(true) {
            return;
        }

        let tray_id = {
            let mut state = self.state.borrow_mut();
            state.menu_messages.clear();
            state.has_menu = false;
            let tray_id = state.tray_id.clone();
            state.native.remove();
            tray_id
        };

        REGISTRY.with(|registry| {
            registry.borrow_mut().trays.remove(&tray_id);
        });

        if self.owns_app_loop {
            stop_app_loop();
        }
    }
}

impl<T: Tray> Shared<T>
where
    T::Message: Clone,
{
    fn new(tray: T, owns_app_loop: bool) -> Result<Rc<Self>> {
        MainThreadMarker::new().ok_or(Error::Unsupported(
            "macOS tray objects must be created and updated on the main thread",
        ))?;

        let tray_id = tray.id().to_string();
        let view = NormalizedTrayView::from_tray(&tray);
        let menu_tree = if view.menu.is_empty() {
            None
        } else {
            Some(NativeMenuTree::new(&tray_id, &view.menu)?)
        };

        let native = NativeTray::new(&tray_id, &view, menu_tree)?;
        let menu_messages = collect_menu_messages(&tray_id, &view.menu);
        let has_menu = !view.menu.is_empty();

        Ok(Rc::new(Self {
            closed: Cell::new(false),
            owns_app_loop,
            menu_tracking: Cell::new(false),
            pending_render: Cell::new(false),
            state: RefCell::new(State {
                tray_id,
                tray,
                native,
                menu_messages,
                menu_model: view.menu,
                has_menu,
            }),
        }))
    }

    fn render(&self) -> Result<()> {
        if self.closed.get() {
            return Ok(());
        }

        let mut state = self.state.borrow_mut();
        let view = NormalizedTrayView::from_tray(&state.tray);

        let menu_changed = menu_visuals_differ(&state.menu_model, &view.menu);
        let menu_tree = if menu_changed {
            if view.menu.is_empty() {
                None
            } else {
                Some(NativeMenuTree::new(&state.tray_id, &view.menu)?)
            }
        } else {
            None
        };

        let defer_menu_update = self.menu_tracking.get() && menu_changed;
        if defer_menu_update {
            self.pending_render.set(true);
        }

        state
            .native
            .apply_view(&view, menu_changed && !defer_menu_update, menu_tree)?;

        if defer_menu_update {
            return Ok(());
        }

        state.menu_messages = collect_menu_messages(&state.tray_id, &view.menu);
        state.menu_model = view.menu;
        state.has_menu = !state.menu_model.is_empty();

        Ok(())
    }

    fn render_or_log(&self) {
        if let Err(error) = self.render() {
            #[cfg(feature = "tracing")]
            tracing::error!(?error, "macOS tray render failed");
            #[cfg(not(feature = "tracing"))]
            let _ = error;
        }
    }

    fn dispatch_tray_event(&self, event: TrayEvent<T::Message>) {
        {
            let mut state = self.state.borrow_mut();
            state.tray.event(event);
        }
        self.render_or_log();
        self.maybe_shutdown();
    }
}

impl<T: Tray> RuntimeOps for Shared<T>
where
    T::Message: Clone,
{
    fn on_menu_event(&self, menu_id: &str) {
        if self.closed.get() {
            return;
        }

        let message = {
            let state = self.state.borrow();
            state.menu_messages.get(menu_id).cloned()
        };

        if let Some(message) = message {
            self.dispatch_tray_event(TrayEvent::Menu(message));
        }
    }

    fn on_interaction(&self, event: InteractionEvent) {
        if self.closed.get() {
            return;
        }
        self.dispatch_tray_event(TrayEvent::Interaction(event));
    }

    fn on_menu_tracking_changed(&self, open: bool) {
        self.menu_tracking.set(open);
        if !open && self.pending_render.replace(false) {
            self.render_or_log();
            self.maybe_shutdown();
        }
    }
}

struct NativeTray {
    tray_id: String,
    status_item: Option<Retained<NSStatusItem>>,
    tray_target: Option<Retained<TrayTarget>>,
    menu_tree: Option<NativeMenuTree>,
    mtm: MainThreadMarker,
}

impl NativeTray {
    fn new<Message>(
        tray_id: &str,
        view: &NormalizedTrayView<Message>,
        menu_tree: Option<NativeMenuTree>,
    ) -> Result<Self> {
        let mtm = MainThreadMarker::new().ok_or(Error::Unsupported(
            "macOS tray objects must be created and updated on the main thread",
        ))?;

        let mut tray = Self {
            tray_id: tray_id.to_string(),
            status_item: None,
            tray_target: None,
            menu_tree,
            mtm,
        };

        if view.visible {
            tray.create_status_item(view)?;
        }

        Ok(tray)
    }

    fn apply_view<Message>(
        &mut self,
        view: &NormalizedTrayView<Message>,
        replace_menu: bool,
        menu_tree: Option<NativeMenuTree>,
    ) -> Result<()> {
        if replace_menu {
            self.menu_tree = menu_tree;
        }

        if !view.visible {
            self.remove();
            return Ok(());
        }

        if self.status_item.is_none() {
            self.create_status_item(view)?;
            return Ok(());
        }

        if let Some(status_item) = self.status_item.as_deref() {
            set_status_item_icon(status_item, view.icon.as_ref(), self.mtm)?;
            set_status_item_title(status_item, view.title.as_deref(), self.mtm);
            set_status_item_tooltip(status_item, view.tooltip.as_deref(), self.mtm);
        }

        if let Some(tray_target) = self.tray_target.as_deref() {
            tray_target
                .ivars()
                .menu_on_left_click
                .set(view.menu_on_primary_click);
            tray_target.update_dimensions();
        }

        if replace_menu {
            self.attach_menu();
        }

        Ok(())
    }

    fn create_status_item<Message>(&mut self, view: &NormalizedTrayView<Message>) -> Result<()> {
        // Reference: tray-icon/src/platform_impl/macos/mod.rs:49.
        let status_item =
            NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);

        set_status_item_icon(&status_item, view.icon.as_ref(), self.mtm)?;
        set_status_item_title(&status_item, view.title.as_deref(), self.mtm);
        set_status_item_tooltip(&status_item, view.tooltip.as_deref(), self.mtm);

        let tray_target = unsafe {
            let button = status_item.button(self.mtm).unwrap();
            let frame = button.frame();
            let target = self.mtm.alloc().set_ivars(TrayTargetIvars {
                tray_id: NSString::from_str(&self.tray_id),
                menu: RefCell::new(None),
                status_item: status_item.retain(),
                menu_on_left_click: Cell::new(view.menu_on_primary_click),
            });
            let tray_target: Retained<TrayTarget> = msg_send![super(target), initWithFrame: frame];
            tray_target.setWantsLayer(true);
            button.addSubview(&tray_target);
            tray_target
        };

        self.status_item = Some(status_item);
        self.tray_target = Some(tray_target);
        self.attach_menu();
        Ok(())
    }

    fn attach_menu(&mut self) {
        let Some(status_item) = self.status_item.as_deref() else {
            return;
        };
        let Some(tray_target) = self.tray_target.as_deref() else {
            return;
        };

        unsafe {
            if let Some(menu_tree) = &self.menu_tree {
                status_item.setMenu(Some(&menu_tree.root));
                let () = msg_send![&*menu_tree.root, setDelegate: &*menu_tree.delegate];
                *tray_target.ivars().menu.borrow_mut() = Some(menu_tree.root.retain());
            } else {
                status_item.setMenu(None);
                *tray_target.ivars().menu.borrow_mut() = None;
            }
        }
    }

    fn remove(&mut self) {
        if let (Some(status_item), Some(tray_target)) = (&self.status_item, &self.tray_target) {
            NSStatusBar::systemStatusBar().removeStatusItem(status_item);
            tray_target.removeFromSuperview();
        }

        self.status_item = None;
        self.tray_target = None;
    }
}

impl Drop for NativeTray {
    fn drop(&mut self) {
        self.remove();
    }
}

struct NativeMenuTree {
    root: Retained<NSMenu>,
    delegate: Retained<MenuDelegate>,
    _action_states: Vec<Rc<MenuActionState>>,
}

impl NativeMenuTree {
    fn new<Message>(tray_id: &str, items: &[NormalizedMenuItem<Message>]) -> Result<Self> {
        // Reference: muda/src/platform_impl/macos/mod.rs:81.
        let mtm = MainThreadMarker::new().ok_or(Error::Unsupported(
            "macOS menus must be created on the main thread",
        ))?;
        let root = NSMenu::new(mtm);
        root.setAutoenablesItems(false);
        let delegate = MenuDelegate::new(mtm, tray_id);
        unsafe {
            let () = msg_send![&*root, setDelegate: &*delegate];
        }

        let mut action_states = Vec::new();
        let mut path = Vec::new();
        for (index, item) in items.iter().enumerate() {
            path.push(index);
            append_menu_item(&root, tray_id, &mut path, item, &mut action_states, mtm)?;
            path.pop();
        }

        Ok(Self {
            root,
            delegate,
            _action_states: action_states,
        })
    }
}

#[derive(Debug)]
struct MenuActionState {
    tray_id: String,
    menu_id: String,
}

#[derive(Debug)]
struct TrayTargetIvars {
    tray_id: Retained<NSString>,
    menu: RefCell<Option<Retained<NSMenu>>>,
    status_item: Retained<NSStatusItem>,
    menu_on_left_click: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "TrayinitTrayTarget"]
    #[ivars = TrayTargetIvars]
    struct TrayTarget;

    impl TrayTarget {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            show_menu_if_needed(self, MouseButton::Left);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let mtm = MainThreadMarker::from(self);
            let button = self.ivars().status_item.button(mtm).unwrap();
            button.highlight(false);

            let has_menu = tray_target_has_menu(self);
            if !(self.ivars().menu_on_left_click.get() && has_menu) {
                dispatch_interaction_from_event(self, event, InteractionKind::PrimaryActivate);
            }
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _event: &NSEvent) {
            show_menu_if_needed(self, MouseButton::Right);
        }

        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: &NSEvent) {
            if !tray_target_has_menu(self) {
                dispatch_interaction_from_event(self, event, InteractionKind::ContextMenu);
            }
        }

        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, event: &NSEvent) {
            if event.buttonNumber() == 2 {
                dispatch_interaction_from_event(self, event, InteractionKind::SecondaryActivate);
            }
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            unsafe {
                // Reference: tray-icon/src/platform_impl/macos/mod.rs:428.
                let areas = self.trackingAreas();
                for index in 0..areas.count() {
                    let area = areas.objectAtIndex(index);
                    self.removeTrackingArea(&area);
                }

                let _: () = msg_send![super(self), updateTrackingAreas];

                let options = NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::MouseMoved
                    | NSTrackingAreaOptions::ActiveAlways
                    | NSTrackingAreaOptions::InVisibleRect;
                let rect = CGRect {
                    origin: CGPoint { x: 0.0, y: 0.0 },
                    size: CGSize {
                        width: 0.0,
                        height: 0.0,
                    },
                };
                let area = NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    rect,
                    options,
                    Some(self),
                    None,
                );
                self.addTrackingArea(&area);
            }
        }
    }
);

impl TrayTarget {
    fn update_dimensions(&self) {
        let mtm = MainThreadMarker::from(self);
        let button = self.ivars().status_item.button(mtm).unwrap();
        self.setFrame(button.frame());
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TrayinitMenuDelegate"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Retained<NSString>]
    struct MenuDelegate;

    impl MenuDelegate {
        #[unsafe(method(menuWillOpen:))]
        fn menu_will_open(&self, _menu: &NSMenu) {
            dispatch_menu_tracking(&self.ivars().to_string(), true);
        }

        #[unsafe(method(menuDidClose:))]
        fn menu_did_close(&self, _menu: &NSMenu) {
            dispatch_menu_tracking(&self.ivars().to_string(), false);
        }
    }
);

impl MenuDelegate {
    fn new(mtm: MainThreadMarker, tray_id: &str) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(NSString::from_str(tray_id));
        unsafe { msg_send![super(this), init] }
    }
}

define_class!(
    #[unsafe(super(NSMenuItem))]
    #[name = "TrayinitMenuItem"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Cell<*const MenuActionState>]
    struct ActionMenuItem;

    impl ActionMenuItem {
        #[unsafe(method(fireMenuItemAction:))]
        fn fire_menu_item_action(&self, _sender: Option<&AnyObject>) {
            let Some(state) = (unsafe { self.ivars().get().as_ref() }) else {
                return;
            };
            dispatch_menu_action(&state.tray_id, &state.menu_id);
        }
    }
);

impl ActionMenuItem {
    fn new(
        mtm: MainThreadMarker,
        title: &str,
        action: Option<Sel>,
        accelerator: Option<&Accelerator>,
    ) -> Result<Retained<Self>> {
        // Reference:
        // muda/src/platform_impl/macos/mod.rs:959.
        // muda/src/platform_impl/macos/mod.rs:1051.
        let title = NSString::from_str(title);
        let key_equivalent = accelerator
            .map(key_equivalent)
            .transpose()
            .map_err(Error::Accelerator)?
            .unwrap_or_default();
        let key_equivalent = NSString::from_str(&key_equivalent);
        let modifier_mask = accelerator
            .map(modifier_mask)
            .transpose()
            .map_err(Error::Accelerator)?
            .unwrap_or_else(NSEventModifierFlags::empty);

        let this = mtm.alloc().set_ivars(Cell::new(ptr::null()));
        let item: Retained<Self> = unsafe {
            msg_send![super(this), initWithTitle: &*title, action: action, keyEquivalent: &*key_equivalent]
        };
        item.setKeyEquivalentModifierMask(modifier_mask);
        Ok(item)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MouseButton {
    Left,
    Right,
}

fn show_menu_if_needed(this: &TrayTarget, button: MouseButton) {
    // Reference: tray-icon/src/platform_impl/macos/mod.rs:471.
    let mtm = MainThreadMarker::from(this);
    unsafe {
        let ns_button = this.ivars().status_item.button(mtm).unwrap();
        let menu_on_left_click = this.ivars().menu_on_left_click.get();
        let has_menu = tray_target_has_menu(this);

        if button == MouseButton::Right || (menu_on_left_click && button == MouseButton::Left) {
            if has_menu {
                ns_button.performClick(None);
            } else {
                ns_button.highlight(true);
            }
        } else {
            ns_button.highlight(true);
        }
    }
}

fn tray_target_has_menu(this: &TrayTarget) -> bool {
    if let Some(menu) = &*this.ivars().menu.borrow() {
        menu.numberOfItems() > 0
    } else {
        false
    }
}

fn dispatch_interaction_from_event(this: &TrayTarget, event: &NSEvent, kind: InteractionKind) {
    // Reference:
    // tray-icon/src/platform_impl/macos/mod.rs:494.
    // tray-icon/src/platform_impl/macos/mod.rs:509.
    let mtm = MainThreadMarker::from(this);
    let tray_id = this.ivars().tray_id.to_string();

    let window = event.window(mtm).unwrap();
    let icon_rect = tray_rect(&window);
    let mouse_location = NSEvent::mouseLocation();
    let scale_factor = window.backingScaleFactor();
    let cursor_position: dpi::PhysicalPosition<f64> = dpi::LogicalPosition::new(
        mouse_location.x,
        flip_window_screen_coordinates(mouse_location.y),
    )
    .to_physical(scale_factor);

    dispatch_interaction(
        &tray_id,
        InteractionEvent {
            kind,
            position: Some(dpi::PhysicalPosition::new(
                cursor_position.x.round() as i32,
                cursor_position.y.round() as i32,
            )),
            area: Some(icon_rect),
        },
    );
}

fn tray_rect(window: &NSWindow) -> (dpi::PhysicalPosition<i32>, dpi::PhysicalSize<i32>) {
    let frame = window.frame();
    let scale_factor = window.backingScaleFactor();
    let position: dpi::PhysicalPosition<f64> = dpi::LogicalPosition::new(
        frame.origin.x,
        flip_window_screen_coordinates(frame.origin.y) - frame.size.height,
    )
    .to_physical(scale_factor);
    let size: dpi::PhysicalSize<f64> =
        dpi::LogicalSize::new(frame.size.width, frame.size.height).to_physical(scale_factor);

    (
        dpi::PhysicalPosition::new(position.x.round() as i32, position.y.round() as i32),
        dpi::PhysicalSize::new(size.width.round() as i32, size.height.round() as i32),
    )
}

fn flip_window_screen_coordinates(y: f64) -> f64 {
    CGDisplayPixelsHigh(CGMainDisplayID()) as f64 - y
}

fn stop_app_loop() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    let app = NSApplication::sharedApplication(mtm);
    app.stop(None);

    if let Some(event) = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::ApplicationDefined,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::empty(),
        0.0,
        0,
        None,
        0,
        0,
        0,
    ) {
        app.postEvent_atStart(&event, true);
    }
}

fn set_status_item_icon(
    status_item: &NSStatusItem,
    icon: Option<&Icon>,
    mtm: MainThreadMarker,
) -> Result<()> {
    let button = status_item.button(mtm).unwrap();

    if let Some(icon) = icon {
        let nsimage = to_nsimage(icon, Some(18.0))?;
        button.setImage(Some(&nsimage));
        button.setImagePosition(NSCellImagePosition::ImageLeft);
    } else {
        button.setImage(None);
    }

    Ok(())
}

fn set_status_item_title(status_item: &NSStatusItem, title: Option<&str>, mtm: MainThreadMarker) {
    if let Some(button) = status_item.button(mtm) {
        let title = NSString::from_str(title.unwrap_or_default());
        button.setTitle(&title);
    }
}

fn set_status_item_tooltip(
    status_item: &NSStatusItem,
    tooltip: Option<&str>,
    mtm: MainThreadMarker,
) {
    if let Some(button) = status_item.button(mtm) {
        let tooltip = tooltip.map(NSString::from_str);
        button.setToolTip(tooltip.as_deref());
    }
}

fn append_menu_item<Message>(
    parent: &NSMenu,
    tray_id: &str,
    path: &mut Vec<usize>,
    item: &NormalizedMenuItem<Message>,
    action_states: &mut Vec<Rc<MenuActionState>>,
    mtm: MainThreadMarker,
) -> Result<()> {
    match item {
        NormalizedMenuItem::Standard(item) => {
            let menu_item =
                build_action_menu_item(tray_id, path, item, action_states, mtm, false, false)?;
            parent.addItem(&menu_item);
        },
        NormalizedMenuItem::Check(item) => {
            let menu_item =
                build_action_menu_item(tray_id, path, item, action_states, mtm, true, false)?;
            parent.addItem(&menu_item);
        },
        NormalizedMenuItem::Radio(item) => {
            let menu_item =
                build_action_menu_item(tray_id, path, item, action_states, mtm, false, true)?;
            parent.addItem(&menu_item);
        },
        NormalizedMenuItem::Submenu(submenu) => {
            let submenu_menu = NSMenu::new(mtm);
            submenu_menu.setAutoenablesItems(false);

            for (index, child) in submenu.children.iter().enumerate() {
                path.push(index);
                append_menu_item(&submenu_menu, tray_id, path, child, action_states, mtm)?;
                path.pop();
            }

            let title = strip_mnemonic(&submenu.label);
            let menu_item = ActionMenuItem::new(mtm, &title, None, None)?;
            menu_item.setEnabled(submenu.enabled);
            menu_item.setSubmenu(Some(&submenu_menu));
            if let Some(icon) = submenu.icon.as_ref() {
                set_menu_item_icon(&menu_item, Some(icon))?;
            }
            parent.addItem(&menu_item);
        },
        NormalizedMenuItem::Separator => {
            parent.addItem(&NSMenuItem::separatorItem(mtm));
        },
    }

    Ok(())
}

fn build_action_menu_item<Message>(
    tray_id: &str,
    path: &[usize],
    item: &NormalizedCommandItem<Message>,
    action_states: &mut Vec<Rc<MenuActionState>>,
    mtm: MainThreadMarker,
    is_check: bool,
    is_radio: bool,
) -> Result<Retained<ActionMenuItem>> {
    // Reference:
    // muda/src/platform_impl/macos/mod.rs:453.
    // muda/src/platform_impl/macos/mod.rs:1051.
    let title = strip_mnemonic(&item.label);
    let action = if item.message.is_some() {
        Some(sel!(fireMenuItemAction:))
    } else {
        None
    };
    let menu_item = ActionMenuItem::new(mtm, &title, action, item.accelerator.as_ref())?;

    unsafe {
        if action.is_some() {
            menu_item.setTarget(Some(&menu_item));
        }
        menu_item.setEnabled(item.enabled);
    }

    if let Some(icon) = item.icon.as_ref() {
        set_menu_item_icon(&menu_item, Some(icon))?;
    }

    if is_check {
        let checked = matches!(item.state, CommandState::Check { checked: true });
        menu_item.setState(if checked {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
    } else if is_radio {
        let selected = matches!(item.state, CommandState::Radio { selected: true });
        menu_item.setState(if selected {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
    }

    if item.message.is_some() {
        let state = Rc::new(MenuActionState {
            tray_id: tray_id.to_string(),
            menu_id: native_menu_id(tray_id, path),
        });
        menu_item.ivars().set(Rc::as_ptr(&state));
        action_states.push(state);
    }

    Ok(menu_item)
}

fn set_menu_item_icon(menu_item: &NSMenuItem, icon: Option<&Icon>) -> Result<()> {
    // Reference: muda/src/platform_impl/macos/mod.rs:1076.
    if let Some(icon) = icon {
        let nsimage = to_nsimage(icon, Some(18.0))?;
        menu_item.setImage(Some(&nsimage));
    } else {
        menu_item.setImage(None);
    }
    Ok(())
}

fn native_menu_id(tray_id: &str, path: &[usize]) -> String {
    let mut id = String::from("trayinit::macos::");
    id.push_str(tray_id);
    id.push_str("::");
    for (index, segment) in path.iter().enumerate() {
        if index > 0 {
            id.push('.');
        }
        id.push_str(&segment.to_string());
    }
    id
}

fn collect_menu_messages<Message: Clone>(
    tray_id: &str,
    items: &[NormalizedMenuItem<Message>],
) -> HashMap<String, Message> {
    let mut messages = HashMap::new();
    let mut path = Vec::new();
    collect_menu_messages_inner(tray_id, items, &mut path, &mut messages);
    messages
}

fn collect_menu_messages_inner<Message: Clone>(
    tray_id: &str,
    items: &[NormalizedMenuItem<Message>],
    path: &mut Vec<usize>,
    messages: &mut HashMap<String, Message>,
) {
    for (index, item) in items.iter().enumerate() {
        path.push(index);
        match item {
            NormalizedMenuItem::Standard(item)
            | NormalizedMenuItem::Check(item)
            | NormalizedMenuItem::Radio(item) => {
                if let Some(message) = item.message.as_ref() {
                    messages.insert(native_menu_id(tray_id, path), message.clone());
                }
            },
            NormalizedMenuItem::Submenu(submenu) => {
                collect_menu_messages_inner(tray_id, &submenu.children, path, messages);
            },
            NormalizedMenuItem::Separator => {},
        }
        path.pop();
    }
}

fn menu_visuals_differ<Message>(
    old: &[NormalizedMenuItem<Message>],
    new: &[NormalizedMenuItem<Message>],
) -> bool {
    if old.len() != new.len() {
        return true;
    }

    old.iter().zip(new).any(|(old, new)| match (old, new) {
        (NormalizedMenuItem::Standard(old), NormalizedMenuItem::Standard(new))
        | (NormalizedMenuItem::Check(old), NormalizedMenuItem::Check(new))
        | (NormalizedMenuItem::Radio(old), NormalizedMenuItem::Radio(new)) => {
            old.label != new.label
                || old.enabled != new.enabled
                || old.icon != new.icon
                || old.icon_name != new.icon_name
                || old.accelerator != new.accelerator
                || old.state != new.state
        },
        (NormalizedMenuItem::Submenu(old), NormalizedMenuItem::Submenu(new)) => {
            old.label != new.label
                || old.enabled != new.enabled
                || old.icon != new.icon
                || old.icon_name != new.icon_name
                || menu_visuals_differ(&old.children, &new.children)
        },
        (NormalizedMenuItem::Separator, NormalizedMenuItem::Separator) => false,
        _ => true,
    })
}
