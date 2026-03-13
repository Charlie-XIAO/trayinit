use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use trayinit::menu::{CheckItem, MenuItem, RadioGroup, RadioItem, StandardItem, Submenu};
use trayinit::{Icon, Tray, TrayEvent, TrayMethods};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Message {
    ToggleTicks,
    TogglePrimaryClickMenu,
    ResetTicks,
    AccentRed,
    AccentGreen,
    AccentBlue,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Accent {
    Red,
    Green,
    Blue,
}

impl Accent {
    fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Red => (0xE0, 0x52, 0x52),
            Self::Green => (0x2D, 0xB0, 0x72),
            Self::Blue => (0x3E, 0x7B, 0xF6),
        }
    }

    fn selected_id(self) -> Message {
        match self {
            Self::Red => Message::AccentRed,
            Self::Green => Message::AccentGreen,
            Self::Blue => Message::AccentBlue,
        }
    }
}

struct ShowcaseTray {
    ticks_enabled: bool,
    menu_on_primary_click: bool,
    tick_count: u32,
    accent: Accent,
    keep_running: Arc<AtomicBool>,
}

impl Tray for ShowcaseTray {
    type Message = Message;

    fn id(&self) -> &str {
        "dev.trayinit.examples.showcase"
    }

    fn icon(&self) -> Option<Icon> {
        Some(make_icon(self.accent, self.ticks_enabled))
    }

    fn title(&self) -> Option<String> {
        Some(format!("Showcase ticks: {}", self.tick_count))
    }

    fn tooltip(&self) -> Option<String> {
        Some(format!(
            "trayinit showcase: ticks={}, timer={}, left-click menu={}",
            self.tick_count,
            on_off(self.ticks_enabled),
            on_off(self.menu_on_primary_click),
        ))
    }

    fn menu_on_primary_click(&self) -> bool {
        self.menu_on_primary_click
    }

    fn menu(&self) -> Vec<MenuItem<Self::Message>> {
        vec![
            CheckItem::new(
                "Advance ticks every second",
                self.ticks_enabled,
                Message::ToggleTicks,
            )
            .into(),
            CheckItem::new(
                "Open menu on left click",
                self.menu_on_primary_click,
                Message::TogglePrimaryClickMenu,
            )
            .into(),
            StandardItem::new("Reset tick counter", Message::ResetTicks).into(),
            MenuItem::Separator,
            RadioGroup {
                selected: Some(self.accent.selected_id()),
                options: vec![
                    RadioItem::new("Accent: Red", Message::AccentRed),
                    RadioItem::new("Accent: Green", Message::AccentGreen),
                    RadioItem::new("Accent: Blue", Message::AccentBlue),
                ],
                enabled: true,
                visible: true,
            }
            .into(),
            MenuItem::Submenu(Submenu::new(
                "Session",
                vec![
                    StandardItem::new("Reset tick counter", Message::ResetTicks).into(),
                    MenuItem::Separator,
                    StandardItem::new("Quit", Message::Quit).into(),
                ],
            )),
        ]
    }

    fn event(&mut self, event: TrayEvent<Self::Message>) {
        match event {
            TrayEvent::Menu(Message::ToggleTicks) => {
                self.ticks_enabled = !self.ticks_enabled;
            },
            TrayEvent::Menu(Message::TogglePrimaryClickMenu) => {
                self.menu_on_primary_click = !self.menu_on_primary_click;
            },
            TrayEvent::Menu(Message::ResetTicks) => {
                self.tick_count = 0;
            },
            TrayEvent::Menu(Message::AccentRed) => {
                self.accent = Accent::Red;
            },
            TrayEvent::Menu(Message::AccentGreen) => {
                self.accent = Accent::Green;
            },
            TrayEvent::Menu(Message::AccentBlue) => {
                self.accent = Accent::Blue;
            },
            TrayEvent::Menu(Message::Quit) => {
                self.keep_running.store(false, Ordering::Relaxed);
            },
            TrayEvent::Activate(_) | TrayEvent::SecondaryActivate(_) | TrayEvent::Scroll(_) => {},
        }
    }
}

fn main() {
    let keep_running = Arc::new(AtomicBool::new(true));
    let tray = ShowcaseTray {
        ticks_enabled: true,
        menu_on_primary_click: false,
        tick_count: 0,
        accent: Accent::Blue,
        keep_running: Arc::clone(&keep_running),
    };
    let handle = tray.spawn().expect("spawn showcase tray example");

    println!("Running showcase tray example.");
    println!("Startup mode: spawn() self-hosted tray.");
    println!("Features in this example:");
    println!("- generated tray icon");
    println!("- tooltip/title updates");
    println!("- check items");
    println!("- radio group");
    println!("- submenu");
    println!("- external state updates via Handle::update");

    while keep_running.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_secs(1));

        if !keep_running.load(Ordering::Relaxed) {
            break;
        }

        handle
            .update(|tray| {
                if tray.ticks_enabled {
                    tray.tick_count = tray.tick_count.saturating_add(1);
                }
            })
            .expect("update showcase tray example");
    }

    handle.shutdown().expect("shutdown showcase tray example");
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn make_icon(accent: Accent, active: bool) -> Icon {
    let (r, g, b) = accent.rgb();
    let width = 32usize;
    let height = 32usize;
    let mut rgba = vec![0; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let offset = (y * width + x) * 4;
            let border = x < 2 || y < 2 || x >= width - 2 || y >= height - 2;
            let active_band = active && x > 9 && x < 23 && y > 9 && y < 23;

            let (pr, pg, pb, pa) = if border {
                (0x18, 0x1A, 0x1F, 0xFF)
            } else if active_band {
                (0xF8, 0xFB, 0xFF, 0xFF)
            } else {
                (r, g, b, 0xFF)
            };

            rgba[offset] = pr;
            rgba[offset + 1] = pg;
            rgba[offset + 2] = pb;
            rgba[offset + 3] = pa;
        }
    }

    Icon::from_rgba(rgba, width as u32, height as u32).expect("valid generated icon")
}
