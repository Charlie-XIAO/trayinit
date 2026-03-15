# Event API Proposal

This note records the direction for tray interaction events in `trayinit`.

The core decision is to keep the public API semantic-first:

- `PrimaryActivate`
- `SecondaryActivate`
- `ContextMenu`
- `Scroll`

and to avoid exposing pointer-vs-keyboard provenance, button lifecycle, or hover
tracking in the default cross-platform event stream.

## Current Public API

```rust
#[non_exhaustive]
pub enum TrayEvent<Message> {
    Menu(Message),
    Interaction(InteractionEvent),
    Scroll(ScrollEvent),
}

#[non_exhaustive]
pub struct InteractionEvent {
    pub kind: InteractionKind,
    pub position: Option<PhysicalPosition<i32>>,
    pub area: Option<(PhysicalPosition<i32>, PhysicalSize<i32>)>,
}

#[non_exhaustive]
pub enum InteractionKind {
    PrimaryActivate,
    SecondaryActivate,
    ContextMenu,
}

#[non_exhaustive]
pub struct ScrollEvent {
    pub delta: i32,
    pub axis: ScrollAxis,
    pub position: Option<PhysicalPosition<i32>>,
    pub area: Option<(PhysicalPosition<i32>, PhysicalSize<i32>)>,
}
```

## Goals

- Keep one primary event surface for `Tray::event(...)`.
- Preserve the semantic distinctions that matter across platforms.
- Keep optional geometry data when the backend can report it.
- Avoid promising event detail that some backends cannot supply reliably.
- Leave room for future extensions without committing them to the first core API.

## Non-goals

- A full raw pointer event stream in the core API.
- Perfect parity across all platforms.
- Hover, move, enter, leave, or button down/up in the default event family.
- Guaranteed keyboard-vs-pointer provenance in the public API.

Those can be reconsidered later as explicit extensions if they prove valuable.

## Why This Shape

Most tray applications care about what happened, not about every transport-level
detail of how the platform reported it.

Examples:

- "primary activate" is useful
- "open the menu" is useful
- "alternate activate" is useful
- "pointer up from the left button, unless it was keyboard-origin on this shell"
  is usually not useful enough to justify the complexity

This matters especially because the backends do not agree:

- Windows can surface mouse-style notifications and some semantic follow-up
  notifications from the shell.
- Linux SNI is semantic by design.
- macOS is centered around status-item button/menu behavior rather than a rich
  raw callback stream.

If the public API is semantic-first, backends can still use lower-level detail
internally where needed without forcing downstream code to depend on it.

## Semantics

### `InteractionKind`

- `PrimaryActivate`
  - the tray's main action
  - usually left click
- `SecondaryActivate`
  - a distinct alternate activation
  - commonly middle click on platforms that support it
  - semantically different from opening the menu
- `ContextMenu`
  - an explicit request to show the tray menu
  - usually right click

### `ScrollEvent`

- `Scroll` stays separate from `Interaction`.
- `delta` is backend-native integer magnitude for now.
- `axis` is horizontal or vertical.
- `position` and `area` are optional geometry hints, same as `InteractionEvent`.

The important point is preserving the semantic event, not prematurely forcing
all backends into the same scroll unit.

## Behavioral Rules

### Rule 1: The core stream is semantic

The default event stream should answer:

- what action happened
- where it happened, if known

It should not try to carry every raw platform detail by default.

### Rule 2: Do not invent provenance

If the backend cannot reliably tell whether activation came from pointer or
keyboard, the public event should remain semantic.

This is now the policy for Windows, because actual shell callback sequences can
make keyboard and pointer activation indistinguishable from tray callbacks alone
on some systems.

### Rule 3: Do not synthesize context-menu events just for internal policy

Emit `ContextMenu` when the backend surfaces a real semantic menu request that
application code may care about.

If `trayinit` simply opens its own declarative menu as an internal policy
decision, it does not need to emit an extra synthetic event.

### Rule 4: One semantic activation per gesture

The core stream should avoid duplicate semantic events for one logical user
gesture.

This is why:

- Windows follow-up notifications such as `NIN_SELECT` and `WM_CONTEXTMENU`
  should be deduplicated against already-emitted semantic pointer activations
- double-click needs an explicit policy before it is added to the core stream

### Rule 5: Raw hover and lifecycle detail are future extensions

Do not add these to the core event family yet:

- move
- enter
- leave
- button down
- button up

They are noisy, platform-skewed, and not required for the main cross-platform
tray story.

## Reference Behavior

### `tray-icon`

`tray-icon` exposes a much richer raw event stream on Windows and macOS,
including button down/up, enter, move, leave, and some double-click handling.

That is a useful implementation reference, but it is not the right public shape
for `trayinit`'s primary event API.

Relevant reference files:

- `D:\Projects\probe\tray-icon\src\lib.rs`
- `D:\Projects\probe\tray-icon\src\platform_impl\windows\mod.rs`
- `D:\Projects\probe\tray-icon\src\platform_impl\macos\mod.rs`

### `ksni`

`ksni` is the closer reference for public API shape:

- `activate(x, y)`
- `secondary_activate(x, y)`
- `scroll(delta, orientation)`

It is semantic rather than raw, which aligns with the direction here.

Relevant reference files:

- `D:\Projects\probe\ksni\src\lib.rs`
- `D:\Projects\probe\ksni\src\dbus_interface.rs`

## Platform Mapping

### Windows

Current semantic mapping:

- `WM_LBUTTONUP`
  - `PrimaryActivate`
- `WM_MBUTTONUP`
  - `SecondaryActivate`
- `WM_RBUTTONUP`
  - `ContextMenu`
- `NIN_SELECT`
  - semantic follow-up for primary activation
  - should be deduplicated if an equivalent activation was already emitted
- `WM_CONTEXTMENU`
  - semantic follow-up for menu request
  - should be deduplicated if an equivalent menu request was already emitted
- `NIN_KEYSELECT`
  - may represent semantic primary activation on some systems

Important note:

- Even with `NOTIFYICON_VERSION_4`, Windows shell behavior can still report
  keyboard-origin tray actions through the same left/right-style callback
  sequence as mouse input, followed by semantic shell notifications.
- Because that provenance is not reliable enough for the public API, the core
  `InteractionEvent` does not expose keyboard-vs-pointer detail.

Scroll is not implemented yet in the Windows backend and should remain
unsupported until verified.

### macOS

Expected semantic mapping:

- primary status-item action
  - `PrimaryActivate`
- alternate activation when the platform clearly surfaces one
  - `SecondaryActivate`
- menu request
  - `ContextMenu`

Notes:

- AppKit status items are fundamentally button/menu-oriented.
- macOS should be treated as semantic-first.
- Do not promise double-click or scroll until verified in the backend.

### Linux SNI / DBus

Expected semantic mapping:

- `Activate(x, y)`
  - `PrimaryActivate`
- `SecondaryActivate(x, y)`
  - `SecondaryActivate`
- `Scroll(delta, orientation)`
  - `Scroll`
- context-menu behavior
  - often host-owned rather than application-owned
  - may not yield an explicit callback at all

This is the clearest justification for keeping the public API semantic-first.

## Double-click

Double-click is intentionally not part of the current core semantic API.

The issue is policy, not low-level capability. On platforms like Windows, a
double-click is reported on top of normal click sequencing, so a naive public
API tends to create one of two bad outcomes:

- emit a normal activation and then a second double-click semantic event
- delay the single-click semantic action until the double-click timeout expires

Until there is a clear policy that does not make the main activation path worse,
double-click should stay out of the core semantic event family.

## Future Extensions

If we later decide the extra detail is worth exposing, the clean path is a
separate extension or separate event family for raw backend detail, such as:

- move
- enter
- leave
- button down/up
- double-click as a raw transport event
- keyboard/pointer provenance when it is actually reliable

That keeps the main `TrayEvent` story simple and cross-platform.

## Implementation Notes

### `trayinit` Windows backend

The Windows backend already has the architecture needed for the semantic model:

- semantic click mapping
- geometry extraction
- deduplication of shell follow-up notifications

The key policy is:

- emit the first semantic activation/menu request
- suppress equivalent follow-up semantic notifications from the shell
- do not try to expose provenance the shell cannot report consistently

### `menu_on_primary_click`

When `trayinit` internally decides to open its own declarative menu in response
to a primary interaction, that internal menu-opening policy does not need to
generate a public `ContextMenu` event by itself.

### `Scroll`

`Scroll` stays in the public API even though the Windows and macOS backends do
not implement it yet, because Linux SNI has a native semantic scroll surface and
it is a good fit for the cross-platform model.

## Recommended Next Steps

1. Keep the current public semantic API as-is.
2. Implement `Scroll` where the platform genuinely supports it.
3. Leave double-click out of the core stream until there is an explicit policy.
4. Consider a future raw-event extension only if real downstream use cases
   justify the extra complexity.

## Sources

- `trayinit` current API:
  - `D:\Projects\probe\trayinit\src\tray.rs`
- `tray-icon` event model:
  - `D:\Projects\probe\tray-icon\src\lib.rs`
  - `D:\Projects\probe\tray-icon\src\platform_impl\windows\mod.rs`
  - `D:\Projects\probe\tray-icon\src\platform_impl\macos\mod.rs`
- `ksni` semantic interaction model:
  - `D:\Projects\probe\ksni\src\lib.rs`
  - `D:\Projects\probe\ksni\src\dbus_interface.rs`
- Win32 notification icon references:
  - https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-notifyicondataw
  - https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw
- StatusNotifierItem specification:
  - https://specifications.freedesktop.org/status-notifier-item-spec/latest-single/
