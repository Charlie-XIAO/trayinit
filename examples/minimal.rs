mod common;

use anyhow::Result;
use trayinit::{Menu, MenuNode, Tray, TrayEvent, TrayState};

const OPEN_ID: &str = "open";
const QUIT_ID: &str = "quit";

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn main() -> Result<()> {
    let (sink, events) = trayinit::channel();
    let tray = Tray::new(tray_state()?, sink)?;

    for event in events {
        println!("{event:?}");
        if is_quit_event(&event) {
            break;
        }
    }

    tray.shutdown()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn main() -> Result<()> {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    let mtm = MainThreadMarker::new().expect("not running on main thread");

    let app = NSApplication::sharedApplication(mtm);
    app.finishLaunching();

    let tray = Tray::new(tray_state()?, move |event| {
        println!("{event:?}");
        if is_quit_event(&event) {
            let mtm = MainThreadMarker::new().expect("not running on main thread");
            NSApplication::sharedApplication(mtm).terminate(None);
        }
    })?;

    app.run();
    tray.shutdown()?;
    Ok(())
}

fn tray_state() -> Result<TrayState> {
    Ok(TrayState::new()
        .with_title("trayinit")
        .with_icon(common::icon()?)
        .with_tooltip("trayinit")
        .with_menu(Menu::new([
            MenuNode::item(OPEN_ID, "Open"),
            MenuNode::separator(),
            MenuNode::item(QUIT_ID, "Quit"),
        ])))
}

fn is_quit_event(event: &TrayEvent) -> bool {
    matches!(
        event,
        TrayEvent::MenuItemActivated { item_id, .. } if item_id.as_str() == QUIT_ID
    )
}
