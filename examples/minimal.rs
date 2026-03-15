use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use trayinit::menu::{CheckItem, MenuItem, StandardItem};
use trayinit::{Tray, TrayEvent, TrayMethods};

#[derive(Debug, Copy, Clone)]
enum Message {
    Toggle,
    Quit,
}

struct MinimalTray {
    enabled: bool,
    keep_running: Arc<AtomicBool>,
}

impl Tray for MinimalTray {
    type Message = Message;

    fn id(&self) -> &str {
        "dev.trayinit.examples.minimal"
    }

    fn title(&self) -> Option<String> {
        let status = if self.enabled { "enabled" } else { "disabled" };
        Some(format!("Minimal example: {status}"))
    }

    fn tooltip(&self) -> Option<String> {
        Some("trayinit minimal".into())
    }

    fn menu(&self) -> Vec<MenuItem<Self::Message>> {
        vec![
            CheckItem::new("Enabled", self.enabled, Message::Toggle).into(),
            MenuItem::Separator,
            StandardItem::new("Quit", Message::Quit).into(),
        ]
    }

    fn event(&mut self, event: TrayEvent<Self::Message>) {
        match event {
            TrayEvent::Menu(Message::Toggle) => {
                self.enabled = !self.enabled;
            },
            TrayEvent::Menu(Message::Quit) => {
                self.keep_running.store(false, Ordering::Relaxed);
            },
            TrayEvent::Interaction(_) | TrayEvent::Scroll(_) => {},
            _ => {},
        }
    }
}

fn main() {
    let keep_running = Arc::new(AtomicBool::new(true));
    let tray = MinimalTray {
        enabled: false,
        keep_running: Arc::clone(&keep_running),
    };
    let handle = tray.spawn().expect("spawn minimal tray example");

    println!("Running minimal tray example.");
    println!("Startup mode: spawn() self-hosted tray.");
    println!("Use the tray icon menu to toggle state or quit.");

    while keep_running.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(100));
    }

    handle.shutdown().expect("shutdown minimal tray example");
}
