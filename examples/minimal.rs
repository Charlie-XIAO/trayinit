use trayinit::{Icon, Menu, MenuNode, Tray, TrayEvent, TrayState, channel};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let icon_size = platform_icon_size();
    let icon = Icon::from_rgba(
        checker_icon_rgba(icon_size),
        icon_size as u32,
        icon_size as u32,
    )?;
    let state = base_state()
        .with_icon(icon)
        .with_tooltip("trayinit")
        .with_menu(Menu::new([
            MenuNode::item("open", "Open"),
            MenuNode::separator(),
            MenuNode::item("quit", "Quit"),
        ]));

    let (sink, events) = channel();
    let tray = Tray::new(state, sink)?;

    // This is intentionally a tiny blocking smoke example. Real winit/iced
    // integrations should forward tray events into the application event loop.
    for event in events {
        println!("{event:?}");
        if matches!(
            event,
            TrayEvent::MenuItemActivated { item_id, .. } if item_id.as_str() == "quit"
        ) {
            break;
        }
    }

    tray.shutdown()?;
    Ok(())
}

fn base_state() -> TrayState {
    let state = TrayState::new();

    #[cfg(target_os = "linux")]
    {
        state.with_title("trayinit")
    }

    #[cfg(not(target_os = "linux"))]
    {
        state
    }
}

fn platform_icon_size() -> usize {
    #[cfg(target_os = "linux")]
    {
        64
    }

    #[cfg(not(target_os = "linux"))]
    {
        32
    }
}

fn checker_icon_rgba(size: usize) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let light = ((x / 8) + (y / 8)) % 2 == 0;
            let (r, g, b) = checker_colors(light);
            rgba.extend_from_slice(&[r, g, b, 0xff]);
        }
    }
    rgba
}

#[cfg(target_os = "linux")]
fn checker_colors(light: bool) -> (u8, u8, u8) {
    if light {
        (0x4c, 0x78, 0xa8)
    } else {
        (0x1f, 0x28, 0x36)
    }
}

#[cfg(not(target_os = "linux"))]
fn checker_colors(light: bool) -> (u8, u8, u8) {
    if light {
        (0x26, 0xa6, 0x9a)
    } else {
        (0x24, 0x2a, 0x32)
    }
}
