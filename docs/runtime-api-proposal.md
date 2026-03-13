# Runtime API Proposal

This note proposes how `trayinit` should expose tray startup/integration modes.

The key clarification is:

- `blocking` vs `non-blocking` is one axis
- `current-thread` vs `dedicated-thread` is a different axis

Those two axes should not be collapsed into one enum or one method.

## The Two Axes

### 1. Who owns the top-level app loop?

- Host-owned loop
  - Example: `winit`
  - The application already has a primary event loop.
  - The tray must integrate into that app without taking over control flow.

- Backend-owned loop
  - Example: standalone tray-only app
  - The tray runtime owns the app's top-level control flow and blocks until exit.

### 2. Where does the backend work run?

- Current thread
  - Needed for AppKit/macOS.
  - Also possible on Windows.

- Dedicated thread / worker runtime
  - Natural on Windows.
  - Natural on Linux SNI/DBus.

These are related, but not the same:

- A current-thread backend can be non-blocking if some other framework already owns the loop.
- A dedicated-thread backend can be non-blocking even if the main thread is otherwise idle.

## Proposed Public API

The public surface should describe loop ownership first, and runtime placement second.

```rust
pub trait TrayMethods: Tray + private::Sealed {
    fn builder(self) -> Builder<Self> {
        Builder::new(self)
    }

    /// Non-blocking.
    ///
    /// Integrates the tray into an already-existing host application loop.
    /// The backend may use the current thread or helper threads internally.
    fn attach(self) -> Result<Handle<Self>> {
        self.builder().attach()
    }

    /// Blocking.
    ///
    /// Runs a standalone tray application and does not return until shutdown.
    fn run(self) -> Result<()> {
        self.builder().run()
    }

    /// Non-blocking convenience for self-hosted backends.
    ///
    /// This is mainly for Windows/Linux, where the tray can own itself on a
    /// helper thread without taking over the caller's main thread.
    fn spawn(self) -> Result<Handle<Self>> {
        self.builder().spawn()
    }
}
```

```rust
pub struct Builder<T: Tray> {
    tray: T,
    runtime_preference: RuntimePreference,
    linux: LinuxOptions,
}

impl<T: Tray> Builder<T> {
    pub fn attach(self) -> Result<Handle<T>>;
    pub fn run(self) -> Result<()>;
    pub fn spawn(self) -> Result<Handle<T>>;
}
```

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RuntimePreference {
    /// Pick the natural strategy for the selected startup mode and platform.
    #[default]
    Auto,

    /// Prefer a dedicated worker thread or backend-owned worker runtime.
    DedicatedThread,

    /// Prefer binding backend work to the caller's current thread.
    CurrentThread,
}
```

## Semantics

### `attach()`

Use when the application already has a host loop.

Examples:

- `winit` app
- GUI app using some other framework
- app that already knows how its own lifetime is managed

Important semantics:

- `attach()` is non-blocking
- it does **not** mean "must run on current thread"
- it means "do not take over the app's top-level control flow"

Backend behavior may still differ by platform:

- macOS: current thread / main thread
- Windows: current thread is preferred for real loop integration
- Linux: helper thread is still acceptable and probably preferable

This is the method windowed apps should prefer.

### `run()`

Use when the tray runtime should own the app's primary loop.

Examples:

- tray-only app
- background utility with no host GUI framework

Important semantics:

- `run()` is blocking
- `run()` owns the application's top-level control flow until shutdown
- on macOS, this is the natural standalone path because AppKit wants the main event loop

This is the cross-platform standalone entry point.

### `spawn()`

Use when the tray should be self-hosted but the caller does not want a blocking `run()`.

Examples:

- Windows CLI utility that wants a tray plus some other non-UI work
- Linux app that wants to keep using its own logic on the main thread while the tray sits on a worker thread

Important semantics:

- `spawn()` is non-blocking
- `spawn()` is specifically a self-hosted mode
- `spawn()` should be treated as optional / platform-dependent

On macOS this should likely return `Error::Unsupported(...)`.

That is okay: `spawn()` is convenience, not the primary cross-platform story.

## Recommended Platform Mapping

### Windows

`attach()`:

- Supported
- Preferred shape for `winit`
- Backend can bind to the current thread and rely on the host message loop

`run()`:

- Supported
- Standalone tray-only mode
- Backend owns a Win32 message loop on the current thread

`spawn()`:

- Supported
- Dedicated hidden-window worker thread

### Linux

`attach()`:

- Supported
- Should **not** imply "inject into winit/calloop"
- The backend may still use its own worker thread / DBus runtime

`run()`:

- Supported
- Standalone tray-only mode
- Backend owns the process-lifetime tray runtime

`spawn()`:

- Supported
- Natural fit for SNI/DBus

### macOS

`attach()`:

- Supported
- Must be called on the main thread
- Intended for `winit` or another host loop already running on the main thread

`run()`:

- Supported
- Must be called on the main thread
- Standalone tray-only mode
- Backend owns the app's main event loop

`spawn()`:

- Not supported
- Reason: a correct AppKit-backed tray should not be hidden behind a detached worker thread

## What `Auto` Should Mean

`RuntimePreference::Auto` should be interpreted relative to the chosen startup mode.

### `attach()` + `Auto`

- Windows: current-thread integration
- Linux: dedicated worker thread / backend runtime
- macOS: current-thread integration

### `run()` + `Auto`

- Windows: current-thread owned loop
- Linux: backend-owned runtime on the current thread, if practical
- macOS: current-thread owned AppKit loop

### `spawn()` + `Auto`

- Windows: dedicated thread
- Linux: dedicated worker thread / runtime
- macOS: unsupported

## What This Means For `winit`

For `winit`, the main recommendation should be:

```rust
let event_loop = EventLoop::new()?;
let tray = AppTray::new(...).attach()?;
event_loop.run_app(&mut app)?;
```

The important point is:

- `winit` owns the app loop
- `attach()` tells `trayinit` not to own the app loop

Platform behavior under that one API:

- Windows: tray backend can bind to the same thread / message loop
- macOS: tray backend must bind to the same main thread / app loop
- Linux: tray backend can still use a helper thread for DBus/SNI

So the caller gets one app-level API, even though backend internals differ.

## Why Not Make `attach()` Mean "current thread only"?

Because that would make Linux awkward for no benefit.

What application code really cares about is:

- "does this take over my app loop?"
- not "does every backend use the same transport thread topology internally?"

If Linux needs a worker thread under `attach()`, that is still a valid attachment mode:

- the tray is integrated into the host app's lifetime
- the tray does not own top-level control flow
- the host app remains free to use `winit`

## Why Not Expose Only `spawn()` and `run()`?

Because `spawn()` is the wrong abstraction for macOS + `winit`.

In a `winit` app on macOS:

- the host loop already exists
- the tray should join that loop on the main thread
- there is no reason to model this as a self-hosted worker

So `attach()` deserves to exist as a first-class startup mode.

## Recommended Next Step

Keep the current `spawn()` implementation for Windows, but evolve the public API toward:

- `attach()`
- `run()`
- `spawn()`

Then treat:

- `attach()` as the preferred window-app API
- `run()` as the preferred standalone cross-platform API
- `spawn()` as an optional convenience for self-hosted backends

## Source Notes

These constraints are consistent with:

- `winit` event loop docs:
  - `EventLoopBuilder::build()` documents the cross-platform main-thread requirement
  - `EventLoopProxy` is the supported cross-thread wakeup path
  - `pump_events` is documented as non-portable and something you usually should not use

- Apple's app lifecycle docs:
  - the app starts on a main thread
  - `NSApplication::run` starts the event processing loop on the app's main thread
  - the app's main event loop processes events in the main run loop
