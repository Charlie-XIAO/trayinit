# Linux Deferred Work

This note records Linux-specific work that is intentionally deferred, so the
current decisions do not get lost in chat history.

## Already done

- SNI tray registration on D-Bus without GTK
- Declarative DBusMenu export and diffing
- `Activate` / `SecondaryActivate` / `ContextMenu` / `Scroll` mapping
- Tray/menu raster icons
- Linux `icon-name` support for tray and menu
- Attention/overlay icon properties
- Tray category export
- Watcher startup policy knobs:
  - `linux_own_dbus_name(...)`
  - `linux_assume_watcher_available(...)`

## Not intending to add right now

- Watcher online/offline callbacks in the public API
  - Reason: this wants a deliberate lifecycle surface, not a Linux-only quick
    callback added ad hoc.
- Linux-specific `window_id`, `icon_theme_path`, and `text_direction` API
  - Reason: these are real SNI fields, but they widen the public surface with
    Linux-only knobs that do not yet have a clear cross-platform story.
- DBusMenu `disposition`
  - Reason: low priority and Linux-specific; not needed for the current tray
   /menu model.
- Separate tooltip title/icon override API
  - Reason: the current mapping already exports SNI tooltip data from
    `title()`, `tooltip()`, and icon state. A second tooltip-only struct would
    mostly duplicate existing data for hosts that often ignore tooltips anyway.
- Raw DBusMenu features such as `hovered` and `ItemActivationRequested`
  - Reason: these go beyond the current semantic, declarative tray/menu model.

## Might do later

- Watcher lifecycle API
  - Example direction: notify the app when the watcher/host disappears or
    comes back, without forcing libappindicator/XEmbed fallback into this crate.
- Linux tray/window integration fields
  - `window_id`, `icon_theme_path`, `text_direction`
- Tooltip override API
  - Only if a real use case appears for tooltip title/icon being different from
    the tray title/icon.
- Additional DBusMenu behavior
  - `hovered`, `ItemActivationRequested`, or other protocol surface beyond the
    current click-driven menu model.
- Revisit Linux runtime semantics
  - Today `attach()` / `spawn()` / `run()` all use a dedicated backend thread.
    `Builder::linux_tokio_handle(...)` can reuse a host Tokio runtime instead
    of creating a private one, but it does not yet give Linux a distinct
    current-thread integration model.

## Known caveats

- Attention/overlay presentation is host-controlled
  - `attention_icon*`, `attention_movie_name`, and `overlay_icon*` are real
    SNI properties that `trayinit` exports, but Linux hosts are free to decide
    how to visualize them.
  - In practice, hosts may ignore overlay icons entirely, or present attention
    state using composition/caching behavior that does not look like a strict
    "replace the current icon with this exact new icon" model.
  - Because of that, mixing base tray-icon changes with active attention state
    can produce host-specific results that are hard to interpret from the UI
    alone.
  - If this needs to be verified later, the right next step is to inspect the
    exported D-Bus state/signals directly (for example with `busctl`) before
    treating it as a backend bug.
