//! macOS backend built on top of `tray-icon`'s `NSStatusItem` implementation
//! and its re-exported `muda` menu types, rather than a fresh AppKit rewrite.
//! That keeps behavior close to the battle-tested upstream path we already use
//! as a reference on Windows.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Once;
use std::{fmt, mem, thread_local};

use dpi::{PhysicalPosition, PhysicalSize};
use tray_icon::menu::accelerator::Accelerator as NativeAccelerator;
use tray_icon::menu::{
    self as native_menu, CheckMenuItem as NativeCheckMenuItem, ContextMenu, Icon as NativeMenuIcon,
    IconMenuItem, IsMenuItem, Menu as NativeMenu, MenuEvent as NativeMenuEvent,
    MenuId as NativeMenuId, MenuItem as NativeMenuItem,
    PredefinedMenuItem as NativePredefinedMenuItem, Submenu as NativeSubmenu,
};
use tray_icon::{
    Icon as NativeTrayIcon, MouseButton, MouseButtonState, Rect as NativeRect,
    TrayIcon as NativeTrayIconHandle, TrayIconBuilder, TrayIconEvent,
};

use crate::menu::Accelerator;
use crate::model::{CommandState, NormalizedMenuItem, NormalizedTrayView};
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

    install_event_handlers();

    let Builder {
        tray,
        runtime_preference: _,
        linux: _,
    } = builder;

    let shared = Shared::new(tray)?;
    let tray_id = shared.state.borrow().tray_id.clone();
    Shared::register(shared.clone());
    shared.maybe_shutdown();

    Ok(Handle::new(tray_id, PlatformHandle::new(&shared)))
}

pub fn run<T: Tray>(builder: Builder<T>) -> Result<()>
where
    T::Message: Clone,
{
    let _ = builder;
    Err(Error::Unsupported(
        "macOS run() is not implemented yet; use attach() from an existing AppKit event loop",
    ))
}

trait RuntimeOps {
    fn on_menu_event(&self, menu_id: &str);
    fn on_tray_icon_event(&self, event: &TrayIconEvent);
}

#[derive(Default)]
struct Registry {
    trays: HashMap<String, Rc<dyn RuntimeOps>>,
    menus: HashMap<String, String>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
}

static EVENT_HANDLERS_INSTALLED: Once = Once::new();

fn install_event_handlers() {
    EVENT_HANDLERS_INSTALLED.call_once(|| {
        TrayIconEvent::set_event_handler(Some(on_tray_icon_event));
        NativeMenuEvent::set_event_handler(Some(on_menu_event));
    });
}

fn on_tray_icon_event(event: TrayIconEvent) {
    let tray_id = event.id().as_ref().to_string();
    let runtime = REGISTRY.with(|registry| registry.borrow().trays.get(&tray_id).cloned());
    if let Some(runtime) = runtime {
        runtime.on_tray_icon_event(&event);
    }
}

fn on_menu_event(event: NativeMenuEvent) {
    let menu_id = event.id.as_ref().to_string();
    let runtime = REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(tray_id) = registry.menus.get(&menu_id) else {
            return None;
        };
        registry.trays.get(tray_id).cloned()
    });
    if let Some(runtime) = runtime {
        runtime.on_menu_event(&menu_id);
    }
}

struct Shared<T: Tray> {
    closed: Cell<bool>,
    state: RefCell<State<T>>,
}

struct State<T: Tray> {
    tray_id: String,
    tray: T,
    tray_icon: NativeTrayIconHandle,
    menu_messages: HashMap<String, T::Message>,
    registered_menu_ids: Vec<String>,
    has_menu: bool,
}

impl<T: Tray> Shared<T> {
    fn register(self: Rc<Self>)
    where
        T::Message: Clone,
    {
        let (tray_id, menu_ids) = {
            let state = self.state.borrow();
            (state.tray_id.clone(), state.registered_menu_ids.clone())
        };

        let runtime: Rc<dyn RuntimeOps> = self;
        REGISTRY.with(|registry| {
            let mut registry = registry.borrow_mut();
            registry.trays.insert(tray_id.clone(), runtime);
            for menu_id in menu_ids {
                registry.menus.insert(menu_id, tray_id.clone());
            }
        });
    }

    fn maybe_shutdown(&self) {
        if self.closed.get() {
            return;
        }

        let should_exit = self.state.borrow().tray.should_exit();
        if should_exit {
            self.shutdown();
        }
    }

    fn shutdown(&self) {
        if self.closed.replace(true) {
            return;
        }

        let (tray_id, menu_ids) = {
            let mut state = self.state.borrow_mut();
            state.menu_messages.clear();
            state.has_menu = false;
            (
                state.tray_id.clone(),
                mem::take(&mut state.registered_menu_ids),
            )
        };

        REGISTRY.with(|registry| {
            let mut registry = registry.borrow_mut();
            registry.trays.remove(&tray_id);
            for menu_id in menu_ids {
                registry.menus.remove(&menu_id);
            }
        });
    }
}

impl<T: Tray> Shared<T>
where
    T::Message: Clone,
{
    fn new(tray: T) -> Result<Rc<Self>> {
        let tray_id = tray.id().to_string();
        let view = NormalizedTrayView::from_tray(&tray);
        let (menu, menu_messages, registered_menu_ids) = build_native_menu(&tray_id, &view.menu)?;

        let mut builder = TrayIconBuilder::new()
            .with_id(tray_id.clone())
            .with_menu_on_left_click(view.menu_on_primary_click);

        if let Some(icon) = view.icon.as_ref() {
            builder = builder.with_icon(to_tray_icon(icon)?);
        }
        if let Some(title) = view.title.as_deref() {
            builder = builder.with_title(title);
        }
        if let Some(tooltip) = view.tooltip.as_deref() {
            builder = builder.with_tooltip(tooltip);
        }
        if let Some(menu) = menu {
            builder = builder.with_menu(Box::new(menu) as Box<dyn ContextMenu>);
        }

        let tray_icon = builder.build().map_err(map_tray_icon_error)?;
        if !view.visible {
            tray_icon.set_visible(false).map_err(map_tray_icon_error)?;
        }

        Ok(Rc::new(Self {
            closed: Cell::new(false),
            state: RefCell::new(State {
                tray_id,
                tray,
                tray_icon,
                menu_messages,
                registered_menu_ids,
                has_menu: !view.menu.is_empty(),
            }),
        }))
    }

    fn render(&self) -> Result<()> {
        if self.closed.get() {
            return Ok(());
        }

        let (tray_id, old_menu_ids, new_menu_ids) = {
            let mut state = self.state.borrow_mut();
            let view = NormalizedTrayView::from_tray(&state.tray);
            let (menu, menu_messages, registered_menu_ids) =
                build_native_menu(&state.tray_id, &view.menu)?;

            state
                .tray_icon
                .set_icon(view.icon.as_ref().map(to_tray_icon).transpose()?)
                .map_err(map_tray_icon_error)?;
            state
                .tray_icon
                .set_tooltip(view.tooltip.as_deref())
                .map_err(map_tray_icon_error)?;
            state.tray_icon.set_title(view.title.as_deref());
            state
                .tray_icon
                .set_visible(view.visible)
                .map_err(map_tray_icon_error)?;
            state
                .tray_icon
                .set_show_menu_on_left_click(view.menu_on_primary_click);
            state
                .tray_icon
                .set_menu(menu.map(|menu| Box::new(menu) as Box<dyn ContextMenu>));

            let tray_id = state.tray_id.clone();
            let old_menu_ids = mem::take(&mut state.registered_menu_ids);
            let new_menu_ids = registered_menu_ids.clone();
            state.registered_menu_ids = registered_menu_ids;
            state.menu_messages = menu_messages;
            state.has_menu = !view.menu.is_empty();

            (tray_id, old_menu_ids, new_menu_ids)
        };

        REGISTRY.with(|registry| {
            let mut registry = registry.borrow_mut();
            for menu_id in old_menu_ids {
                registry.menus.remove(&menu_id);
            }
            for menu_id in &new_menu_ids {
                registry.menus.insert(menu_id.clone(), tray_id.clone());
            }
        });

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

    fn on_tray_icon_event(&self, event: &TrayIconEvent) {
        if self.closed.get() {
            return;
        }

        let TrayIconEvent::Click {
            position,
            rect,
            button,
            button_state,
            ..
        } = event
        else {
            return;
        };

        if *button_state != MouseButtonState::Up {
            return;
        }

        let (menu_on_primary_click, has_menu) = {
            let state = self.state.borrow();
            (state.tray.menu_on_primary_click(), state.has_menu)
        };

        let kind = match button {
            MouseButton::Left if !(menu_on_primary_click && has_menu) => {
                Some(InteractionKind::PrimaryActivate)
            },
            MouseButton::Right if !has_menu => Some(InteractionKind::ContextMenu),
            MouseButton::Middle => Some(InteractionKind::SecondaryActivate),
            _ => None,
        };

        if let Some(kind) = kind {
            self.dispatch_tray_event(TrayEvent::Interaction(InteractionEvent {
                kind,
                position: Some(position_from_native(*position)),
                area: Some(area_from_native(rect)),
            }));
        }
    }
}

fn build_native_menu<Message: Clone>(
    tray_id: &str,
    items: &[NormalizedMenuItem<Message>],
) -> Result<(Option<NativeMenu>, HashMap<String, Message>, Vec<String>)> {
    if items.is_empty() {
        return Ok((None, HashMap::new(), Vec::new()));
    }

    let menu = NativeMenu::new();
    let mut menu_messages = HashMap::new();
    let mut registered_menu_ids = Vec::new();
    let mut path = Vec::new();

    for (index, item) in items.iter().enumerate() {
        path.push(index);
        append_normalized_item(
            &menu,
            tray_id,
            &mut path,
            item,
            &mut menu_messages,
            &mut registered_menu_ids,
        )?;
        path.pop();
    }

    Ok((Some(menu), menu_messages, registered_menu_ids))
}

trait NativeMenuParent {
    fn append_item(&self, item: &dyn IsMenuItem) -> native_menu::Result<()>;
}

impl NativeMenuParent for NativeMenu {
    fn append_item(&self, item: &dyn IsMenuItem) -> native_menu::Result<()> {
        self.append(item)
    }
}

impl NativeMenuParent for NativeSubmenu {
    fn append_item(&self, item: &dyn IsMenuItem) -> native_menu::Result<()> {
        self.append(item)
    }
}

fn append_normalized_item<P, Message: Clone>(
    parent: &P,
    tray_id: &str,
    path: &mut Vec<usize>,
    item: &NormalizedMenuItem<Message>,
    menu_messages: &mut HashMap<String, Message>,
    registered_menu_ids: &mut Vec<String>,
) -> Result<()>
where
    P: NativeMenuParent,
{
    match item {
        NormalizedMenuItem::Standard(item) => append_command_item(
            parent,
            tray_id,
            path,
            item,
            menu_messages,
            registered_menu_ids,
            false,
        ),
        NormalizedMenuItem::Check(item) | NormalizedMenuItem::Radio(item) => append_command_item(
            parent,
            tray_id,
            path,
            item,
            menu_messages,
            registered_menu_ids,
            true,
        ),
        NormalizedMenuItem::Submenu(submenu) => {
            let id = native_menu_id(tray_id, path);
            let native =
                NativeSubmenu::with_id(NativeMenuId::new(&id), &submenu.label, submenu.enabled);
            if let Some(icon) = submenu.icon.as_ref() {
                native.set_icon(Some(to_menu_icon(icon)?));
            }

            for (index, child) in submenu.children.iter().enumerate() {
                path.push(index);
                append_normalized_item(
                    &native,
                    tray_id,
                    path,
                    child,
                    menu_messages,
                    registered_menu_ids,
                )?;
                path.pop();
            }

            parent.append_item(&native).map_err(map_menu_error)
        },
        NormalizedMenuItem::Separator => parent
            .append_item(&NativePredefinedMenuItem::separator())
            .map_err(map_menu_error),
    }
}

fn append_command_item<P, Message: Clone>(
    parent: &P,
    tray_id: &str,
    path: &[usize],
    item: &crate::model::NormalizedCommandItem<Message>,
    menu_messages: &mut HashMap<String, Message>,
    registered_menu_ids: &mut Vec<String>,
    uses_check_state: bool,
) -> Result<()>
where
    P: NativeMenuParent,
{
    let id = native_menu_id(tray_id, path);
    let accelerator = item.accelerator.as_ref().map(to_native_accelerator);

    match (&item.state, uses_check_state, item.icon.as_ref()) {
        (CommandState::Standard, _, Some(icon)) => {
            let native = IconMenuItem::with_id(
                NativeMenuId::new(&id),
                &item.label,
                item.enabled,
                Some(to_menu_icon(icon)?),
                accelerator,
            );
            parent.append_item(&native).map_err(map_menu_error)?;
        },
        (CommandState::Standard, _, None) => {
            let native = NativeMenuItem::with_id(
                NativeMenuId::new(&id),
                &item.label,
                item.enabled,
                accelerator,
            );
            parent.append_item(&native).map_err(map_menu_error)?;
        },
        (CommandState::Check { checked }, _, _)
        | (CommandState::Radio { selected: checked }, true, _) => {
            let native = NativeCheckMenuItem::with_id(
                NativeMenuId::new(&id),
                &item.label,
                item.enabled,
                *checked,
                accelerator,
            );
            parent.append_item(&native).map_err(map_menu_error)?;
        },
        (CommandState::Radio { selected }, false, _) => {
            let native = NativeCheckMenuItem::with_id(
                NativeMenuId::new(&id),
                &item.label,
                item.enabled,
                *selected,
                accelerator,
            );
            parent.append_item(&native).map_err(map_menu_error)?;
        },
    }

    if let Some(message) = item.message.as_ref() {
        menu_messages.insert(id.clone(), message.clone());
        registered_menu_ids.push(id);
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

fn to_tray_icon(icon: &Icon) -> Result<NativeTrayIcon> {
    NativeTrayIcon::from_rgba(icon.rgba().to_vec(), icon.width(), icon.height())
        .map_err(|error| Error::Backend(error.to_string()))
}

fn to_menu_icon(icon: &Icon) -> Result<NativeMenuIcon> {
    NativeMenuIcon::from_rgba(icon.rgba().to_vec(), icon.width(), icon.height())
        .map_err(|error| Error::Backend(error.to_string()))
}

fn to_native_accelerator(accelerator: &Accelerator) -> NativeAccelerator {
    NativeAccelerator::new(Some(accelerator.modifiers()), accelerator.key())
}

fn map_menu_error(error: native_menu::Error) -> Error {
    Error::Backend(error.to_string())
}

fn map_tray_icon_error(error: tray_icon::Error) -> Error {
    match error {
        tray_icon::Error::NotMainThread => {
            Error::Unsupported("macOS tray objects must be created and updated on the main thread")
        },
        tray_icon::Error::OsError(error) => Error::Os(error),
        other => Error::Backend(other.to_string()),
    }
}

fn position_from_native(position: tray_icon::dpi::PhysicalPosition<f64>) -> PhysicalPosition<i32> {
    PhysicalPosition::new(position.x.round() as i32, position.y.round() as i32)
}

fn area_from_native(rect: &NativeRect) -> (PhysicalPosition<i32>, PhysicalSize<i32>) {
    (
        position_from_native(rect.position),
        PhysicalSize::new(rect.size.width as i32, rect.size.height as i32),
    )
}
