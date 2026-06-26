use trayinit::{Icon, Menu, MenuNode, Tray, TrayEvent, TrayState, channel};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let icon = Icon::from_rgba(checker_icon_rgba(64), 64, 64)?;

    let state = TrayState::new()
        .with_title("trayinit")
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

fn checker_icon_rgba(size: usize) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let light = ((x / 8) + (y / 8)) % 2 == 0;
            let (r, g, b) = if light {
                (0x26, 0xa6, 0x9a)
            } else {
                (0x24, 0x2a, 0x32)
            };
            rgba.extend_from_slice(&[r, g, b, 0xff]);
        }
    }
    rgba
}
