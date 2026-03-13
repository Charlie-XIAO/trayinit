use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use trayinit::{ActionItem, CheckItem, Handle, MenuItem, Tray, TrayEvent, TrayMethods, TrayView};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum MenuId {
    Toggle,
    Quit,
}

struct MinimalTray {
    enabled: bool,
    keep_running: Arc<AtomicBool>,
}

impl Tray for MinimalTray {
    type MenuId = MenuId;

    fn id(&self) -> &str {
        "dev.trayinit.examples.minimal"
    }

    fn view(&self) -> TrayView<Self::MenuId> {
        let status = if self.enabled { "enabled" } else { "disabled" };
        TrayView {
            title: Some(format!("Minimal example: {status}")),
            tooltip: Some("trayinit minimal".into()),
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
            },
            TrayEvent::Menu(MenuId::Quit) => {
                self.keep_running.store(false, Ordering::Relaxed);
            },
            TrayEvent::Activate(_) | TrayEvent::SecondaryActivate(_) | TrayEvent::Scroll(_) => {},
        }
    }
}

fn main() {
    let keep_running = Arc::new(AtomicBool::new(true));
    let handle: Handle<MinimalTray> = MinimalTray {
        enabled: true,
        keep_running: Arc::clone(&keep_running),
    }
    .spawn()
    .expect("spawn minimal tray example");

    println!("Running minimal tray example.");
    println!("Startup mode: spawn() self-hosted tray.");
    println!("Use the tray icon menu to toggle state or quit.");

    while keep_running.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(100));
    }

    handle.shutdown().expect("shutdown minimal tray example");
}
