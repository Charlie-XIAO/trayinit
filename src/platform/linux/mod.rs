mod menu;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, mpsc};
use std::{fmt, thread};

use dpi::PhysicalPosition;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::runtime::Builder as RuntimeBuilder;
use tokio::sync::mpsc as tokio_mpsc;
use zbus::fdo::DBusProxy;
use zbus::names::InterfaceName;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, Type, Value};
use zbus::{Connection, connection};

use self::menu::{Layout, MenuSnapshot, message_at_path};
use crate::model::NormalizedTrayView;
use crate::{
    Builder, ClosedError, Error, Handle, Icon, InteractionEvent, InteractionKind, LinuxOptions,
    Result, RuntimePreference, ScrollAxis, ScrollEvent, Tray, TrayEvent, TrayStatus,
};

const SNI_PATH: ObjectPath<'static> = ObjectPath::from_static_str_unchecked("/StatusNotifierItem");
const MENU_PATH: ObjectPath<'static> = ObjectPath::from_static_str_unchecked("/MenuBar");
const SNI_INTERFACE: InterfaceName<'static> =
    InterfaceName::from_static_str_unchecked("org.kde.StatusNotifierItem");
const MENU_INTERFACE: InterfaceName<'static> =
    InterfaceName::from_static_str_unchecked("com.canonical.dbusmenu");
static INSTANCE_COUNTER: AtomicUsize = AtomicUsize::new(1);

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

impl<T: Tray> fmt::Debug for PlatformHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlatformHandle")
            .field("closed", &self.is_closed())
            .finish()
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
            let mut state = self.shared.lock_state();
            f(&mut state.tray)
        };

        self.refresh()?;
        Ok(result)
    }

    pub fn refresh(&self) -> Result<(), ClosedError> {
        self.shared.post_command(Command::Refresh)
    }

    pub fn shutdown(&self) -> Result<()> {
        if self.is_closed() {
            return Ok(());
        }

        self.shared
            .post_command(Command::Shutdown)
            .map_err(Error::from)
    }

    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
    }
}

pub fn spawn<T: Tray>(builder: Builder<T>) -> Result<Handle<T>>
where
    T::Message: Clone,
{
    if matches!(
        builder.runtime_preference_ref(),
        RuntimePreference::CurrentThread
    ) {
        return Err(Error::Unsupported(
            "current-thread Linux tray runtime is not implemented yet",
        ));
    }

    start_backend(builder)
        .map(|(tray_id, shared)| Handle::new(tray_id, PlatformHandle::new(shared)))
}

pub fn attach<T: Tray>(builder: Builder<T>) -> Result<Handle<T>>
where
    T::Message: Clone,
{
    if matches!(
        builder.runtime_preference_ref(),
        RuntimePreference::CurrentThread
    ) {
        return Err(Error::Unsupported(
            "current-thread Linux tray runtime is not implemented yet",
        ));
    }

    start_backend(builder)
        .map(|(tray_id, shared)| Handle::new(tray_id, PlatformHandle::new(shared)))
}

pub fn run<T: Tray>(builder: Builder<T>) -> Result<()>
where
    T::Message: Clone,
{
    if matches!(
        builder.runtime_preference_ref(),
        RuntimePreference::CurrentThread
    ) {
        return Err(Error::Unsupported(
            "current-thread Linux tray runtime is not implemented yet",
        ));
    }

    let (_tray_id, shared) = start_backend(builder)?;
    shared.wait_closed();
    Ok(())
}

fn start_backend<T: Tray>(builder: Builder<T>) -> Result<(String, Arc<Shared<T>>)>
where
    T::Message: Clone,
{
    let Builder {
        tray,
        runtime_preference: _,
        linux,
    } = builder;

    let tray_id = tray.id().to_string();
    #[cfg(feature = "tracing")]
    tracing::debug!(tray_id = %tray_id, "Starting Linux tray backend");

    let (command_tx, command_rx) = tokio_mpsc::unbounded_channel();
    let shared = Arc::new(Shared::new(tray, command_tx));
    let init_shared = Arc::clone(&shared);
    let thread_name = format!("trayinit-linux-{}", tray_id);
    let (init_tx, init_rx) = mpsc::sync_channel(1);

    thread::Builder::new()
        .name(thread_name)
        .spawn(move || backend_thread(init_shared, command_rx, linux, init_tx))
        .map_err(Error::Os)?;

    match init_rx.recv() {
        Ok(Ok(())) => Ok((tray_id, shared)),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(Error::Initialization(
            "Linux tray backend exited before initialization completed",
        )),
    }
}

fn backend_thread<T: Tray>(
    shared: Arc<Shared<T>>,
    command_rx: tokio_mpsc::UnboundedReceiver<Command>,
    linux: LinuxOptions,
    init_tx: mpsc::SyncSender<Result<()>>,
) where
    T::Message: Clone,
{
    let runtime = match RuntimeBuilder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = init_tx.send(Err(Error::Backend(format!(
                "failed to create Linux tray runtime: {error}"
            ))));
            shared.mark_closed();
            return;
        },
    };

    let result = runtime.block_on(run_backend(shared.clone(), command_rx, linux, init_tx));
    if let Err(error) = result {
        eprintln!("trayinit: Linux tray backend exited with error: {error}");
    }
    shared.mark_closed();
}

async fn run_backend<T: Tray>(
    shared: Arc<Shared<T>>,
    mut command_rx: tokio_mpsc::UnboundedReceiver<Command>,
    linux: LinuxOptions,
    init_tx: mpsc::SyncSender<Result<()>>,
) -> Result<()>
where
    T::Message: Clone,
{
    let sni = StatusNotifierItem::new(Arc::clone(&shared));
    let menu = DbusMenu::new(Arc::clone(&shared));

    // Reference: ksni/src/service.rs::run connection builder.
    let conn = connection::Builder::session()
        .map_err(zbus_error)?
        .serve_at(SNI_PATH, sni)
        .map_err(zbus_error)?
        .serve_at(MENU_PATH, menu)
        .map_err(zbus_error)?
        .build()
        .await
        .map_err(zbus_error)?;

    let name = if linux.own_dbus_name {
        let name = format!(
            "org.kde.StatusNotifierItem-{}-{}",
            std::process::id(),
            INSTANCE_COUNTER.fetch_add(1, Ordering::AcqRel)
        );
        conn.request_name(&*name).await.map_err(zbus_error)?;
        name
    } else {
        conn.unique_name()
            .expect("zbus should expose a unique name after connecting")
            .to_string()
    };

    let watcher = StatusNotifierWatcherProxy::new(&conn)
        .await
        .map_err(zbus_error)?;
    let registered = match watcher.register_status_notifier_item(&name).await {
        Ok(()) => true,
        Err(error) => {
            let fdo_error: zbus::fdo::Error = error.into();
            if linux.assume_watcher_available
                && matches!(fdo_error, zbus::fdo::Error::ServiceUnknown(_))
            {
                false
            } else {
                return Err(Error::Backend(format!(
                    "failed to register Linux tray with StatusNotifierWatcher: {fdo_error}"
                )));
            }
        },
    };

    if registered
        && !linux.assume_watcher_available
        && !watcher
            .is_status_notifier_host_registered()
            .await
            .map_err(zbus_error)?
    {
        return Err(Error::Backend(
            "no StatusNotifierHost is currently registered".into(),
        ));
    }

    let dbus = DBusProxy::new(&conn).await.map_err(zbus_error)?;
    let mut watcher_name_changes = dbus
        .receive_name_owner_changed_with_args(&[(0, "org.kde.StatusNotifierWatcher")])
        .await
        .map_err(zbus_error)?;

    let _ = init_tx.send(Ok(()));

    loop {
        tokio::select! {
            Some(command) = command_rx.recv() => {
                match command {
                    Command::Refresh => {
                        if shared.refresh_and_emit(&conn).await? {
                            break;
                        }
                    }
                    Command::Shutdown => {
                        break;
                    }
                }
            }
            Some(change) = watcher_name_changes.next() => {
                let args = change.args().map_err(zbus_error)?;
                if args.new_owner.is_some() {
                    let _ = watcher.register_status_notifier_item(&name).await;
                }
            }
            else => break,
        }
    }

    Ok(())
}

struct Shared<T: Tray> {
    state: Mutex<ServiceState<T>>,
    command_tx: tokio_mpsc::UnboundedSender<Command>,
    closed: AtomicBool,
    close_state: (Mutex<bool>, Condvar),
}

impl<T: Tray> Shared<T> {
    fn new(tray: T, command_tx: tokio_mpsc::UnboundedSender<Command>) -> Self
    where
        T::Message: Clone,
    {
        Self {
            state: Mutex::new(ServiceState::new(tray)),
            command_tx,
            closed: AtomicBool::new(false),
            close_state: (Mutex::new(false), Condvar::new()),
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, ServiceState<T>> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn post_command(&self, command: Command) -> Result<(), ClosedError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(ClosedError);
        }
        self.command_tx.send(command).map_err(|_| ClosedError)
    }

    fn mark_closed(&self) {
        self.closed.store(true, Ordering::Release);
        let (flag, cvar) = &self.close_state;
        let mut closed = match flag.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *closed = true;
        cvar.notify_all();
    }

    fn wait_closed(&self) {
        let (flag, cvar) = &self.close_state;
        let mut closed = match flag.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        while !*closed {
            closed = match cvar.wait(closed) {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
    }

    async fn refresh_and_emit(&self, conn: &Connection) -> Result<bool>
    where
        T::Message: Clone,
    {
        let changes = {
            let mut state = self.lock_state();
            state.refresh_snapshot()
        };

        emit_snapshot_changes::<T>(conn, &changes).await?;
        Ok(changes.should_exit)
    }

    fn apply_interaction(&self, event: InteractionEvent)
    where
        T::Message: Clone,
    {
        let mut state = self.lock_state();
        state.tray.event(TrayEvent::Interaction(event));
    }

    fn apply_scroll(&self, event: ScrollEvent)
    where
        T::Message: Clone,
    {
        let mut state = self.lock_state();
        state.tray.event(TrayEvent::Scroll(event));
    }

    async fn dispatch_menu_path(&self, conn: &Connection, path: &[usize]) -> zbus::fdo::Result<()>
    where
        T::Message: Clone,
    {
        {
            let mut state = self.lock_state();
            let current_menu = NormalizedTrayView::from_tray(&state.tray).menu;
            let Some(message) = message_at_path(&current_menu, path) else {
                return Err(zbus::fdo::Error::InvalidArgs(
                    "menu item no longer exists".into(),
                ));
            };
            state.tray.event(TrayEvent::Menu(message));
        }

        let should_exit = self.refresh_and_emit(conn).await.map_err(to_fdo_error)?;
        if should_exit {
            let _ = self.post_command(Command::Shutdown);
        }
        Ok(())
    }
}

struct ServiceState<T: Tray> {
    tray: T,
    snapshot: Snapshot,
}

impl<T: Tray> ServiceState<T>
where
    T::Message: Clone,
{
    fn new(tray: T) -> Self {
        let snapshot = Snapshot::from_tray(&tray, 0, 0);
        Self { tray, snapshot }
    }

    fn refresh_snapshot(&mut self) -> SnapshotChanges {
        let old = self.snapshot.clone();
        let mut next = Snapshot::from_tray(&self.tray, old.menu_revision, old.menu_id_offset);
        let menu_diff = old.menu.diff(&next.menu);

        if menu_diff.layout_changed {
            next.menu_revision = old.menu_revision.saturating_add(1);
            next.menu_id_offset = old
                .menu_id_offset
                .saturating_add(old.menu.entry_count() as i32)
                .saturating_add(1);
        }

        let should_exit = self.tray.should_exit();
        self.snapshot = next.clone();

        SnapshotChanges {
            old,
            new: next,
            should_exit,
            menu_diff,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    id: String,
    title: String,
    status: Status,
    item_is_menu: bool,
    icon_pixmap: Vec<IconPixmap>,
    tool_tip: ToolTipData,
    menu_status: MenuStatus,
    menu: MenuSnapshot,
    menu_revision: u32,
    menu_id_offset: i32,
}

impl Snapshot {
    fn from_tray<T: Tray>(tray: &T, menu_revision: u32, menu_id_offset: i32) -> Self {
        let view = NormalizedTrayView::from_tray(tray);
        let title = view.title.unwrap_or_default();
        let icon_pixmap = view
            .icon
            .as_ref()
            .map(icon_pixmap_from_icon)
            .into_iter()
            .collect::<Vec<_>>();
        let menu = MenuSnapshot::from_normalized(&view.menu);
        let item_is_menu = view.menu_on_primary_click && !menu.is_empty();
        let status = status_from_view(view.visible, view.status);

        Self {
            id: tray.id().to_string(),
            title: title.clone(),
            status,
            item_is_menu,
            icon_pixmap: icon_pixmap.clone(),
            tool_tip: ToolTipData {
                icon_name: String::new(),
                icon_pixmap,
                title,
                description: view.tooltip.unwrap_or_default(),
            },
            menu_status: menu_status_from_tray_status(status),
            menu,
            menu_revision,
            menu_id_offset,
        }
    }
}

struct SnapshotChanges {
    old: Snapshot,
    new: Snapshot,
    should_exit: bool,
    menu_diff: menu::MenuDiff,
}

#[derive(Clone, Copy, Debug)]
enum Command {
    Refresh,
    Shutdown,
}

pub struct StatusNotifierItem<T: Tray>(Arc<Shared<T>>);

impl<T: Tray> StatusNotifierItem<T> {
    fn new(shared: Arc<Shared<T>>) -> Self {
        Self(shared)
    }
}

pub struct DbusMenu<T: Tray>(Arc<Shared<T>>);

impl<T: Tray> DbusMenu<T> {
    fn new(shared: Arc<Shared<T>>) -> Self {
        Self(shared)
    }
}

#[zbus::proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
trait StatusNotifierWatcher {
    async fn register_status_notifier_item(&self, service: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> zbus::Result<bool>;
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl<T> StatusNotifierItem<T>
where
    T: Tray,
    T::Message: Clone,
{
    async fn context_menu(
        &self,
        #[zbus(connection)] conn: &Connection,
        x: i32,
        y: i32,
    ) -> zbus::fdo::Result<()> {
        let has_menu = {
            let state = self.0.lock_state();
            !state.snapshot.menu.is_empty()
        };

        if has_menu {
            Err(zbus::fdo::Error::UnknownMethod("menu".into()))
        } else {
            self.0.apply_interaction(InteractionEvent {
                kind: InteractionKind::ContextMenu,
                position: Some(PhysicalPosition::new(x, y)),
                area: None,
            });
            let should_exit = self.0.refresh_and_emit(conn).await.map_err(to_fdo_error)?;
            if should_exit {
                let _ = self.0.post_command(Command::Shutdown);
            }
            Ok(())
        }
    }

    async fn activate(
        &self,
        #[zbus(connection)] conn: &Connection,
        x: i32,
        y: i32,
    ) -> zbus::fdo::Result<()> {
        let item_is_menu = {
            let state = self.0.lock_state();
            state.snapshot.item_is_menu
        };

        if item_is_menu {
            return Err(zbus::fdo::Error::UnknownMethod("ItemIsMenu".into()));
        }

        self.0.apply_interaction(InteractionEvent {
            kind: InteractionKind::PrimaryActivate,
            position: Some(PhysicalPosition::new(x, y)),
            area: None,
        });
        let should_exit = self.0.refresh_and_emit(conn).await.map_err(to_fdo_error)?;
        if should_exit {
            let _ = self.0.post_command(Command::Shutdown);
        }
        Ok(())
    }

    async fn secondary_activate(
        &self,
        #[zbus(connection)] conn: &Connection,
        x: i32,
        y: i32,
    ) -> zbus::fdo::Result<()> {
        self.0.apply_interaction(InteractionEvent {
            kind: InteractionKind::SecondaryActivate,
            position: Some(PhysicalPosition::new(x, y)),
            area: None,
        });
        let should_exit = self.0.refresh_and_emit(conn).await.map_err(to_fdo_error)?;
        if should_exit {
            let _ = self.0.post_command(Command::Shutdown);
        }
        Ok(())
    }

    async fn scroll(
        &self,
        #[zbus(connection)] conn: &Connection,
        delta: i32,
        orientation: Orientation,
    ) -> zbus::fdo::Result<()> {
        self.0.apply_scroll(ScrollEvent {
            delta,
            axis: match orientation {
                Orientation::Horizontal => ScrollAxis::Horizontal,
                Orientation::Vertical => ScrollAxis::Vertical,
            },
            position: None,
            area: None,
        });
        let should_exit = self.0.refresh_and_emit(conn).await.map_err(to_fdo_error)?;
        if should_exit {
            let _ = self.0.post_command(Command::Shutdown);
        }
        Ok(())
    }

    #[zbus(property)]
    fn category(&self) -> zbus::fdo::Result<Category> {
        Ok(Category::ApplicationStatus)
    }

    #[zbus(property)]
    fn id(&self) -> zbus::fdo::Result<String> {
        Ok(self.0.lock_state().snapshot.id.clone())
    }

    #[zbus(property)]
    fn title(&self) -> zbus::fdo::Result<String> {
        Ok(self.0.lock_state().snapshot.title.clone())
    }

    #[zbus(property)]
    fn status(&self) -> zbus::fdo::Result<Status> {
        Ok(self.0.lock_state().snapshot.status)
    }

    #[zbus(property)]
    fn window_id(&self) -> zbus::fdo::Result<i32> {
        Ok(0)
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> zbus::fdo::Result<String> {
        Ok(String::new())
    }

    #[zbus(property)]
    fn menu(&self) -> zbus::fdo::Result<ObjectPath<'static>> {
        Ok(MENU_PATH)
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> zbus::fdo::Result<bool> {
        Ok(self.0.lock_state().snapshot.item_is_menu)
    }

    #[zbus(property)]
    fn icon_name(&self) -> zbus::fdo::Result<String> {
        Ok(String::new())
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> zbus::fdo::Result<Vec<IconPixmap>> {
        Ok(self.0.lock_state().snapshot.icon_pixmap.clone())
    }

    #[zbus(property)]
    fn overlay_icon_name(&self) -> zbus::fdo::Result<String> {
        Ok(String::new())
    }

    #[zbus(property)]
    fn overlay_icon_pixmap(&self) -> zbus::fdo::Result<Vec<IconPixmap>> {
        Ok(Vec::new())
    }

    #[zbus(property)]
    fn attention_icon_name(&self) -> zbus::fdo::Result<String> {
        Ok(String::new())
    }

    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> zbus::fdo::Result<Vec<IconPixmap>> {
        Ok(Vec::new())
    }

    #[zbus(property)]
    fn attention_movie_name(&self) -> zbus::fdo::Result<String> {
        Ok(String::new())
    }

    #[zbus(property)]
    fn tool_tip(&self) -> zbus::fdo::Result<ToolTipData> {
        Ok(self.0.lock_state().snapshot.tool_tip.clone())
    }

    #[zbus(signal)]
    async fn new_title(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_icon(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_attention_icon(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_overlay_icon(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_tool_tip(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_status(ctxt: &SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl<T> DbusMenu<T>
where
    T: Tray,
    T::Message: Clone,
{
    async fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<(u32, Layout)> {
        let state = self.0.lock_state();
        let depth = if recursion_depth < 0 {
            None
        } else {
            Some(recursion_depth as usize)
        };
        let layout = state
            .snapshot
            .menu
            .layout_for_id(
                state.snapshot.menu_id_offset,
                parent_id,
                depth,
                &property_names,
            )
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("parentId not found".into()))?;
        Ok((state.snapshot.menu_revision, layout))
    }

    async fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<
        Vec<(
            i32,
            std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        )>,
    > {
        let state = self.0.lock_state();
        let mut grouped = Vec::new();
        for id in ids {
            if let Some(properties) = state.snapshot.menu.properties_for_id(
                state.snapshot.menu_id_offset,
                id,
                &property_names,
            ) {
                if !properties.is_empty() {
                    grouped.push((id, properties));
                }
            }
        }
        Ok(grouped)
    }

    async fn get_property(
        &self,
        id: i32,
        name: String,
    ) -> zbus::fdo::Result<zbus::zvariant::OwnedValue> {
        let state = self.0.lock_state();
        let mut properties = state
            .snapshot
            .menu
            .properties_for_id(state.snapshot.menu_id_offset, id, &[name])
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("id not found".into()))?;
        properties
            .drain()
            .next()
            .map(|(_, value)| value)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("property not found".into()))
    }

    async fn event(
        &self,
        #[zbus(connection)] conn: &Connection,
        id: i32,
        event_id: String,
        _data: zbus::zvariant::OwnedValue,
        _timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        if event_id != "clicked" {
            return Ok(());
        }

        let path = {
            let state = self.0.lock_state();
            state
                .snapshot
                .menu
                .message_path_for_id(state.snapshot.menu_id_offset, id)
                .map(ToOwned::to_owned)
        }
        .ok_or_else(|| zbus::fdo::Error::InvalidArgs("id not found".into()))?;

        self.0.dispatch_menu_path(conn, &path).await
    }

    async fn event_group(
        &self,
        #[zbus(connection)] conn: &Connection,
        events: Vec<(i32, String, zbus::zvariant::OwnedValue, u32)>,
    ) -> zbus::fdo::Result<Vec<i32>> {
        let mut not_found = Vec::new();
        for (id, event_id, _data, _timestamp) in events {
            if event_id != "clicked" {
                continue;
            }

            let path = {
                let state = self.0.lock_state();
                state
                    .snapshot
                    .menu
                    .message_path_for_id(state.snapshot.menu_id_offset, id)
                    .map(ToOwned::to_owned)
            };

            match path {
                Some(path) => {
                    if self.0.dispatch_menu_path(conn, &path).await.is_err() {
                        not_found.push(id);
                    }
                },
                None => not_found.push(id),
            }
        }
        Ok(not_found)
    }

    async fn about_to_show(&self) -> zbus::fdo::Result<bool> {
        Ok(false)
    }

    async fn about_to_show_group(&self) -> zbus::fdo::Result<(Vec<i32>, Vec<i32>)> {
        Ok((Vec::new(), Vec::new()))
    }

    #[zbus(property)]
    fn version(&self) -> zbus::fdo::Result<u32> {
        Ok(3)
    }

    #[zbus(property)]
    fn text_direction(&self) -> zbus::fdo::Result<TextDirection> {
        Ok(TextDirection::LeftToRight)
    }

    #[zbus(property)]
    fn status(&self) -> zbus::fdo::Result<MenuStatus> {
        Ok(self.0.lock_state().snapshot.menu_status)
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> zbus::fdo::Result<Vec<String>> {
        Ok(Vec::new())
    }

    #[zbus(signal)]
    async fn items_properties_updated(
        ctxt: &SignalEmitter<'_>,
        updated_props: Vec<(
            i32,
            std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        )>,
        removed_props: Vec<(i32, Vec<String>)>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn layout_updated(
        ctxt: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}

async fn emit_snapshot_changes<T: Tray>(conn: &Connection, changes: &SnapshotChanges) -> Result<()>
where
    T::Message: Clone,
{
    let sni = conn
        .object_server()
        .interface::<_, StatusNotifierItem<T>>(SNI_PATH)
        .await
        .map_err(zbus_error)?;
    let menu = conn
        .object_server()
        .interface::<_, DbusMenu<T>>(MENU_PATH)
        .await
        .map_err(zbus_error)?;

    if changes.old.title != changes.new.title {
        StatusNotifierItem::<T>::new_title(sni.signal_emitter())
            .await
            .map_err(zbus_error)?;
    }
    if changes.old.icon_pixmap != changes.new.icon_pixmap {
        StatusNotifierItem::<T>::new_icon(sni.signal_emitter())
            .await
            .map_err(zbus_error)?;
    }
    if changes.old.tool_tip != changes.new.tool_tip {
        StatusNotifierItem::<T>::new_tool_tip(sni.signal_emitter())
            .await
            .map_err(zbus_error)?;
    }
    if changes.old.status != changes.new.status {
        StatusNotifierItem::<T>::new_status(sni.signal_emitter(), &changes.new.status.to_string())
            .await
            .map_err(zbus_error)?;
        zbus::fdo::Properties::properties_changed(
            sni.signal_emitter(),
            SNI_INTERFACE,
            HashMap::from([("Status", Value::from(changes.new.status))]),
            Cow::Borrowed(&[]),
        )
        .await
        .map_err(zbus_error)?;
    }
    if changes.old.item_is_menu != changes.new.item_is_menu {
        zbus::fdo::Properties::properties_changed(
            sni.signal_emitter(),
            SNI_INTERFACE,
            HashMap::from([("ItemIsMenu", changes.new.item_is_menu.into())]),
            Cow::Borrowed(&[]),
        )
        .await
        .map_err(zbus_error)?;
    }
    if changes.old.menu_status != changes.new.menu_status {
        zbus::fdo::Properties::properties_changed(
            menu.signal_emitter(),
            MENU_INTERFACE,
            HashMap::from([("Status", Value::from(changes.new.menu_status))]),
            Cow::Borrowed(&[]),
        )
        .await
        .map_err(zbus_error)?;
    }

    if changes.menu_diff.layout_changed {
        DbusMenu::<T>::layout_updated(menu.signal_emitter(), changes.new.menu_revision, 0)
            .await
            .map_err(zbus_error)?;
    } else if !changes.menu_diff.updated_props.is_empty()
        || !changes.menu_diff.removed_props.is_empty()
    {
        let updated_props = changes
            .menu_diff
            .updated_props
            .iter()
            .map(|(index, properties)| {
                (
                    menu_item_id(changes.new.menu_id_offset, *index),
                    properties.clone(),
                )
            })
            .collect();
        let removed_props = changes
            .menu_diff
            .removed_props
            .iter()
            .map(|(index, properties)| {
                (
                    menu_item_id(changes.new.menu_id_offset, *index),
                    properties.clone(),
                )
            })
            .collect();
        DbusMenu::<T>::items_properties_updated(
            menu.signal_emitter(),
            updated_props,
            removed_props,
        )
        .await
        .map_err(zbus_error)?;
    }

    Ok(())
}

fn menu_item_id(id_offset: i32, index: usize) -> i32 {
    id_offset
        .checked_add(index as i32)
        .and_then(|value| value.checked_add(1))
        .expect("menu item id should not overflow")
}

fn status_from_view(visible: bool, status: TrayStatus) -> Status {
    if !visible {
        return Status::Passive;
    }

    match status {
        TrayStatus::Passive => Status::Passive,
        TrayStatus::Active => Status::Active,
        TrayStatus::Attention => Status::NeedsAttention,
    }
}

fn menu_status_from_tray_status(status: Status) -> MenuStatus {
    match status {
        Status::NeedsAttention => MenuStatus::Notice,
        Status::Passive | Status::Active => MenuStatus::Normal,
    }
}

fn icon_pixmap_from_icon(icon: &Icon) -> IconPixmap {
    let mut data = icon.rgba().to_vec();
    for pixel in data.chunks_exact_mut(4) {
        pixel.rotate_right(1);
    }

    IconPixmap {
        width: icon.width() as i32,
        height: icon.height() as i32,
        data,
    }
}

fn zbus_error(error: impl std::fmt::Display) -> Error {
    Error::Backend(format!("Linux tray backend error: {error}"))
}

fn to_fdo_error(error: Error) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(error.to_string())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Type, Serialize)]
#[zvariant(signature = "s")]
enum Category {
    ApplicationStatus,
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApplicationStatus => f.write_str("ApplicationStatus"),
        }
    }
}

impl From<Category> for Value<'_> {
    fn from(value: Category) -> Self {
        value.to_string().into()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Type, Serialize)]
#[zvariant(signature = "s")]
enum Status {
    Passive,
    Active,
    NeedsAttention,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Passive => f.write_str("Passive"),
            Self::Active => f.write_str("Active"),
            Self::NeedsAttention => f.write_str("NeedsAttention"),
        }
    }
}

impl From<Status> for Value<'_> {
    fn from(value: Status) -> Self {
        value.to_string().into()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Type, Serialize)]
#[zvariant(signature = "s")]
enum TextDirection {
    #[serde(rename = "ltr")]
    LeftToRight,
}

impl fmt::Display for TextDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LeftToRight => f.write_str("ltr"),
        }
    }
}

impl From<TextDirection> for Value<'_> {
    fn from(value: TextDirection) -> Self {
        value.to_string().into()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Type, Serialize)]
#[zvariant(signature = "s")]
enum MenuStatus {
    #[serde(rename = "normal")]
    Normal,
    #[serde(rename = "notice")]
    Notice,
}

impl fmt::Display for MenuStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Normal => f.write_str("normal"),
            Self::Notice => f.write_str("notice"),
        }
    }
}

impl From<MenuStatus> for Value<'_> {
    fn from(value: MenuStatus) -> Self {
        value.to_string().into()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Type, Deserialize)]
#[zvariant(signature = "s")]
enum Orientation {
    #[serde(alias = "horizontal")]
    Horizontal,
    #[serde(alias = "vertical")]
    Vertical,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Type, Value, Serialize)]
struct ToolTipData {
    icon_name: String,
    icon_pixmap: Vec<IconPixmap>,
    title: String,
    description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Type, Value, Serialize)]
struct IconPixmap {
    width: i32,
    height: i32,
    data: Vec<u8>,
}
