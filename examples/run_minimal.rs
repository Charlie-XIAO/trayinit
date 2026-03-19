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
struct RunMinimalTray {
    enabled: bool,
    quit_requested: bool,
}

#[cfg(not(target_os = "macos"))]
impl Tray for RunMinimalTray {
    type Message = Message;

    fn id(&self) -> &str {
        "dev.trayinit.examples.run-minimal"
    }

    fn tooltip(&self) -> Option<String> {
        Some("trayinit run() minimal".into())
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
                self.quit_requested = true;
            },
            TrayEvent::Interaction(_) | TrayEvent::Scroll(_) => {},
            _ => {},
        }
    }

    fn should_exit(&self) -> bool {
        self.quit_requested
    }
}

#[cfg(target_os = "macos")]
fn main() {
    tracing_subscriber::fmt::init();
    eprintln!("This example uses run(), which is not implemented on macOS yet.");
    eprintln!("Use the attach()-based examples like winit_window or winit_no_window instead.");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    tracing_subscriber::fmt::init();

    let tray = RunMinimalTray {
        enabled: false,
        quit_requested: false,
    };

    println!("Running minimal tray example.");
    println!("Startup mode: run() standalone tray.");
    println!("Use the tray icon menu to toggle state or quit.");

    tray.run().expect("run minimal tray example");
}
