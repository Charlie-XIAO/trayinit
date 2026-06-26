use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::runtime::Builder as RuntimeBuilder;
use zbus::Connection;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedValue, Str, Value};

use super::menu::{LinuxMenu, LinuxMenuNode, LinuxMenuProperties, ROOT_ID, icon_rgba_to_argb};
use crate::backend::{BackendCommand, BackendProxy};
use crate::{
    EventSink, TrayError, TrayEvent, TrayIconEventKind, TrayResult, TrayState, TrayStatus,
};

const SNI_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";

static INSTANCE_COUNTER: AtomicUsize = AtomicUsize::new(1);

pub(crate) fn spawn(
    initial_state: TrayState,
    sink: Arc<dyn EventSink>,
) -> TrayResult<BackendProxy> {
    let (command_tx, command_rx) = mpsc::channel();
    let (init_tx, init_rx) = mpsc::channel();

    let join = thread::Builder::new()
        .name("trayinit-linux-backend".into())
        .spawn(move || backend_thread(initial_state, sink, command_rx, init_tx))
        .map_err(|err| TrayError::ThreadInit(err.to_string()))?;

    init_rx.recv().map_err(|_| {
        TrayError::ThreadInit("backend thread exited during initialization".into())
    })??;

    Ok(BackendProxy::new(command_tx, Arc::new(|| {}), join))
}

fn backend_thread(
    initial_state: TrayState,
    sink: Arc<dyn EventSink>,
    command_rx: Receiver<BackendCommand>,
    init_tx: mpsc::Sender<TrayResult<()>>,
) {
    let runtime = match RuntimeBuilder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = init_tx.send(Err(TrayError::ThreadInit(err.to_string())));
            return;
        },
    };

    runtime.block_on(run_backend(initial_state, sink, command_rx, init_tx));
}

async fn run_backend(
    initial_state: TrayState,
    sink: Arc<dyn EventSink>,
    command_rx: Receiver<BackendCommand>,
    init_tx: mpsc::Sender<TrayResult<()>>,
) {
    let identity = format!(
        "org.kde.StatusNotifierItem-{}-{}",
        std::process::id(),
        INSTANCE_COUNTER.fetch_add(1, Ordering::AcqRel)
    );

    let service = Arc::new(tokio::sync::Mutex::new(LinuxService::new(
        identity.clone(),
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

    if let Err(err) = conn.request_name(identity.as_str()).await {
        let _ = init_tx.send(Err(TrayError::BackendUnavailable(err.to_string())));
        return;
    }

    let _ = init_tx.send(Ok(()));

    register_with_watcher(&conn, &identity, &service).await;

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

        if let Some(changes) = &mut watcher_changes {
            tokio::select! {
                event = changes.next() => {
                    if let Some(event) = event {
                        if let Ok(args) = event.args() {
                            if args.new_owner.as_ref().is_some() {
                                register_with_watcher(&conn, &identity, &service).await;
                            } else {
                                emit_status(
                                    &service,
                                    TrayStatus::TemporarilyUnavailable(
                                        "StatusNotifierWatcher disappeared".into(),
                                    ),
                                ).await;
                            }
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(25)) => {}
            }
        } else {
            tokio::time::sleep(Duration::from_millis(25)).await;
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
    service: &Arc<tokio::sync::Mutex<LinuxService>>,
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

async fn register_with_watcher(
    conn: &Connection,
    identity: &str,
    service: &Arc<tokio::sync::Mutex<LinuxService>>,
) {
    let proxy = match StatusNotifierWatcherProxy::new(conn).await {
        Ok(proxy) => proxy,
        Err(err) => {
            emit_status(
                service,
                TrayStatus::BackendError(format!("failed to create watcher proxy: {err}")),
            )
            .await;
            return;
        },
    };

    match proxy.register_status_notifier_item(identity).await {
        Ok(()) => emit_status(service, TrayStatus::Available).await,
        Err(err) => {
            let fdo_err: zbus::fdo::Error = err.into();
            if matches!(fdo_err, zbus::fdo::Error::ServiceUnknown(_)) {
                emit_status(
                    service,
                    TrayStatus::TemporarilyUnavailable(
                        "StatusNotifierWatcher is not available".into(),
                    ),
                )
                .await;
            } else {
                emit_status(
                    service,
                    TrayStatus::BackendError(format!(
                        "failed to register StatusNotifierItem: {fdo_err}"
                    )),
                )
                .await;
            }
        },
    }
}

async fn emit_status(service: &Arc<tokio::sync::Mutex<LinuxService>>, status: TrayStatus) {
    let sink = {
        let service = service.lock().await;
        service.sink.clone()
    };
    sink.send(TrayEvent::StatusChanged { status });
}

async fn apply_state(
    conn: &Connection,
    service: &Arc<tokio::sync::Mutex<LinuxService>>,
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
    if changes.menu {
        DbusMenuIface::layout_updated(menu.signal_emitter(), changes.revision, ROOT_ID).await?;
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct LinuxStateChanges {
    title: bool,
    icon: bool,
    tooltip: bool,
    status: bool,
    menu: bool,
    revision: u32,
}

struct LinuxService {
    identity: String,
    state: TrayState,
    menu: LinuxMenu,
    sink: Arc<dyn EventSink>,
    revision: u32,
}

impl LinuxService {
    fn new(identity: String, state: TrayState, sink: Arc<dyn EventSink>) -> Self {
        let menu =
            LinuxMenu::from_menu(state.menu.as_ref(), 0).unwrap_or_else(|_| LinuxMenu::empty(0));
        Self {
            identity,
            state,
            menu,
            sink,
            revision: 0,
        }
    }

    fn set_state(&mut self, state: TrayState) -> LinuxStateChanges {
        let old = self.state.clone();
        let menu_changed = old.menu != state.menu;
        if menu_changed {
            self.revision = self.revision.wrapping_add(1);
            self.menu = LinuxMenu::from_menu(state.menu.as_ref(), self.revision)
                .unwrap_or_else(|_| LinuxMenu::empty(self.revision));
        }
        self.state = state;

        LinuxStateChanges {
            title: old.title != self.state.title,
            icon: old.icon != self.state.icon,
            tooltip: old.tooltip != self.state.tooltip,
            status: old.visible != self.state.visible,
            menu: menu_changed,
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
    service: Arc<tokio::sync::Mutex<LinuxService>>,
}

impl StatusNotifierItemIface {
    fn new(service: Arc<tokio::sync::Mutex<LinuxService>>) -> Self {
        Self { service }
    }
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItemIface {
    fn context_menu(&self, _x: i32, _y: i32) -> zbus::fdo::Result<()> {
        Ok(())
    }

    async fn activate(&self, x: i32, y: i32) -> zbus::fdo::Result<()> {
        let (has_menu, sink) = {
            let service = self.service.lock().await;
            (service.state.menu.is_some(), service.sink.clone())
        };

        if has_menu {
            return Err(zbus::fdo::Error::UnknownMethod("ItemIsMenu".into()));
        }

        sink.send(TrayEvent::IconActivated {
            kind: TrayIconEventKind::PrimaryClick,
            position: Some(crate::PhysicalPosition { x, y }),
            rect: None,
        });
        Ok(())
    }

    async fn secondary_activate(&self, x: i32, y: i32) -> zbus::fdo::Result<()> {
        let sink = {
            let service = self.service.lock().await;
            service.sink.clone()
        };
        sink.send(TrayEvent::IconActivated {
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
    service: Arc<tokio::sync::Mutex<LinuxService>>,
}

impl DbusMenuIface {
    fn new(service: Arc<tokio::sync::Mutex<LinuxService>>) -> Self {
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

        let (item_id, sink) = {
            let service = self.service.lock().await;
            let item_id = service
                .menu
                .action_for(id)
                .ok_or_else(|| zbus::fdo::Error::InvalidArgs("id not found".into()))?;
            (item_id, service.sink.clone())
        };

        sink.send(TrayEvent::MenuItemActivated { item_id });
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
}

fn layout_to_dbus(node: LinuxMenuNode) -> DbusMenuLayout {
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

fn properties_to_dbus(properties: LinuxMenuProperties) -> HashMap<String, OwnedValue> {
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
