use trayinit::menu::{CheckItem, MenuItem, StandardItem};
use trayinit::{Tray, TrayEvent, TrayMethods};

#[derive(Debug, Copy, Clone)]
enum Message {
    Toggle,
    Quit,
}

struct RunMinimalTray {
    enabled: bool,
    quit_requested: bool,
}

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

fn main() {
    tracing_subscriber::fmt::init();

    #[cfg(target_os = "macos")]
    {
        eprintln!("This example uses run(), which is not implemented on macOS yet.");
        eprintln!("Use the attach()-based examples like winit_window or winit_no_window instead.");
        return;
    }

    let tray = RunMinimalTray {
        enabled: false,
        quit_requested: false,
    };

    println!("Running minimal tray example.");
    println!("Startup mode: run() standalone tray.");
    println!("Use the tray icon menu to toggle state or quit.");

    tray.run().expect("run minimal tray example");
}
