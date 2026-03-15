# Event API Proposal

This note proposes how `trayinit` should expose tray interaction events without
throwing away important platform detail.

The current API is too lossy:

- `SecondaryActivate` conflates at least right-click and middle-click on Windows.
- double-click has no clean representation.
- `Scroll` exists, but the overall event model does not clearly describe what is
  semantic, what is trigger detail, and what is platform-specific.

The goal is to improve this without copying `tray-icon`'s fully raw event stream
into the core reactive API.

## Goals

- Keep one primary event surface for `Tray::event(...)`.
- Preserve cross-platform semantic meaning: primary activation, secondary
  activation, context-menu request, scroll.
- Preserve button and click-phase detail when a backend can report it.
- Do not invent fake raw detail on platforms that only expose high-level
  semantics.
- Keep room for future extensions such as hover or move events without forcing
  them into the first revision.

## Non-goals

- A full raw pointer event stream in the core API.
- Perfect parity across all platforms.
- Promising hover, move, enter, or leave as part of the core tray API today.

Those can be added later if we decide they are important enough and we can do so
without making the default event story noisy or platform-fragile.

## Current API

Today `trayinit` exposes:

```rust
pub enum TrayEvent<Message> {
    Activate(ActivateEvent),
    SecondaryActivate(ActivateEvent),
    Scroll(ScrollEvent),
    Menu(Message),
}
```

This loses too much information:

- primary vs secondary is semantic, but not enough for right vs middle
- click phase is lost
- double-click is lost
- context-menu request is implicit instead of explicit

## Reference Behavior

### `tray-icon`

`tray-icon` is intentionally much richer on Windows and macOS:

- Windows:
  - click `Down` / `Up` for left, right, and middle buttons
  - `DoubleClick` for left, right, and middle buttons
  - `Enter`, `Move`, `Leave`
- macOS:
  - click `Down` / `Up` for left, right, and middle buttons
  - `Enter`, `Move`, `Leave`
  - current implementation does not emit double-click
- Linux:
  - no tray icon interaction events at all in the crate's GTK path

Relevant reference files:

- `D:\Projects\probe\tray-icon\src\lib.rs`
- `D:\Projects\probe\tray-icon\src\platform_impl\windows\mod.rs`
- `D:\Projects\probe\tray-icon\src\platform_impl\macos\mod.rs`

### `ksni`

`ksni` is semantic rather than raw:

- `activate(x, y)`
- `secondary_activate(x, y)`
- `scroll(delta, orientation)`

It does not expose raw button up/down or hover state.

Its D-Bus interface also has a `ContextMenu(x, y)` method in the StatusNotifier
spec, but `ksni` intentionally does not implement a self-rendered context-menu
callback there and instead relies on the exported menu model.

Relevant reference files:

- `D:\Projects\probe\ksni\src\lib.rs`
- `D:\Projects\probe\ksni\src\dbus_interface.rs`

## Proposed Public API

Use one semantic event stream, but attach richer trigger detail when available.

```rust
pub enum TrayEvent<Message> {
    Menu(Message),
    Interaction(InteractionEvent),
    Scroll(ScrollEvent),
}

pub struct InteractionEvent {
    pub kind: InteractionKind,
    pub trigger: InteractionTrigger,
    pub position: Option<PhysicalPosition<i32>>,
    pub area: Option<(PhysicalPosition<i32>, PhysicalSize<i32>)>,
}

pub enum InteractionKind {
    PrimaryActivate,
    SecondaryActivate,
    ContextMenu,
}

pub enum InteractionTrigger {
    Pointer(PointerTrigger),
    Keyboard,
    Unknown,
}

pub struct PointerTrigger {
    pub button: PointerButton,
    pub phase: PointerPhase,
}

pub enum PointerButton {
    Left,
    Right,
    Middle,
    Other(u16),
}

pub enum PointerPhase {
    Down,
    Up,
    DoubleClick,
}

pub struct ScrollEvent {
    pub delta: i32,
    pub axis: ScrollAxis,
    pub position: Option<PhysicalPosition<i32>>,
    pub area: Option<(PhysicalPosition<i32>, PhysicalSize<i32>)>,
}
```

## Why This Shape

This keeps the semantic part front and center:

- most apps care about "what should happen"
- not about every raw mouse transition

But it still gives richer detail when the backend has it:

- right-click vs middle-click no longer need to collapse into one bucket
- double-click has a clean place to go
- keyboard-triggered activation can be represented later

Most importantly, it avoids having two parallel event systems:

- a "semantic" one
- and a "raw" one

That duplication would make downstream handling awkward, because the same user
gesture could arrive twice in different forms.

## Semantics

### `InteractionKind`

- `PrimaryActivate`
  - the tray's main action
  - usually left click
- `SecondaryActivate`
  - a less important alternate action
  - usually middle click on SNI
  - may come from other buttons on other platforms if that is the platform's
    real behavior
- `ContextMenu`
  - an explicit request to open or show the tray menu
  - usually right click
  - may also be primary click when `menu_on_primary_click()` is enabled

### `InteractionTrigger`

- `Pointer(...)`
  - the backend knows which pointer button and which phase caused the
    interaction
- `Keyboard`
  - the backend knows the interaction came from keyboard navigation or keyboard
    activation
- `Unknown`
  - the backend only has high-level semantic information
  - do not synthesize fake button or phase information in this case

### `ScrollEvent`

- `Scroll` stays first-class rather than being folded into `Interaction`
- `delta` is backend-native integer scroll magnitude
- `axis` is horizontal or vertical
- `position` / `area` are optional, same as interaction events

For the first revision, the important property is preserving semantic scroll,
not trying to normalize every backend into pixels or lines.

## Behavioral Rules

### Rule 1: Semantic first, trigger detail second

If the backend can say "this was a primary activation caused by right-button up"
then the event should be:

```rust
TrayEvent::Interaction(InteractionEvent {
    kind: InteractionKind::PrimaryActivate,
    trigger: InteractionTrigger::Pointer(PointerTrigger {
        button: PointerButton::Right,
        phase: PointerPhase::Up,
    }),
    ..
})
```

If the backend only knows "secondary activate happened", use:

```rust
TrayEvent::Interaction(InteractionEvent {
    kind: InteractionKind::SecondaryActivate,
    trigger: InteractionTrigger::Unknown,
    ..
})
```

### Rule 2: Never invent raw detail

Do not guess:

- left click
- button up
- double click

when the platform API did not actually report it.

### Rule 3: Menu-opening policy should be observable

If `menu_on_primary_click()` causes a click to open the tray menu instead of
performing the tray's main action, the emitted semantic event should be
`ContextMenu`, not `PrimaryActivate`.

This matters because menu-open policy is part of user-visible behavior and
should not disappear from the event stream.

### Rule 4: No noisy hover stream in the first revision

Even though Windows and macOS can support enter / move / leave, they should not
be part of the first core proposal.

Reasons:

- Linux SNI does not expose them
- they are noisy
- they are not part of the main tray interaction model most apps need

If we want them later, add them explicitly as a separate extension rather than
smuggling them into the core API now.

## Platform Mapping

### Windows

Likely mapping:

- `WM_LBUTTONUP`
  - `PrimaryActivate`
  - trigger: `Pointer { Left, Up }`
- `WM_MBUTTONUP`
  - `SecondaryActivate`
  - trigger: `Pointer { Middle, Up }`
- `WM_RBUTTONUP`
  - `ContextMenu`
  - trigger: `Pointer { Right, Up }`
- `WM_*DBLCLK`
  - same semantic kind as the platform policy would normally produce
  - trigger phase: `DoubleClick`

Notes:

- `tray-icon` already demonstrates that Windows tray callbacks can surface left,
  right, and middle click up/down as well as double-click.
- Whether keyboard-triggered tray activation should be exposed depends on a
  later Windows refinement around notification icon versioning and callback
  messages. It should not block this API proposal.
- Scroll is not currently proven in our Windows backend path and should remain
  unsupported until verified.

### macOS

Likely mapping:

- left / right / middle mouse down / up can be observed from the status item
  target view
- current `tray-icon` implementation shows that enter / move / leave are also
  possible via tracking areas

Notes:

- The reference implementation does not currently emit double-click even though
  AppKit may make it possible to infer from event data.
- For the first revision, we should not promise macOS double-click until we
  verify the behavior in implementation.
- Scroll is also not yet part of the reference implementation we are following.

### Linux SNI / DBus

Likely mapping:

- `Activate(x, y)`
  - `PrimaryActivate`
  - trigger: `Unknown`
- `SecondaryActivate(x, y)`
  - `SecondaryActivate`
  - trigger: `Unknown`
- `Scroll(delta, orientation)`
  - `Scroll`
- context-menu open:
  - often host-owned rather than application-owned
  - may not result in any explicit callback at all

Notes:

- This is the clearest example of why the trigger must be optional and why we
  should not synthesize fake button information.
- It is also why hover and move should not be part of the first core API.

## Implementation Notes

### `trayinit` Windows backend

Current `trayinit` already receives the callback messages needed to improve the
event model:

- `WM_LBUTTONUP`
- `WM_RBUTTONUP`
- `WM_MBUTTONUP`
- `WM_*DBLCLK`

So the Windows backend can move to the proposed `InteractionEvent` shape
without architectural changes.

### `menu_on_primary_click`

Today `trayinit` uses policy directly in the Windows backend:

- left click either opens the menu or emits `Activate`
- right click either opens the menu or emits `SecondaryActivate`

Under the proposed model, that becomes clearer:

- menu open request: `ContextMenu`
- tray main action: `PrimaryActivate`
- less important alternate action: `SecondaryActivate`

### Future raw extensions

If we later want richer raw events such as:

- enter
- move
- leave
- raw button down / up independent of semantic action

the clean path is to add them as a separate extension or separate event family,
not to overload the first semantic revision.

## Recommended Next Step

1. Replace the current `Activate` / `SecondaryActivate` variants with a single
   `Interaction(InteractionEvent)` variant.
2. Keep `Menu(Message)` and `Scroll(ScrollEvent)`.
3. Update Windows to emit:
   - `PrimaryActivate`
   - `SecondaryActivate`
   - `ContextMenu`
   with pointer trigger detail.
4. When Linux SNI lands, map its semantic callbacks to the same
   `InteractionEvent` shape using `InteractionTrigger::Unknown`.
5. Defer hover / move / leave until we have evidence that they are worth the
   cross-platform complexity.

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
