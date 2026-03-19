#[cfg(not(target_os = "macos"))]
use std::sync::Arc;
#[cfg(not(target_os = "macos"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_os = "macos"))]
use std::thread;
#[cfg(not(target_os = "macos"))]
use std::time::Duration;

#[cfg(not(target_os = "macos"))]
use trayinit::menu::{CheckItem, MenuItem, StandardItem};
#[cfg(not(target_os = "macos"))]
use trayinit::{Tray, TrayEvent, TrayMethods};

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Copy, Clone)]
enum Message {
    Toggle,
    Quit,
}

#[cfg(not(target_os = "macos"))]
struct MinimalTray {
    enabled: bool,
    keep_running: Arc<AtomicBool>,
}

#[cfg(not(target_os = "macos"))]
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

#[cfg(target_os = "macos")]
fn main() {
    tracing_subscriber::fmt::init();
    eprintln!("This example uses spawn(), which is not implemented on macOS yet.");
    eprintln!("Use the attach()-based examples like winit_window or winit_no_window instead.");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    tracing_subscriber::fmt::init();

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
