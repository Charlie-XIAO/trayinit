#[cfg(windows)]
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

#[cfg(windows)]
use trayinit::{
    ActionItem, CheckItem, Handle, MenuItem, Tooltip, Tray, TrayEvent, TrayMethods, TrayView,
};

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum MenuId {
    Toggle,
    Quit,
}

#[cfg(windows)]
struct MinimalTray {
    enabled: bool,
    keep_running: Arc<AtomicBool>,
}

#[cfg(windows)]
impl Tray for MinimalTray {
    type MenuId = MenuId;

    fn id(&self) -> &str {
        "dev.trayinit.examples.minimal"
    }

    fn view(&self) -> TrayView<Self::MenuId> {
        let status = if self.enabled { "enabled" } else { "disabled" };
        TrayView {
            title: Some(format!("Minimal example: {status}")),
            tooltip: Some(Tooltip::new(
                "trayinit minimal",
                "Toggle the checkbox or quit from the tray menu.",
            )),
            menu: vec![
                CheckItem::new(MenuId::Toggle, "Enabled", self.enabled).into(),
                MenuItem::Separator,
                ActionItem::new(MenuId::Quit, "Quit").into(),
            ],
            ..Default::default()
        }
    }

    fn event(&mut self, event: TrayEvent<Self::MenuId>) {
        match event {
            TrayEvent::Menu(MenuId::Toggle) => {
                self.enabled = !self.enabled;
            }
            TrayEvent::Menu(MenuId::Quit) => {
                self.keep_running.store(false, Ordering::Relaxed);
            }
            TrayEvent::Activate(_) | TrayEvent::SecondaryActivate(_) | TrayEvent::Scroll(_) => {}
        }
    }
}

#[cfg(windows)]
fn main() {
    let keep_running = Arc::new(AtomicBool::new(true));
    let handle: Handle<MinimalTray> = MinimalTray {
        enabled: true,
        keep_running: Arc::clone(&keep_running),
    }
    .spawn()
    .expect("spawn minimal tray example");

    println!("Running minimal tray example.");
    println!("Use the tray icon menu to toggle state or quit.");

    while keep_running.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(100));
    }

    handle.shutdown().expect("shutdown minimal tray example");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This example currently requires Windows.");
}
