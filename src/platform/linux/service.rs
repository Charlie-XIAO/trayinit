use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use futures_util::{FutureExt, StreamExt, pin_mut, select};
use zbus::Connection;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedValue, Str, Value};

use super::menu::{MenuNode, MenuProperties, MenuTree, ROOT_ID, icon_rgba_to_argb};
use super::runtime;
use crate::backend::{BackendCommand, BackendRuntime};
use crate::{
    EventSink, TrayError, TrayEvent, TrayIconEventKind, TrayId, TrayResult, TrayState, TrayStatus,
};

const SNI_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";

static INSTANCE_COUNTER: AtomicUsize = AtomicUsize::new(1);

#[derive(Debug)]
pub struct PlatformOptions {
    pub own_dbus_name: bool,
    pub startup_policy: StartupPolicy,
    pub id: Option<String>,
}

/// Policy for andling tray registration failures at startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupPolicy {
    /// Soft start (default).
    ///
    /// Allows the tray to be created successfully even if no DBus
    /// StatusNotifierWathcer or StatusNotifierHost is present. The tray will
    /// automatically register and become available one they appear at runtime.
    Soft,
    /// Requires a StatusNotifierWatcher to be registered at startup.
    RequireWatcher,
    /// Requires a StatusNotifierWatcher and at least one active
    /// StatusNotifierHost to be registered at startup.
    RequireHost,
}

impl Default for PlatformOptions {
    fn default() -> Self {
        Self {
            own_dbus_name: true,
            startup_policy: StartupPolicy::Soft,
            id: None,
        }
    }
}

pub fn spawn(
    initial_state: TrayState,
    sink: Arc<dyn EventSink>,
    options: PlatformOptions,
    tray_id: TrayId,
) -> TrayResult<BackendRuntime> {
    let (command_tx, command_rx) = mpsc::channel();
    let (init_tx, init_rx) = mpsc::channel();

    let join = thread::Builder::new()
        .name("trayinit-linux-backend".into())
        .spawn(move || backend_thread(initial_state, sink, options, tray_id, command_rx, init_tx))
        .map_err(|err| TrayError::ThreadInit(err.to_string()))?;

    init_rx.recv().map_err(|_| {
        TrayError::ThreadInit("backend thread exited during initialization".into())
    })??;

    Ok(BackendRuntime::new(command_tx, Arc::new(|| {}), join))
}

fn backend_thread(
    initial_state: TrayState,
    sink: Arc<dyn EventSink>,
    options: PlatformOptions,
    tray_id: TrayId,
    command_rx: Receiver<BackendCommand>,
    init_tx: mpsc::Sender<TrayResult<()>>,
) {
    let result = runtime::run(run_backend(
        initial_state,
        sink,
        options,
        tray_id,
        command_rx,
        init_tx.clone(),
    ));
    if let Err(err) = result {
        let _ = init_tx.send(Err(err));
    }
}

async fn run_backend(
    initial_state: TrayState,
    sink: Arc<dyn EventSink>,
    options: PlatformOptions,
    tray_id: TrayId,
    command_rx: Receiver<BackendCommand>,
    init_tx: mpsc::Sender<TrayResult<()>>,
) {
    let generated_id = format!(
        "org.kde.StatusNotifierItem-{}-{}",
        std::process::id(),
        INSTANCE_COUNTER.fetch_add(1, Ordering::AcqRel)
    );
    let id = options
        .id
        .clone()
        .unwrap_or_else(|| tray_id.as_str().to_owned());

    let service = Arc::new(runtime::Mutex::new(Service::new(
        id,
        tray_id,
        initial_state,
        sink,
    )));

    let conn = match zbus::connection::Builder::session()
        .map_err(|err| TrayError::BackendUnavailable(err.to_string()))
        .and_then(|builder| {
            builder
                .serve_at(SNI_PATH, StatusNotifierItemIface::new(service.clone()))
                .map_err(|err| TrayError::BackendUnavailable(err.to_string()))
        })
        .and_then(|builder| {
            builder
                .serve_at(MENU_PATH, DbusMenuIface::new(service.clone()))
                .map_err(|err| TrayError::BackendUnavailable(err.to_string()))
        }) {
        Ok(builder) => match builder.build().await {
            Ok(conn) => conn,
            Err(err) => {
                let _ = init_tx.send(Err(TrayError::BackendUnavailable(err.to_string())));
                return;
            },
        },
        Err(err) => {
            let _ = init_tx.send(Err(err));
            return;
        },
    };

    let registration_name = if options.own_dbus_name {
        if let Err(err) = conn.request_name(generated_id.as_str()).await {
            let _ = init_tx.send(Err(TrayError::BackendUnavailable(err.to_string())));
            return;
        }
        generated_id
    } else {
        match conn.unique_name() {
            Some(name) => name.to_string(),
            None => {
                let _ = init_tx.send(Err(TrayError::BackendUnavailable(
                    "session bus did not assign a unique name".into(),
                )));
                return;
            },
        }
    };

    match options.startup_policy {
        StartupPolicy::Soft => {
            let _ = init_tx.send(Ok(()));
            update_watcher_status(&conn, &registration_name, &service).await;
        },
        StartupPolicy::RequireWatcher | StartupPolicy::RequireHost => {
            match check_watcher(&conn, &registration_name).await {
                Ok(availability) => {
                    let startup_result = startup_result(options.startup_policy, &availability);
                    if let Err(err) = startup_result {
                        let _ = init_tx.send(Err(err));
                        let _ = conn.close().await;
                        return;
                    }
                    let _ = init_tx.send(Ok(()));
                    emit_status(&service, availability.into_status()).await;
                },
                Err(err) => {
                    let _ = init_tx.send(Err(err));
                    let _ = conn.close().await;
                    return;
                },
            }
        },
    }

    let watcher_proxy = StatusNotifierWatcherProxy::new(&conn).await.ok();
    let mut host_registered = if let Some(proxy) = &watcher_proxy {
        proxy.receive_status_notifier_host_registered().await.ok()
    } else {
        None
    };
    let mut host_unregistered = if let Some(proxy) = &watcher_proxy {
        proxy.receive_status_notifier_host_unregistered().await.ok()
    } else {
        None
    };

    let mut watcher_changes = match zbus::fdo::DBusProxy::new(&conn).await {
        Ok(proxy) => proxy
            .receive_name_owner_changed_with_args(&[(0, WATCHER_NAME)])
            .await
            .ok(),
        Err(err) => {
            emit_status(
                &service,
                TrayStatus::BackendError(format!("failed to create DBus proxy: {err}")),
            )
            .await;
            None
        },
    };

    loop {
        match drain_commands(&command_rx, &conn, &service).await {
            CommandOutcome::Continue => {},
            CommandOutcome::Close => break,
        }

        let watcher_next = async {
            if let Some(changes) = &mut watcher_changes {
                changes.next().await
            } else {
                futures_util::future::pending().await
            }
        }
        .fuse();

        let host_reg_next = async {
            if let Some(stream) = &mut host_registered {
                stream.next().await
            } else {
                futures_util::future::pending().await
            }
        }
        .fuse();

        let host_unreg_next = async {
            if let Some(stream) = &mut host_unregistered {
                stream.next().await
            } else {
                futures_util::future::pending().await
            }
        }
        .fuse();

        let timeout = runtime::sleep(Duration::from_millis(25)).fuse();
        pin_mut!(watcher_next, host_reg_next, host_unreg_next, timeout);

        select! {
            event = watcher_next => {
                if let Some(event) = event
                    && let Ok(args) = event.args()
                {
                    if args.new_owner.as_ref().is_some() {
                        update_watcher_status(&conn, &registration_name, &service).await;
                        if let Some(proxy) = &watcher_proxy {
                            host_registered = proxy.receive_status_notifier_host_registered().await.ok();
                            host_unregistered = proxy.receive_status_notifier_host_unregistered().await.ok();
                        }
                    } else {
                        emit_status(
                            &service,
                            TrayStatus::WatcherUnavailable(
                                "StatusNotifierWatcher disappeared".into(),
                            ),
                        )
                        .await;
                        host_registered = None;
                        host_unregistered = None;
                    }
                }
            }
            reg = host_reg_next => {
                if reg.is_some() {
                    update_watcher_status(&conn, &registration_name, &service).await;
                }
            }
            unreg = host_unreg_next => {
                if unreg.is_some() {
                    update_watcher_status(&conn, &registration_name, &service).await;
                }
            }
            _ = timeout => {}
        }
    }

    let _ = conn.close().await;
}

enum CommandOutcome {
    Continue,
    Close,
}

async fn drain_commands(
    command_rx: &Receiver<BackendCommand>,
    conn: &Connection,
    service: &Arc<runtime::Mutex<Service>>,
) -> CommandOutcome {
    loop {
        match command_rx.try_recv() {
            Ok(BackendCommand::SetState(state)) => {
                if let Err(err) = apply_state(conn, service, state).await {
                    emit_status(service, TrayStatus::BackendError(err.to_string())).await;
                }
            },
            Ok(BackendCommand::Close) => return CommandOutcome::Close,
            Err(TryRecvError::Empty) => return CommandOutcome::Continue,
            Err(TryRecvError::Disconnected) => return CommandOutcome::Close,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Availability {
    Available,
    WatcherUnavailable(String),
    NoHost(String),
}

impl Availability {
    fn into_status(self) -> TrayStatus {
        match self {
            Self::Available => TrayStatus::Available,
            Self::WatcherUnavailable(message) => TrayStatus::WatcherUnavailable(message),
            Self::NoHost(message) => TrayStatus::NoHost(message),
        }
    }
}

fn startup_result(policy: StartupPolicy, availability: &Availability) -> TrayResult<()> {
    match (policy, availability) {
        (StartupPolicy::Soft, _) => Ok(()),
        (StartupPolicy::RequireWatcher, Availability::WatcherUnavailable(message)) => {
            Err(TrayError::BackendUnavailable(message.clone()))
        },
        (StartupPolicy::RequireWatcher, _) => Ok(()),
        (StartupPolicy::RequireHost, Availability::Available) => Ok(()),
        (StartupPolicy::RequireHost, Availability::WatcherUnavailable(message))
        | (StartupPolicy::RequireHost, Availability::NoHost(message)) => {
            Err(TrayError::BackendUnavailable(message.clone()))
        },
    }
}

async fn update_watcher_status(
    conn: &Connection,
    registration_name: &str,
    service: &Arc<runtime::Mutex<Service>>,
) {
    match check_watcher(conn, registration_name).await {
        Ok(availability) => emit_status(service, availability.into_status()).await,
        Err(err) => emit_status(service, TrayStatus::BackendError(err.to_string())).await,
    }
}

async fn check_watcher(conn: &Connection, registration_name: &str) -> TrayResult<Availability> {
    let proxy = match StatusNotifierWatcherProxy::new(conn).await {
        Ok(proxy) => proxy,
        Err(err) => {
            let fdo_err: zbus::fdo::Error = err.into();
            if matches!(fdo_err, zbus::fdo::Error::ServiceUnknown(_)) {
                return Ok(Availability::WatcherUnavailable(
                    "StatusNotifierWatcher is not available".into(),
                ));
            }
            return Err(TrayError::BackendUnavailable(format!(
                "failed to create watcher proxy: {fdo_err}"
            )));
        },
    };

    match proxy.register_status_notifier_item(registration_name).await {
        Ok(()) => {},
        Err(err) => {
            let fdo_err: zbus::fdo::Error = err.into();
            if matches!(fdo_err, zbus::fdo::Error::ServiceUnknown(_)) {
                return Ok(Availability::WatcherUnavailable(
                    "StatusNotifierWatcher is not available".into(),
                ));
            } else {
                return Err(TrayError::BackendUnavailable(format!(
                    "failed to register StatusNotifierItem: {fdo_err}"
                )));
            }
        },
    }

    match proxy.is_status_notifier_host_registered().await {
        Ok(true) => Ok(Availability::Available),
        Ok(false) => Ok(Availability::NoHost(
            "no StatusNotifierHost is registered".into(),
        )),
        Err(err) => Err(watcher_error(
            err,
            "failed to query StatusNotifierHost registration",
        )),
    }
}

fn watcher_error(err: zbus::Error, context: &str) -> TrayError {
    let fdo_err: zbus::fdo::Error = err.into();
    if matches!(fdo_err, zbus::fdo::Error::ServiceUnknown(_)) {
        TrayError::BackendUnavailable("StatusNotifierWatcher is not available".into())
    } else {
        TrayError::BackendUnavailable(format!("{context}: {fdo_err}"))
    }
}

async fn emit_status(service: &Arc<runtime::Mutex<Service>>, status: TrayStatus) {
    let (tray_id, sink) = {
        let service = service.lock().await;
        (service.tray_id.clone(), service.sink.clone())
    };
    sink.send(TrayEvent::StatusChanged { tray_id, status });
}

async fn apply_state(
    conn: &Connection,
    service: &Arc<runtime::Mutex<Service>>,
    state: TrayState,
) -> zbus::Result<()> {
    let changes = {
        let mut service = service.lock().await;
        service.set_state(state)
    };

    let sni = conn
        .object_server()
        .interface::<_, StatusNotifierItemIface>(SNI_PATH)
        .await?;
    let menu = conn
        .object_server()
        .interface::<_, DbusMenuIface>(MENU_PATH)
        .await?;

    if changes.title {
        StatusNotifierItemIface::new_title(sni.signal_emitter()).await?;
    }
    if changes.icon {
        StatusNotifierItemIface::new_icon(sni.signal_emitter()).await?;
    }
    if changes.tooltip {
        StatusNotifierItemIface::new_tool_tip(sni.signal_emitter()).await?;
    }
    if changes.status {
        let status = {
            let service = service.lock().await;
            service.sni_status()
        };
        StatusNotifierItemIface::new_status(sni.signal_emitter(), &status).await?;
    }
    match changes.menu {
        MenuChange::None => {},
        MenuChange::LayoutUpdated => {
            DbusMenuIface::layout_updated(menu.signal_emitter(), changes.revision, ROOT_ID).await?;
        },
        MenuChange::PropertiesUpdated { updated, removed } => {
            DbusMenuIface::items_properties_updated(menu.signal_emitter(), updated, removed)
                .await?;
        },
    }

    Ok(())
}

#[derive(Debug)]
struct StateChanges {
    title: bool,
    icon: bool,
    tooltip: bool,
    status: bool,
    menu: MenuChange,
    revision: u32,
}

#[derive(Debug)]
enum MenuChange {
    None,
    LayoutUpdated,
    PropertiesUpdated {
        updated: Vec<(i32, HashMap<String, OwnedValue>)>,
        removed: Vec<(i32, Vec<String>)>,
    },
}

#[derive(Default)]
struct FlatMenu {
    properties: HashMap<i32, HashMap<String, OwnedValue>>,
    children: HashMap<i32, Vec<i32>>,
}

impl FlatMenu {
    fn from_tree(tree: &MenuTree) -> Self {
        let mut flat = Self::default();
        flatten_node(&tree.root, &mut flat);
        flat
    }
}

fn flatten_node(node: &MenuNode, flat: &mut FlatMenu) {
    flat.properties
        .insert(node.id, properties_to_dbus(node.properties.clone()));
    flat.children.insert(
        node.id,
        node.children.iter().map(|child| child.id).collect(),
    );

    for child in &node.children {
        flatten_node(child, flat);
    }
}

fn diff_menu(old: &MenuTree, new: &MenuTree) -> MenuChange {
    let old = FlatMenu::from_tree(old);
    let new = FlatMenu::from_tree(new);

    if old.children != new.children {
        return MenuChange::LayoutUpdated;
    }

    let mut updated = Vec::new();
    let mut removed = Vec::new();

    for (id, new_props) in &new.properties {
        if let Some(old_props) = old.properties.get(id) {
            let (item_updated, item_removed) = diff_properties(old_props, new_props);
            if !item_updated.is_empty() {
                updated.push((*id, item_updated));
            }
            if !item_removed.is_empty() {
                removed.push((*id, item_removed));
            }
        } else if !new_props.is_empty() {
            updated.push((*id, new_props.clone()));
        }
    }

    if updated.is_empty() && removed.is_empty() {
        MenuChange::None
    } else {
        MenuChange::PropertiesUpdated { updated, removed }
    }
}

fn diff_properties(
    old: &HashMap<String, OwnedValue>,
    new: &HashMap<String, OwnedValue>,
) -> (HashMap<String, OwnedValue>, Vec<String>) {
    let mut updated = HashMap::new();
    let mut removed = Vec::new();

    for (key, new_value) in new {
        if old.get(key) != Some(new_value) {
            updated.insert(key.clone(), new_value.clone());
        }
    }

    for key in old.keys() {
        if !new.contains_key(key) {
            removed.push(key.clone());
        }
    }

    (updated, removed)
}

struct Service {
    identity: String,
    tray_id: TrayId,
    state: TrayState,
    menu: MenuTree,
    menu_id_base: i32,
    sink: Arc<dyn EventSink>,
    revision: u32,
}

impl Service {
    fn new(identity: String, tray_id: TrayId, state: TrayState, sink: Arc<dyn EventSink>) -> Self {
        let menu =
            MenuTree::from_menu(state.menu.as_ref(), 0).unwrap_or_else(|_| MenuTree::empty(0));
        Self {
            identity,
            tray_id,
            state,
            menu,
            menu_id_base: 0,
            sink,
            revision: 0,
        }
    }

    fn set_state(&mut self, state: TrayState) -> StateChanges {
        let old = self.state.clone();
        let menu = if old.menu != state.menu {
            let next_revision = self.revision.wrapping_add(1);
            let new_menu = MenuTree::from_menu_with_base(
                state.menu.as_ref(),
                self.menu_id_base,
                next_revision,
            )
            .unwrap_or_else(|_| MenuTree::empty(next_revision));
            let menu_change = diff_menu(&self.menu, &new_menu);

            match menu_change {
                MenuChange::LayoutUpdated => {
                    let max_id = self.menu.max_id();
                    let mut next_base = (self.menu_id_base + max_id) & 0x7FFFFFFF;
                    if next_base == 0 {
                        next_base = 1;
                    }
                    self.menu_id_base = next_base;

                    let new_menu_with_new_base = MenuTree::from_menu_with_base(
                        state.menu.as_ref(),
                        self.menu_id_base,
                        next_revision,
                    )
                    .unwrap_or_else(|_| MenuTree::empty(next_revision));
                    self.revision = next_revision;
                    self.menu = new_menu_with_new_base;
                    MenuChange::LayoutUpdated
                },
                MenuChange::PropertiesUpdated { .. } | MenuChange::None => {
                    let mut final_menu = new_menu;
                    final_menu.revision = self.revision;
                    self.menu = final_menu;
                    menu_change
                },
            }
        } else {
            MenuChange::None
        };
        self.state = state;

        StateChanges {
            title: old.title != self.state.title,
            icon: old.icon != self.state.icon,
            tooltip: old.tooltip != self.state.tooltip,
            status: old.visible != self.state.visible,
            menu,
            revision: self.revision,
        }
    }

    fn sni_status(&self) -> String {
        if self.state.visible {
            "Active".into()
        } else {
            "Passive".into()
        }
    }

    fn icon_pixmap(&self) -> Vec<SniIconPixmap> {
        self.state
            .icon
            .as_ref()
            .map(|icon| {
                vec![(
                    icon.width() as i32,
                    icon.height() as i32,
                    icon_rgba_to_argb(icon.rgba()),
                )]
            })
            .unwrap_or_default()
    }

    fn tooltip(&self) -> SniToolTip {
        (
            String::new(),
            Vec::new(),
            self.state.title.clone().unwrap_or_default(),
            self.state.tooltip.clone().unwrap_or_default(),
        )
    }
}

struct StatusNotifierItemIface {
    service: Arc<runtime::Mutex<Service>>,
}

impl StatusNotifierItemIface {
    fn new(service: Arc<runtime::Mutex<Service>>) -> Self {
        Self { service }
    }
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItemIface {
    fn context_menu(&self, _x: i32, _y: i32) -> zbus::fdo::Result<()> {
        Err(zbus::fdo::Error::UnknownMethod(
            "ContextMenu is not supported; use DBusMenu".into(),
        ))
    }

    async fn activate(&self, x: i32, y: i32) -> zbus::fdo::Result<()> {
        let (has_menu, tray_id, sink) = {
            let service = self.service.lock().await;
            (
                service.state.menu.is_some(),
                service.tray_id.clone(),
                service.sink.clone(),
            )
        };

        if has_menu {
            return Err(zbus::fdo::Error::UnknownMethod("ItemIsMenu".into()));
        }

        sink.send(TrayEvent::IconActivated {
            tray_id,
            kind: TrayIconEventKind::PrimaryClick,
            position: Some(crate::PhysicalPosition { x, y }),
            rect: None,
        });
        Ok(())
    }

    async fn secondary_activate(&self, x: i32, y: i32) -> zbus::fdo::Result<()> {
        let (tray_id, sink) = {
            let service = self.service.lock().await;
            (service.tray_id.clone(), service.sink.clone())
        };
        sink.send(TrayEvent::IconActivated {
            tray_id,
            kind: TrayIconEventKind::SecondaryClick,
            position: Some(crate::PhysicalPosition { x, y }),
            rect: None,
        });
        Ok(())
    }

    fn scroll(&self, _delta: i32, _orientation: String) -> zbus::fdo::Result<()> {
        Ok(())
    }

    #[zbus(property)]
    async fn category(&self) -> zbus::fdo::Result<String> {
        Ok("ApplicationStatus".into())
    }

    #[zbus(property)]
    async fn id(&self) -> zbus::fdo::Result<String> {
        let service = self.service.lock().await;
        Ok(service.identity.clone())
    }

    #[zbus(property)]
    async fn title(&self) -> zbus::fdo::Result<String> {
        let service = self.service.lock().await;
        Ok(service.state.title.clone().unwrap_or_default())
    }

    #[zbus(property)]
    async fn status(&self) -> zbus::fdo::Result<String> {
        let service = self.service.lock().await;
        Ok(service.sni_status())
    }

    #[zbus(property)]
    async fn window_id(&self) -> zbus::fdo::Result<i32> {
        Ok(0)
    }

    #[zbus(property)]
    async fn icon_theme_path(&self) -> zbus::fdo::Result<String> {
        Ok(String::new())
    }

    #[zbus(property)]
    fn menu(&self) -> zbus::fdo::Result<ObjectPath<'static>> {
        Ok(ObjectPath::from_static_str_unchecked(MENU_PATH))
    }

    #[zbus(property)]
    async fn item_is_menu(&self) -> zbus::fdo::Result<bool> {
        let service = self.service.lock().await;
        Ok(service.state.menu.is_some())
    }

    #[zbus(property)]
    async fn icon_name(&self) -> zbus::fdo::Result<String> {
        Ok(String::new())
    }

    #[zbus(property)]
    async fn icon_pixmap(&self) -> zbus::fdo::Result<Vec<SniIconPixmap>> {
        let service = self.service.lock().await;
        Ok(service.icon_pixmap())
    }

    #[zbus(property)]
    async fn overlay_icon_name(&self) -> zbus::fdo::Result<String> {
        Ok(String::new())
    }

    #[zbus(property)]
    async fn overlay_icon_pixmap(&self) -> zbus::fdo::Result<Vec<SniIconPixmap>> {
        Ok(Vec::new())
    }

    #[zbus(property)]
    async fn attention_icon_name(&self) -> zbus::fdo::Result<String> {
        Ok(String::new())
    }

    #[zbus(property)]
    async fn attention_icon_pixmap(&self) -> zbus::fdo::Result<Vec<SniIconPixmap>> {
        Ok(Vec::new())
    }

    #[zbus(property)]
    async fn attention_movie_name(&self) -> zbus::fdo::Result<String> {
        Ok(String::new())
    }

    #[zbus(property)]
    async fn tool_tip(&self) -> zbus::fdo::Result<SniToolTip> {
        let service = self.service.lock().await;
        Ok(service.tooltip())
    }

    #[zbus(signal)]
    async fn new_title(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_icon(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_tool_tip(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_status(ctxt: &SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
}

struct DbusMenuIface {
    service: Arc<runtime::Mutex<Service>>,
}

impl DbusMenuIface {
    fn new(service: Arc<runtime::Mutex<Service>>) -> Self {
        Self { service }
    }
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl DbusMenuIface {
    async fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<(u32, DbusMenuLayout)> {
        let service = self.service.lock().await;
        let layout = service
            .menu
            .layout(parent_id, recursion_depth, &property_names)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("parentId not found".into()))?;
        Ok((service.menu.revision, layout_to_dbus(layout)))
    }

    async fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<Vec<(i32, HashMap<String, OwnedValue>)>> {
        let service = self.service.lock().await;
        Ok(ids
            .into_iter()
            .filter_map(|id| {
                service
                    .menu
                    .properties(id, &property_names)
                    .map(|props| (id, properties_to_dbus(props)))
            })
            .filter(|(_, props)| !props.is_empty())
            .collect())
    }

    async fn get_property(&self, id: i32, name: String) -> zbus::fdo::Result<OwnedValue> {
        let service = self.service.lock().await;
        let props = service
            .menu
            .properties(id, std::slice::from_ref(&name))
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("id not found".into()))?;
        properties_to_dbus(props)
            .remove(&name)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("property not found".into()))
    }

    async fn event(
        &self,
        id: i32,
        event_id: String,
        _data: OwnedValue,
        _timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        if event_id != "clicked" {
            return Ok(());
        }

        let (tray_id, item_id, sink) = {
            let service = self.service.lock().await;
            let item_id = service
                .menu
                .action_for(id)
                .ok_or_else(|| zbus::fdo::Error::InvalidArgs("id not found".into()))?;
            (service.tray_id.clone(), item_id, service.sink.clone())
        };

        sink.send(TrayEvent::MenuItemActivated { tray_id, item_id });
        Ok(())
    }

    async fn event_group(
        &self,
        events: Vec<(i32, String, OwnedValue, u32)>,
    ) -> zbus::fdo::Result<Vec<i32>> {
        let mut not_found = Vec::new();
        for (id, event_id, data, timestamp) in events {
            if self.event(id, event_id, data, timestamp).await.is_err() {
                not_found.push(id);
            }
        }
        Ok(not_found)
    }

    async fn about_to_show(&self, _id: i32) -> zbus::fdo::Result<bool> {
        Ok(false)
    }

    async fn about_to_show_group(&self, _ids: Vec<i32>) -> zbus::fdo::Result<(Vec<i32>, Vec<i32>)> {
        Ok((Vec::new(), Vec::new()))
    }

    #[zbus(property)]
    fn version(&self) -> zbus::fdo::Result<u32> {
        Ok(3)
    }

    #[zbus(property)]
    fn text_direction(&self) -> zbus::fdo::Result<String> {
        Ok("ltr".into())
    }

    #[zbus(property)]
    fn status(&self) -> zbus::fdo::Result<String> {
        Ok("normal".into())
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> zbus::fdo::Result<Vec<String>> {
        Ok(Vec::new())
    }

    #[zbus(signal)]
    async fn items_properties_updated(
        ctxt: &SignalEmitter<'_>,
        updated_props: Vec<(i32, HashMap<String, OwnedValue>)>,
        removed_props: Vec<(i32, Vec<String>)>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn layout_updated(
        ctxt: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}

type SniIconPixmap = (i32, i32, Vec<u8>);
type SniToolTip = (String, Vec<SniIconPixmap>, String, String);
type DbusMenuLayout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

#[zbus::proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
trait StatusNotifierWatcher {
    async fn register_status_notifier_item(&self, service: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> zbus::Result<bool>;

    #[zbus(signal)]
    fn status_notifier_host_registered(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn status_notifier_host_unregistered(&self) -> zbus::Result<()>;
}

fn layout_to_dbus(node: MenuNode) -> DbusMenuLayout {
    (
        node.id,
        properties_to_dbus(node.properties),
        node.children
            .into_iter()
            .map(layout_to_dbus)
            .map(|child| {
                OwnedValue::try_from(Value::from(child))
                    .expect("DBusMenu layout must be serializable")
            })
            .collect(),
    )
}

fn properties_to_dbus(properties: MenuProperties) -> HashMap<String, OwnedValue> {
    if properties.is_empty() {
        return HashMap::new();
    }

    let mut map = HashMap::new();
    insert_str(&mut map, "type", properties.item_type);
    if let Some(label) = properties.label {
        map.insert("label".into(), OwnedValue::from(Str::from(label)));
    }
    if let Some(enabled) = properties.enabled {
        map.insert("enabled".into(), OwnedValue::from(enabled));
    }
    if let Some(visible) = properties.visible {
        map.insert("visible".into(), OwnedValue::from(visible));
    }
    insert_str(&mut map, "toggle-type", properties.toggle_type);
    if let Some(toggle_state) = properties.toggle_state {
        map.insert("toggle-state".into(), OwnedValue::from(toggle_state));
    }
    insert_str(&mut map, "children-display", properties.children_display);
    map
}

fn insert_str(
    map: &mut HashMap<String, OwnedValue>,
    name: &'static str,
    value: Option<&'static str>,
) {
    if let Some(value) = value {
        map.insert(name.into(), OwnedValue::from(Str::from_static(value)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Menu, MenuNode, TrayState, channel};

    #[test]
    fn default_linux_options_soft_start_and_own_name() {
        let options = PlatformOptions::default();

        assert!(options.own_dbus_name);
        assert_eq!(options.startup_policy, StartupPolicy::Soft);
        assert_eq!(options.id, None);
    }

    #[test]
    fn availability_maps_to_status() {
        assert_eq!(Availability::Available.into_status(), TrayStatus::Available);
        assert_eq!(
            Availability::WatcherUnavailable("offline".into()).into_status(),
            TrayStatus::WatcherUnavailable("offline".into())
        );
        assert_eq!(
            Availability::NoHost("no host".into()).into_status(),
            TrayStatus::NoHost("no host".into())
        );
    }

    #[test]
    fn soft_start_accepts_all_availability_states() {
        assert!(startup_result(StartupPolicy::Soft, &Availability::Available).is_ok());
        assert!(
            startup_result(
                StartupPolicy::Soft,
                &Availability::WatcherUnavailable("offline".into())
            )
            .is_ok()
        );
        assert!(
            startup_result(StartupPolicy::Soft, &Availability::NoHost("no host".into())).is_ok()
        );
    }

    #[test]
    fn require_watcher_accepts_registered_no_host() {
        assert!(
            startup_result(
                StartupPolicy::RequireWatcher,
                &Availability::NoHost("no host".into())
            )
            .is_ok()
        );
    }

    #[test]
    fn require_watcher_rejects_missing_watcher() {
        assert!(matches!(
            startup_result(
                StartupPolicy::RequireWatcher,
                &Availability::WatcherUnavailable("offline".into())
            ),
            Err(TrayError::BackendUnavailable(message)) if message == "offline"
        ));
    }

    #[test]
    fn require_host_only_accepts_available() {
        assert!(startup_result(StartupPolicy::RequireHost, &Availability::Available).is_ok());
        assert!(matches!(
            startup_result(
                StartupPolicy::RequireHost,
                &Availability::NoHost("no host".into())
            ),
            Err(TrayError::BackendUnavailable(message)) if message == "no host"
        ));
    }

    #[test]
    fn linux_id_option_controls_sni_id_property() {
        let (sink, _events) = channel();
        let service = Service::new(
            "custom-id".into(),
            TrayId::new("test"),
            TrayState::new(),
            Arc::new(sink),
        );

        assert_eq!(service.identity, "custom-id");
    }

    #[test]
    fn check_state_change_updates_toggle_state_without_bumping_revision() {
        let (sink, _events) = channel();
        let initial =
            TrayState::new().with_menu(Menu::new([MenuNode::check("sync", "Sync", false)]));
        let mut service = Service::new("tray".into(), TrayId::new("test"), initial, Arc::new(sink));

        let changes = service.set_state(
            TrayState::new().with_menu(Menu::new([MenuNode::check("sync", "Sync", true)])),
        );

        assert_eq!(service.revision, 0);
        match changes.menu {
            MenuChange::PropertiesUpdated { updated, removed } => {
                assert!(removed.is_empty());
                assert_eq!(updated.len(), 1);
                assert_eq!(updated[0].0, 1);
                assert_eq!(
                    updated[0].1.get("toggle-state"),
                    Some(&OwnedValue::from(1i32))
                );
            },
            other => panic!("expected property update, got {other:?}"),
        }
    }

    #[test]
    fn label_change_updates_label_property() {
        let (sink, _events) = channel();
        let initial = TrayState::new().with_menu(Menu::new([MenuNode::item("open", "Open")]));
        let mut service = Service::new("tray".into(), TrayId::new("test"), initial, Arc::new(sink));

        let changes = service.set_state(
            TrayState::new().with_menu(Menu::new([MenuNode::item("open", "Open file")])),
        );

        match changes.menu {
            MenuChange::PropertiesUpdated { updated, removed } => {
                assert!(removed.is_empty());
                assert_eq!(updated.len(), 1);
                assert_eq!(updated[0].0, 1);
                assert_eq!(
                    updated[0].1.get("label"),
                    Some(&OwnedValue::from(Str::from("Open file")))
                );
            },
            other => panic!("expected property update, got {other:?}"),
        }
    }

    #[test]
    fn item_to_check_updates_toggle_properties() {
        let (sink, _events) = channel();
        let initial = TrayState::new().with_menu(Menu::new([MenuNode::item("sync", "Sync")]));
        let mut service = Service::new("tray".into(), TrayId::new("test"), initial, Arc::new(sink));

        let changes = service.set_state(
            TrayState::new().with_menu(Menu::new([MenuNode::check("sync", "Sync", true)])),
        );

        match changes.menu {
            MenuChange::PropertiesUpdated { updated, removed } => {
                assert!(removed.is_empty());
                assert_eq!(updated.len(), 1);
                assert_eq!(updated[0].1.get("toggle-type").map(|_| ()), Some(()));
                assert_eq!(
                    updated[0].1.get("toggle-state"),
                    Some(&OwnedValue::from(1i32))
                );
            },
            other => panic!("expected property update, got {other:?}"),
        }
    }

    #[test]
    fn check_to_item_removes_toggle_properties() {
        let (sink, _events) = channel();
        let initial =
            TrayState::new().with_menu(Menu::new([MenuNode::check("sync", "Sync", true)]));
        let mut service = Service::new("tray".into(), TrayId::new("test"), initial, Arc::new(sink));

        let changes = service
            .set_state(TrayState::new().with_menu(Menu::new([MenuNode::item("sync", "Sync")])));

        match changes.menu {
            MenuChange::PropertiesUpdated { updated, removed } => {
                assert!(updated.is_empty());
                assert_eq!(removed.len(), 1);
                assert_eq!(removed[0].0, 1);
                let mut properties = removed[0].1.clone();
                properties.sort();
                assert_eq!(properties, ["toggle-state", "toggle-type"]);
            },
            other => panic!("expected property update, got {other:?}"),
        }
    }

    #[test]
    fn layout_menu_update_bumps_revision() {
        let (sink, _events) = channel();
        let initial = TrayState::new().with_menu(Menu::new([MenuNode::item("open", "Open")]));
        let mut service = Service::new("tray".into(), TrayId::new("test"), initial, Arc::new(sink));

        let changes = service.set_state(TrayState::new().with_menu(Menu::new([
            MenuNode::item("open", "Open"),
            MenuNode::item("quit", "Quit"),
        ])));

        assert_eq!(service.revision, 1);
        assert_eq!(changes.revision, 1);
        assert!(matches!(changes.menu, MenuChange::LayoutUpdated));
    }

    #[test]
    fn submenu_child_addition_is_layout_update() {
        let (sink, _events) = channel();
        let initial = TrayState::new().with_menu(Menu::new([MenuNode::submenu(
            "More",
            [MenuNode::item("about", "About")],
        )]));
        let mut service = Service::new("tray".into(), TrayId::new("test"), initial, Arc::new(sink));

        let changes =
            service.set_state(TrayState::new().with_menu(Menu::new([MenuNode::submenu(
                "More",
                [
                    MenuNode::item("about", "About"),
                    MenuNode::item("help", "Help"),
                ],
            )])));

        assert!(matches!(changes.menu, MenuChange::LayoutUpdated));
    }

    #[test]
    fn action_id_only_change_updates_mapping_without_signal() {
        let (sink, _events) = channel();
        let initial = TrayState::new().with_menu(Menu::new([MenuNode::item("open", "Open")]));
        let mut service = Service::new("tray".into(), TrayId::new("test"), initial, Arc::new(sink));

        let changes = service
            .set_state(TrayState::new().with_menu(Menu::new([MenuNode::item("open-2", "Open")])));

        assert!(matches!(changes.menu, MenuChange::None));
        assert_eq!(service.menu.action_for(1).unwrap().as_str(), "open-2");
    }

    #[test]
    fn layout_update_offsets_menu_ids() {
        let (sink, _events) = channel();
        let initial = TrayState::new().with_menu(Menu::new([
            MenuNode::item("open", "Open"),
            MenuNode::item("quit", "Quit"),
        ]));
        let mut service = Service::new("tray".into(), TrayId::new("test"), initial, Arc::new(sink));

        assert_eq!(service.menu.max_id(), 2);

        // Add an item to trigger layout update
        let changes = service.set_state(TrayState::new().with_menu(Menu::new([
            MenuNode::item("about", "About"),
            MenuNode::item("help", "Help"),
            MenuNode::item("quit", "Quit"),
        ])));

        assert!(matches!(changes.menu, MenuChange::LayoutUpdated));
        assert_eq!(service.menu_id_base, 2);
        assert_eq!(service.menu.max_id(), 5);

        assert_eq!(service.menu.action_for(3).unwrap().as_str(), "about");
        assert_eq!(service.menu.action_for(4).unwrap().as_str(), "help");
        assert_eq!(service.menu.action_for(5).unwrap().as_str(), "quit");

        assert!(service.menu.action_for(1).is_none());
        assert!(service.menu.action_for(2).is_none());
    }
}
