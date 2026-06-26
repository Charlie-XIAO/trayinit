# Pre-RFC: Declarative Native Tray Icons and Tray Menus

## Summary

This document proposes a new Rust crate for cross-platform native tray icons and
native tray menus. The crate should be tray-specific: it should not become a
general native menu framework, windowing abstraction, or replacement for `muda`.

The north star is a tray-specific state model plus platform backends:

- Applications describe the current tray state as Rust data.
- The menu tree is part of that state.
- Updates submit a replacement state.
- Backends translate state changes into native platform operations.
- Event delivery is explicit and app-owned, not global.

The crate should not depend on `tray-icon`, `muda`, or `ksni`. Those crates are
useful references for platform behavior, implementation techniques, and edge
cases only.

The recommended high-level shape is:

- A standalone core crate with no required `winit` dependency.
- Optional `winit` integration that adapts tray events to `EventLoopProxy`.
- No separate iced adapter crate; iced/iced-winit can wire the core crate
  internally if desired.
- A declarative `TrayState` and `Menu` model with typed IDs.
- A `Tray` owner plus cloneable command handle.
- A per-tray event sink supplied by the application.
- Windows prototype first, Linux second, macOS last.

Windows-first is only the implementation order, chosen because the current
development machine is Windows. It is not an API design bias. The common API
must remain equally suitable for Windows, macOS, and Linux StatusNotifierItem
backends, and must not expose Win32-specific constraints as the primary public
model.

## Reference Implementation Findings

### Windows: `tray-icon` behavior to follow

`tray-icon` uses a battle-tested Win32 shape that this crate should largely
follow internally:

- Create a hidden HWND dedicated to tray icon messages.
- Register the tray icon with `Shell_NotifyIconW(NIM_ADD)` and a callback
  message in `NOTIFYICONDATAW`.
- Update icon and tooltip with `Shell_NotifyIconW(NIM_MODIFY)`.
- Remove the icon with `Shell_NotifyIconW(NIM_DELETE)` during shutdown/drop.
- Register the `TaskbarCreated` message and re-add tray icons after Explorer or
  the taskbar restarts.
- Allow `TaskbarCreated` through UIPI with `ChangeWindowMessageFilterEx` for
  elevated processes.
- Store the latest icon and tooltip state so re-registration after taskbar
  restart is possible.
- Use `Shell_NotifyIconGetRect` where available to report tray icon geometry.
- Use `SetForegroundWindow(hwnd)` before `TrackPopupMenu` so a native popup menu
  closes correctly when the user clicks outside it.
- Track click, double-click, enter, move, and leave events where Win32 provides
  enough information.

The public API should not follow `tray-icon`'s builder plus imperative setters
as the primary abstraction. The new crate should expose state replacement, not a
menu object graph that users mutate directly.

### macOS: `tray-icon` behavior to follow

`tray-icon` uses the right native substrate on macOS:

- Use `NSStatusBar` and `NSStatusItem`.
- Use the status item button for image, title, tooltip, and tracking.
- Use `NSMenu`/`NSMenuItem` for the native tray menu.
- Enforce AppKit main-thread access. The reference implementation uses
  `MainThreadMarker`; the new crate should do the same or an equivalent.
- Support template icons because that is important for menu bar appearance.
- Remove the status item from `NSStatusBar` on shutdown/drop.

The common API should not expose `NSStatusItem`. Platform extension methods can
be considered after v1 if raw handle access proves necessary.

### Linux: `tray-icon` behavior to avoid

`tray-icon`'s Linux path is the main thing this new crate should avoid:

- It depends on `libappindicator`.
- It gets menu support through `muda` and GTK.
- It inherits GTK3 as a dependency for tray menus.
- Its Linux public behavior has awkward limitations such as menu replacement or
  removal not fitting cleanly into the model.
- It may need temporary icon files in cases where a DBus icon pixmap model would
  be a better fit.

For this crate, Linux v1 should not use GTK3, GTK, AppIndicator, or XEmbed.

### Linux: `ksni` behavior to follow

`ksni` is the primary Linux reference. Its StatusNotifierItem/DBusMenu approach
is the right basis for a GTK-free backend.

Implementation ideas to follow:

- Use DBus and implement `org.kde.StatusNotifierItem`.
- Export a DBusMenu object implementing `com.canonical.dbusmenu`.
- Register the item with `org.kde.StatusNotifierWatcher`.
- Monitor `org.kde.StatusNotifierWatcher` owner changes and re-register when it
  comes back.
- Expose `/StatusNotifierItem` and `/MenuBar`-style object paths.
- Provide SNI properties and signals for title, status, icon, tooltip, and menu.
- Provide DBusMenu layout and property methods:
  - `GetLayout`
  - `GetGroupProperties`
  - `GetProperty`
  - `Event`
  - `EventGroup`
  - `AboutToShow`
  - `LayoutUpdated`
  - `ItemsPropertiesUpdated`
- Flatten the menu tree internally into DBus menu item IDs.
- Distinguish layout changes from property-only changes.
- Bump the DBusMenu revision and invalidate IDs when layout changes, because
  tray hosts cache menu layout and item properties.
- Treat missing watcher or host as a clear runtime condition.
- Consider a soft-start option for apps that launch before the desktop shell is
  ready.

One important detail from `ksni`: menu-on-activation behavior is subtle. Some
Linux tray hosts expect `ItemIsMenu`; GNOME/AppIndicator compatibility can
require returning `UnknownMethod("ItemIsMenu")` from `Activate` to make menu
opening behavior work. The new crate should encode this as a backend policy, not
as a user-facing DBus detail.

### `muda`: implementation lessons and API ideas to avoid

`muda` is useful as an implementation reference for native menu construction:

- Win32 `HMENU`, checked/disabled states, item IDs, and menu tracking.
- macOS `NSMenu`/`NSMenuItem` target/action mapping.
- Native icon menu items and check menu items.
- Platform-specific cleanup of menu handles and item references.

The new crate should avoid `muda`'s public abstraction:

- Do not expose a general `Menu` object with `append`, `insert`, `remove`, and
  item-specific setter methods as the primary API.
- Do not expose menu bar, app menu, predefined app menu, or arbitrary context
  menu functionality in v1.
- Do not use global static event receivers or process-global event handlers.
- Do not make users manually construct native menus object-by-object.

## Current Winit and Iced Integration Analysis

### Winit model

Current local `winit` is `0.30.13`. Its modern application model is
`ApplicationHandler`:

- Apps create an `EventLoop<T>`, usually with
  `EventLoop::<T>::with_user_event().build()`.
- Apps run by passing an `ApplicationHandler<T>` to `run_app`.
- Cross-thread or external integrations wake the event loop with
  `EventLoopProxy<T>::send_event`.
- User events are delivered to `ApplicationHandler::user_event`.
- Windows should generally be created in `resumed` via `ActiveEventLoop`.
- `ActiveEventLoop` and `EventLoop` expose owned display handles, but tray icons
  do not need those for the proposed v1 design.

Winit issue #2160 is still open. It is about "blessed" out-of-scope ecosystem
features such as clipboards, menus, dialogs, notifications, and tray icons. The
important current takeaway is that tray support is considered adjacent to winit,
not part of winit core. The issue does not imply that a tray crate should depend
directly on winit.

The best integration model is therefore an optional adapter, not a required
winit dependency.

### Should the core crate depend on winit?

Recommendation: no.

Reasons:

- Linux SNI/DBus does not need a windowing event loop.
- Windows can use a private hidden HWND.
- macOS needs AppKit main-thread access, not winit specifically.
- Non-winit users should not pay for or align with a winit dependency.
- Iced currently tracks its own winit dependency/revision; a direct dependency
  in the tray core could create version friction.

The core crate should define its own `EventSink` trait. The optional `winit`
feature should provide a small adapter from `TrayEvent` to a user event.

### Tray events into a winit app

The intended flow is:

1. The application creates `EventLoop<UserEvent>`.
2. The application creates a `winit` event sink using `event_loop.create_proxy()`.
3. The application creates `Tray`, preferably in `ApplicationHandler::resumed`
   in winit examples.
4. Native backends send `TrayEvent` into the sink.
5. The sink maps `TrayEvent` into `UserEvent` and calls
   `EventLoopProxy::send_event`.
6. The application receives it in `ApplicationHandler::user_event`.
7. The application updates its model and submits a new `TrayState`.

This keeps the tray crate from owning the winit event loop.

For plain Windows and Linux core usage, creating the tray before `run_app` is
acceptable if the application does not need winit lifecycle integration. Winit
examples should still use `resumed` because it is the idiomatic place to create
external application resources. On macOS, tray creation and mutation must happen
on the main thread with AppKit available; `resumed` is the clearest lifecycle
point for examples.

### Iced / iced-winit

Current local iced is `0.15.0-dev`. Its public app model is `Program`:

- `Program::Message: Send + 'static`.
- `Program::update` returns a `Task<Message>`.
- `Program::subscription` returns a `Subscription<Message>`.
- `iced_winit` wraps winit events in an internal runtime and uses a proxy around
  `EventLoopProxy<Action<Message>>`.

The crate should not ship `my_crate_iced`. Instead:

- The core crate should be easy for iced-winit to use internally.
- iced-winit can own a `Tray`, map `TrayEvent` into `Message`, and submit
  `TrayState` after app updates.
- Until first-class iced support exists, users can bridge with a channel or a
  winit user event.

An eventual iced API could be shaped like:

```rust
iced::application(Model::new, update, view)
    .tray(|state| state.tray_state())
    .on_tray_event(Message::Tray)
    .run()
```

That should live in iced/iced-winit if accepted by iced maintainers.

### Event delivery model

Considered options:

- Callback model: simple, but makes backend thread semantics leak into app
  logic and can block native menu handling.
- Channel model: portable, but a bare channel does not wake a winit app.
- Trait sink model: flexible and lets the app or adapter decide how events move.
- Runtime-driver model: too heavy for v1 and risks owning the app loop.
- Direct `EventLoopProxy`: ergonomic for winit, but couples core to winit.

Recommendation: core `EventSink` trait plus helper sinks:

- `channel` helper for standalone apps.
- Optional `winit` helper for `EventLoopProxy`.

### Thread and loop constraints

Windows:

- HWND and native menu handles are thread-affine.
- The backend should own a private backend thread with a hidden HWND and message
  loop for the core crate.
- Multiple `Tray` instances are supported: each instance owns its own backend
  thread, hidden HWND, tray icon, menu, and event sink. Reusing
  `NOTIFYICONDATAW.uID = 1` is safe because Windows identifies tray icons by the
  `(HWND, uID)` pair. Repeated window-class registration is safe when
  `ERROR_CLASS_ALREADY_EXISTS` is accepted, and each HWND receives
  `TaskbarCreated` independently.
- `Tray::new` should spawn that thread, wait until the hidden HWND is ready, and
  then return.
- `TrayHandle` calls from other threads should enqueue commands and wake the
  backend thread by posting a private window/thread message.
- Tray and menu events should be sent back through the supplied `EventSink` from
  the backend thread.
- The backend must handle `TaskbarCreated` re-registration.
- `EventSink::send` must never be called while holding backend state locks,
  native menu locks, or mutable native-menu borrow guards. Event handlers may
  immediately call `Tray::set_state`; that must enqueue cleanly without
  deadlocking or recursively applying native mutations.

macOS:

- AppKit objects must be created and mutated on the main thread.
- `Tray::new` should return `TrayError::NotMainThread` if called from the wrong
  thread on macOS, or use an explicitly documented main-thread dispatcher if one
  is introduced later.
- `TrayHandle` updates from other threads must marshal to the main thread.

Linux:

- SNI and DBusMenu are async DBus services.
- The backend does not need to share a winit event loop.
- A backend-managed async runtime/task is acceptable. Stage 2 uses zbus on a
  backend-owned runtime with real `linux-zbus-async-io` and `linux-zbus-tokio`
  feature switches; no async runtime leaks into the public API.
- The crate should not require GTK or a GLib main loop.
- Multiple `Tray` instances are supported by giving each instance its own zbus
  connection and generated `org.kde.StatusNotifierItem-{pid}-{counter}` service
  name. The standard `/StatusNotifierItem` and `/MenuBar` object paths can be
  reused because the service name differs per tray.

## Public API Proposal

### Core state model

```rust
pub struct TrayState {
    pub visible: bool,
    pub icon: Option<Icon>,
    pub title: Option<String>,
    pub tooltip: Option<String>,
    pub menu: Option<Menu>,
    pub activation: ActivationPolicy,
    pub platform: PlatformOptions,
}

pub struct Menu {
    pub items: Vec<MenuNode>,
}

pub enum MenuNode {
    Item(MenuItem),
    Check(CheckItem),
    Submenu(Submenu),
    Separator,
}

pub struct MenuItem {
    pub id: MenuItemId,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub icon: Option<MenuIcon>,
}

pub struct CheckItem {
    pub id: MenuItemId,
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
    pub visible: bool,
}

pub struct Submenu {
    pub id: Option<MenuItemId>,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub items: Vec<MenuNode>,
}
```

`TrayState::new()` is the construction path. Defaults are:

- `visible = true`
- `icon = None`
- `title = None`
- `tooltip = None`
- `menu = None`
- `activation = ActivationPolicy::PlatformDefault`
- `platform = PlatformOptions::default()`

`TrayState` should implement `Clone`, `Debug`, `PartialEq`, `Eq`, and validation
helpers. The state does not own native handles.

### IDs

```rust
pub struct MenuItemId(Arc<str>);
```

Requirements:

- A `Tray` instance is the identity of one tray icon. `TrayState` does not need
  an app-provided tray ID in the common model.
- Actionable menu item IDs must be stable across updates.
- Duplicate item IDs within the same tray menu are invalid.
- Separators do not need IDs.
- Submenu IDs are optional. Backends may generate internal submenu IDs unless a
  future concrete platform requirement proves user-provided submenu IDs are
  needed.

Message-like values can be layered on top of explicit IDs by applications or
framework integrations, but they should not be the core identity mechanism.
Backends need stable internal numeric/string IDs, and an `Eq` application message
does not by itself provide a durable platform mapping. The recommended core
contract is explicit stable `MenuItemId`; framework helpers can map item IDs to
application messages.

Multiple tray icons can be modeled as multiple `Tray` instances. Applications
that need app-level tray identity can wrap the event sink per instance or map
events into their own tagged message type.

### Icons

The initial common icon type should support raw pixels:

```rust
pub struct Icon {
    pub rgba: Arc<[u8]>,
    pub width: u32,
    pub height: u32,
}
```

Recommended additions:

- `Icon::from_rgba(Vec<u8>, width, height) -> Result<Icon, IconError>`.
- Optional Linux `IconSource::ThemeName(String)` or `LinuxIconName` later if
  needed.
- No required image decoding dependency in the core API.

### Activation policy

```rust
pub enum ActivationPolicy {
    OpenMenu,
    SendEvent,
    PlatformDefault,
}
```

Recommended default: `PlatformDefault`.

Platform mapping:

- Windows/macOS can show the menu on left click if configured.
- Linux SNI may use `ItemIsMenu`/activation behavior internally.
- Some Linux hosts may ignore or reinterpret activation hints; document this.

### Events

```rust
pub enum TrayEvent {
    MenuItemActivated {
        item_id: MenuItemId,
    },
    IconActivated {
        activation: ActivationKind,
        position: Option<PhysicalPosition<i32>>,
        rect: Option<PhysicalRect<i32>>,
    },
    Scroll {
        delta: i32,
        orientation: Orientation,
    },
    StatusChanged {
        status: RuntimeStatus,
    },
}
```

Mouse move/enter/leave should not be a core portability promise for v1. Windows
and macOS can support them later as platform-specific extensions or optional
events if there is real demand.

### Event delivery

```rust
pub trait EventSink: Send + Sync + 'static {
    fn send(&self, event: TrayEvent);
}
```

Provide convenience implementations:

- Blanket implementation for closures.
- Channel helper returning `(EventSender, EventReceiver)`.
- Optional winit sink adapter.

Do not provide a process-global event receiver.

`EventSink::send` may be called from backend/platform threads. Sinks should
forward events into an application queue, channel, or event-loop proxy and should
not directly mutate UI state. Sinks must also be reentrancy-safe with respect to
tray commands: if event handling immediately calls `Tray::set_state`, that call
must enqueue a command and must not run native mutation recursively inside the
event dispatch path.

Event delivery is best-effort. A closed channel receiver or failed event-loop
proxy should not panic the backend. Runtime platform failures are reported with
`TrayEvent::StatusChanged`; event delivery failure is treated as the receiving
application no longer listening.

### Handle and lifetime semantics

```rust
pub struct Tray {
    // Owns backend lifetime.
}

#[derive(Clone)]
pub struct TrayHandle {
    // Sends commands to backend.
}

impl Tray {
    pub fn new(state: TrayState, sink: impl EventSink) -> Result<Self, TrayError>;
    pub fn set_state(&self, state: TrayState) -> Result<(), TrayError>;
    pub fn handle(&self) -> TrayHandle;
    pub fn shutdown(self) -> Result<(), TrayError>;
}

impl TrayHandle {
    pub fn set_state(&self, state: TrayState) -> Result<(), TrayError>;
    pub fn close(&self) -> Result<(), TrayError>;
}
```

Semantics:

- `Tray` is RAII. Dropping it removes the tray icon when possible.
- `TrayHandle` does not keep the tray alive forever unless the backend needs
  that internally; commands fail with `BackendClosed` after shutdown.
- `set_state` validates the whole state before enqueueing it.
- `Ok(())` from `Tray::set_state` or `TrayHandle::set_state` means "validated
  and accepted/queued"; it does not mean the OS has fully applied the update.
- If validation fails, the old native state remains active.
- Later backend or native failures are surfaced through runtime status/error
  events such as `TrayEvent::StatusChanged`.
- Backends may internally diff or fully rebuild native menus.

### Send and Sync

Recommended guarantees:

- `TrayState`, `Menu`, menu nodes, IDs, icons, and events:
  `Clone + Send + Sync + PartialEq + Eq` where all fields support it.
- `TrayHandle`: `Clone + Send + Sync`.
- `Tray`: do not promise `Send` or `Sync` in the portable API.
- Event sinks may be called from backend/platform threads unless an adapter
  marshals events elsewhere.

### Errors

```rust
pub enum TrayError {
    UnsupportedPlatform,
    NotMainThread,
    InvalidState(InvalidState),
    BackendClosed,
    Os(OsError),
    Dbus(DbusError),
    WatcherUnavailable,
    WouldNotShow,
}
```

Recommended behavior:

- Creation errors are explicit.
- Runtime backend status changes are emitted as `TrayEvent::StatusChanged`.
- Event sink failure should not panic; the backend should mark/report the sink as
  closed and continue or shut down according to documented policy.

## Architecture Proposal

### Crate and module layout

Recommended modules:

```text
src/
  lib.rs
  model.rs
  menu.rs
  icon.rs
  event.rs
  error.rs
  backend/
    mod.rs        // shared backend command/proxy/validation boundary
    plan.rs       // platform-neutral menu planning
  platform/
    mod.rs       // platform dispatch and unsupported-platform fallback
    windows/
      mod.rs      // Win32 hidden-HWND/Shell_NotifyIcon backend
    linux/
      mod.rs      // Linux backend module gate
      service.rs  // zbus SNI/DBusMenu service runtime
      menu.rs     // pure DBusMenu planning/property mapping
    macos/
      mod.rs      // future AppKit backend
  integration/
    mod.rs
    winit.rs
```

The platform modules should be private implementation details except for
carefully chosen platform extension APIs. Shared validation, command semantics,
and platform-neutral planning stay outside `platform/` because they define the
portable backend contract.

### Backend boundary

Backends should receive validated commands:

```rust
enum BackendCommand {
    SetState(TrayState),
    Close,
}
```

Each backend owns:

- The last successfully applied `TrayState`.
- Native handles.
- A command ingress path.
- The event sink.

Backends must not hold backend locks or native menu mutable state while invoking
the event sink. Event dispatch should capture the public `TrayEvent`, release
internal locks and native borrow guards, and only then call `EventSink::send`.

The common layer should perform:

- Duplicate ID checks.
- Basic icon data validation.
- String length checks where cross-platform limits are known.
- Stable ID assumptions.

Backend-specific code should perform:

- Native handle creation.
- Platform conversion.
- Platform-specific fallbacks and warnings.

### Feature flags

Recommended initial flags:

```toml
[features]
default = ["linux-zbus-async-io"]
linux-zbus-async-io = [
    "dep:async-io",
    "dep:async-lock",
    "zbus/async-io",
]
linux-zbus-tokio = [
    "dep:tokio",
    "zbus/tokio",
]
winit = ["dep:winit"]
serde = ["dep:serde"]
```

Notes:

- Linux runtime features are real switches. `linux-zbus-async-io` is the
  default so downstream apps are not forced to pull in Tokio. Apps that prefer
  Tokio should disable default features and enable `linux-zbus-tokio`.
- `zbus` and `futures-util` are required Linux backend dependencies. The runtime
  features select only the concrete runtime crates and zbus runtime feature.
- On Linux, selecting both Linux runtime features or selecting neither is a
  compile-time error. Non-Linux builds do not require a Linux runtime feature.
- The Windows stage should not add Linux or macOS dependencies.
- The core should not depend on `winit` unless `winit` is enabled.

### Runtime or driver object

Do not require an application-owned runtime driver for v1.

Recommended backend approach:

- Windows: backend owns hidden HWND and command marshalling.
- Linux: backend owns DBus async service task/thread.
- macOS: backend owns AppKit status item on main thread and marshals commands
  there.

A future low-level driver API can be considered if users need to integrate DBus
or Win32 pumping manually.

### Avoiding global mutable state

Avoid:

- Global event receivers.
- Global menu item registries.
- Process-global callbacks.

Allow:

- Backend-internal class registration on Windows if required.
- Backend-internal static Objective-C class declarations on macOS if required.
- Per-process counters for generated backend IDs, hidden from public API.

### Keeping platform code testable

Separate pure planning from native side effects:

- Menu validation can be pure.
- Menu flattening can be pure.
- Diff planning can be pure.
- Command enqueue behavior can be tested without native APIs.
- Reentrancy can be modeled with a test sink that immediately calls `set_state`
  after receiving a menu event.
- Native apply code should be thin.

Tests should cover pure code heavily and native code through examples/manual
tests where automation is difficult.

## API Sketches

### Minimal tray icon

```rust
use native_tray::{Icon, Tray, TrayState};

let icon = Icon::from_rgba(rgba, width, height)?;

let tray = Tray::new(
    TrayState::new()
        .icon(icon)
        .tooltip("Example is running"),
    |event| {
        eprintln!("{event:?}");
    },
)?;
```

### Tray icon with menu

```rust
use native_tray::{Menu, MenuNode, TrayState};

let state = TrayState::new()
    .icon(icon)
    .tooltip("Example")
    .menu(Menu::new([
        MenuNode::item("show", "Show Window"),
        MenuNode::check("sync", "Sync enabled", sync_enabled),
        MenuNode::separator(),
        MenuNode::item("quit", "Quit"),
    ]));

tray.set_state(state)?;
```

### Updating tray/menu declaratively

```rust
fn tray_state(model: &AppModel) -> TrayState {
    TrayState::new()
        .visible(model.show_tray)
        .icon(model.icon.clone())
        .tooltip(model.tooltip.clone())
        .menu(Menu::new([
            MenuNode::check("sync", "Sync enabled", model.sync_enabled),
            MenuNode::item("open", "Open Dashboard")
                .enabled(model.can_open_dashboard),
            MenuNode::separator(),
            MenuNode::item("quit", "Quit"),
        ]))
}

tray.set_state(tray_state(&model))?;
```

### Receiving menu events

```rust
let tray = Tray::new(tray_state(&model), move |event| {
    let _ = tx.send(AppEvent::Tray(event));
})?;
```

### Winit integration

```rust
enum UserEvent {
    Tray(native_tray::TrayEvent),
}

let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
let tray_sink = native_tray::winit::event_sink(
    event_loop.create_proxy(),
    UserEvent::Tray,
);

struct App {
    tray: Option<native_tray::Tray>,
    model: Model,
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        if self.tray.is_none() {
            self.tray = Some(
                native_tray::Tray::new(tray_state(&self.model), tray_sink.clone()).unwrap()
            );
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Tray(native_tray::TrayEvent::MenuItemActivated { item_id, .. }) => {
                self.model.handle_tray_item(item_id);

                if let Some(tray) = &self.tray {
                    let _ = tray.set_state(tray_state(&self.model));
                }
            }
            UserEvent::Tray(_) => {}
        }
    }
}
```

### Iced-winit integration sketch

This is not a separate crate. It is a sketch for an eventual iced/iced-winit
change:

```rust
iced::application(Model::new, update, view)
    .tray(|state| state.tray_state())
    .on_tray_event(Message::Tray)
    .run()
```

Internally, iced-winit would:

- Create a tray event sink that sends `Action::Output(Message::Tray(event))`
  through its existing proxy.
- Own the `Tray`.
- Recompute `TrayState` after message updates.
- Store the last submitted `TrayState`.
- Call `tray.set_state` only when the new state differs from the prior state.

## Tradeoffs

### Declarative model vs imperative handles

Choose declarative public state.

Pros:

- Matches application state models used by winit and iced apps.
- Makes dynamic menu updates ergonomic.
- Keeps platform handles private.
- Avoids `muda`-style object mutation.

Cons:

- Requires backend diffing or replacement logic.
- Requires stable user IDs and validation.
- Some platform operations are naturally imperative internally.

`TrayState`, `Menu`, `MenuNode`, IDs, icons, and events should implement
`PartialEq`/`Eq` where possible. This lets applications and integrations avoid
redundant `set_state` calls before the backend sees them. Equality is an API
convenience and integration optimization; it is not a substitute for backend
diffing where a platform needs it.

### Diffing vs full replacement

Expose full replacement publicly; choose per-backend internally.

- Linux should diff layout and properties because DBusMenu clients cache layout
  and IDs.
- Windows can initially rebuild menus on state changes unless that causes visible
  problems.
- macOS can initially rebuild `NSMenu` trees unless stateful menu behavior
  requires finer updates.

### Callback vs channel vs event-loop proxy

Choose `EventSink`.

- Callbacks alone are too easy to run on the wrong thread.
- Channels alone do not wake winit.
- Direct `EventLoopProxy` couples the core crate to winit.
- An injected sink gives the core a small surface and lets integrations decide
  the best delivery path.

### Standalone core vs direct winit dependency vs optional winit feature

Choose standalone core plus optional `winit`.

- Core stays useful outside winit.
- Winit apps get ergonomic adapter helpers.
- Iced can integrate without version friction.

### Backend-managed thread vs user-event-loop integration

Choose backend-managed platform work for v1.

- Windows should use a private backend thread with a hidden HWND and message
  loop, not the user's winit loop.
- Linux needs async DBus processing but not a GUI loop.
- macOS needs the main thread; command marshalling is unavoidable.

### Linux DBus async runtime strategy

Current Stage 2 direction:

- Uses a backend-managed `zbus` runtime selected by feature flag.
- Defaults to `async-io` to avoid forcing Tokio into downstream apps with their
  own runtime switches, such as iced applications using smol.
- Supports Tokio as an opt-in runtime for applications that already use Tokio.
- Keeps the runtime Linux-only and backend-internal; no async runtime type or
  async constructor leaks into the portable public API.
- Rejects both-runtime and no-runtime Linux builds at compile time.
- Do not require GLib.
- Keep the DBus runtime boundary backend-internal.
- Do not expose async constructors in the portable API unless a later platform
  constraint makes that unavoidable.
- Initial watcher/host absence should soft-start successfully and emit
  `TrayStatus::TemporarilyUnavailable`.
- Later watcher loss or recovery should be reported through runtime status
  events.

## Unresolved Questions

These need owner decisions before or during implementation. Each has a
recommended default.

- Crate name:
  - Recommended default: `native-tray` for design discussion.
  - Consequence: can be changed before publish without API impact.

- Icon sources:
  - Recommended default: raw RGBA data in common API; Linux theme-name support as
    a platform option later.
  - Consequence: no image decoding dependency is required in v1.

- Mouse hover/move events:
  - Recommended default: not part of portable v1.
  - Consequence: Windows/macOS can expose richer platform events later without
    overpromising Linux support.

- Menu accelerators:
  - Recommended default: out of v1.
  - Consequence: avoids cross-platform shortcut semantics that are less relevant
    for tray menus.

- Multiple tray icons:
  - Recommended default: support from the beginning.
  - Consequence: each `Tray` instance owns one tray icon; applications that need
    app-level identity wrap each sink or map events into their own tagged
    message type.

- Event sink failure:
  - Recommended default: best-effort and infallible; do not panic if a receiver
    is closed.
  - Consequence: sinks should forward into queues/proxies and may drop events
    after the application stops listening.

- macOS template icon:
  - Recommended default: `PlatformOptions::macos_template_icon`, default `false`.
  - Consequence: common model stays portable while supporting native appearance.

- Linux watcher absence:
  - Recommended default: creation returns `WatcherUnavailable` or `WouldNotShow`,
    with an option to treat early absence as soft and wait for watcher online.
  - Consequence: strict by default, configurable for startup-before-shell cases.

## Staged Implementation Plan

### Stage 0: API/design validation

- Create and review this document.
- Do not add crate skeleton, dependencies, or implementation code.
- Validate that the public API direction is tray-specific and declarative.
- Validate that the integration model works for winit and can be adopted by
  iced-winit.

### Stage 1: Windows prototype

Prototype first on Windows because the current development machine is Windows.

Goals:

- Create the initial crate skeleton only when Stage 1 starts.
- Implement the pure model layer first: `TrayState::new`, `Menu`, actionable
  `MenuItemId`, optional submenu IDs, `Icon`, `TrayEvent`, and `EventSink`.
- Implement validation before native code: duplicate actionable IDs, invalid
  icon dimensions/byte length, and basic state consistency.
- Implement menu flattening/planning before native code, including backend
  generated submenu IDs.
- Add model/command tests before native code:
  - duplicate menu item IDs are rejected
  - invalid icons are rejected
  - state validation rejects missing/invalid required data
  - menu flattening preserves actionable IDs and generates submenu identity
  - equal `TrayState` values can be used to skip redundant updates
  - menu event dispatch can immediately call `Tray::set_state` without holding
    backend/native locks or recursively applying native mutations
- Implement `Tray` and `TrayHandle` command semantics so `set_state` means
  validated and accepted/queued.
- Implement Windows backend with a private backend thread, hidden HWND, and
  command wakeup message.
- Register/update/remove tray icon using `Shell_NotifyIconW`.
- Handle `TaskbarCreated` re-registration.
- Build native popup menus from the declarative menu state.
- Deliver menu item activation events through `EventSink` without holding
  backend locks.
- Provide a minimal cross-platform smoke example with platform cfg only where
  behavior differs.

Non-goals:

- Linux backend.
- macOS backend.
- General native menu bars.
- Full menu diff optimization beyond what is needed for correctness.

### Stage 2: Linux StatusNotifierItem prototype

Goals:

- Implement GTK-free SNI/DBusMenu backend.
- Use `zbus`; avoid GTK, AppIndicator, and XEmbed.
- Export SNI and DBusMenu objects on per-instance zbus connections.
- Register with StatusNotifierWatcher using generated service names.
- Implement soft-start watcher handling:
  - no watcher at startup: `Tray::new` succeeds and emits
    `TrayStatus::TemporarilyUnavailable`
  - watcher registration/recovery succeeds: emit `TrayStatus::Available`
  - watcher loss: emit `TrayStatus::TemporarilyUnavailable`
  - session bus/runtime setup failure: `Tray::new` returns an error
- Implement menu layout flattening, revision tracking, and `LayoutUpdated`.
- Deliver menu events through `EventSink`.
- Document desktop environment limitations.

### Stage 3: macOS prototype

Goals:

- Implement `NSStatusItem` and `NSMenu`.
- Enforce main-thread creation and mutation.
- Support icon, title, tooltip, menu, and menu item activation.
- Support template icons.
- Provide cleanup on drop/shutdown.

Constraint:

- The maintainer does not currently have a macOS machine at hand, so this stage
  should be more conservative and may need outside testing.

### Stage 4: Winit example

Goals:

- Add optional `winit` feature.
- Provide `EventLoopProxy` event sink adapter.
- Add an example using `ApplicationHandler`, `resumed`, `user_event`, and
  declarative state updates.

### Stage 5: Iced-winit integration sketch/example

Goals:

- Provide a channel/subscription based external iced example if practical.
- Draft a concise iced-winit integration proposal showing how iced could own
  tray support internally.
- Do not create a separate iced adapter crate.

### Stage 6: API polish and tests

Goals:

- Add model validation tests.
- Add duplicate-ID tests.
- Add menu diff/flattening tests.
- Add backend command planning tests.
- Tighten error names and docs.
- Document platform limitations clearly.
- Prepare examples for all implemented platforms.
